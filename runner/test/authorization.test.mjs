import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, rm, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { BACKGROUND_CONTEXT as context, withAbortSignal } from '@earendil-works/chord/context';
import { createModels, fauxProvider, fauxAssistantMessage, fauxText, fauxToolCall } from '@earendil-works/pi-ai';
import { Harness, createRegistry, defineExtension, defineTool, hook, ToolTask } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { Authorization, guardProvider, guardTool, hostClockUpperBound } from '../src/authorization.ts';

// Also deserialized, validated and round-tripped by forge-app's lease tests.
const serializedGrant = JSON.parse(await readFile(new URL('./authorization-lease.json', import.meta.url), 'utf8'));
const { launch, owner } = serializedGrant;
function grant(sequence = 1, issued = 1000) {
  return { ...structuredClone(serializedGrant), sequence, issued_boottime_millis: issued, expires_boottime_millis: issued + 30000 };
}
function fixture(t, authorized = true) {
  const clock = { now: 1000, fail: false };
  const authorization = new Authorization(launch, 1, owner, 'epoch', () => { if (clock.fail) throw new Error('private clock detail'); return clock.now; });
  t.after(() => authorization.close());
  if (authorized) authorization.accept(grant());
  return { clock, authorization };
}
const denied = { code: 'assignment-lease-denied' };
const answer = text => fauxAssistantMessage([fauxText(text)]);
const toolCall = name => fauxAssistantMessage([fauxToolCall(name, {}, { id: 'call' })], { stopReason: 'toolUse' });

test('admission boottime rounds conservatively and never falls back to wall time', () => {
  assert.equal(hostClockUpperBound(() => '123.45 999.00\n'), 123460);
  assert.equal(hostClockUpperBound(() => '123.99 999.00\n'), 124000);
  for (const input of ['', 'NaN 0', '1.001 1.00', '-1.00 1.00', '9007199254740992.00 0.00']) {
    assert.throws(() => hostClockUpperBound(() => input), denied);
  }
  assert.throws(() => hostClockUpperBound(() => { throw new Error('unavailable'); }));
  assert.ok(hostClockUpperBound() > 0);
});

test('initial permission, sequential renewal, exact retry and exact expiry', t => {
  const { clock, authorization: a } = fixture(t, false);
  assert.throws(() => a.admit(), denied);
  a.accept(grant()); a.admit();
  clock.now = 11000;
  const renewal = grant(2, clock.now);
  a.accept(renewal);
  // Neither an exact retry nor mutation of the original object extends time.
  a.accept(structuredClone(renewal));
  renewal.expires_boottime_millis = 999999;
  clock.now = 40999; a.admit();
  clock.now = 41000;
  assert.throws(() => a.admit(), denied);
  assert.equal(a.signal.aborted, true);
  assert.throws(() => a.accept(grant(3, clock.now)), denied);
});

test('cooperative timer cancels active permission without another admission', async t => {
  const { clock, authorization: a } = fixture(t);
  const cancelled = new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('cooperative cancellation was not observed')), 2000);
    a.signal.addEventListener('abort', () => { clearTimeout(timeout); resolve(); }, { once: true });
  });
  clock.now = 31000;
  await cancelled;
  assert.throws(() => a.admit(), denied);
});

test('binding, owner, epoch, sequence, bounds and unknown fields fail terminally', t => {
  const changes = [
    g => { g.launch.attempt_id = 'foreign'; },
    g => { g.generation++; },
    g => { g.owner.boot_id = 'new-boot'; },
    g => { g.owner.time_namespace = 'time:[2]'; },
    g => { g.owner.worker_start = 'new-process'; },
    g => { g.daemon_epoch = 'new-daemon'; },
    g => { g.sequence = 3; },
    g => { g.issued_boottime_millis++; },
    g => { g.expires_boottime_millis++; },
    g => { g.sequence = Number.MAX_SAFE_INTEGER + 1; },
    g => { g.unknown = true; },
    g => { g.owner.unknown = true; },
  ];
  for (const change of changes) {
    const { clock, authorization: a } = fixture(t);
    clock.now = 11000;
    const next = grant(2, clock.now); change(next);
    assert.throws(() => a.accept(next), denied);
    assert.equal(a.signal.aborted, true);
    assert.throws(() => a.accept(grant(2, clock.now)), denied);
  }
});

test('lost connectivity, expired delivery, clock failure/backward readings and restart cannot revive', t => {
  for (const fault of ['expiry', 'backward', 'unavailable']) {
    const { clock, authorization: a } = fixture(t);
    if (fault === 'expiry') clock.now = 31000;
    if (fault === 'backward') clock.now = 999;
    if (fault === 'unavailable') clock.fail = true;
    assert.throws(() => a.check(), denied);
    clock.now = 11000; clock.fail = false;
    assert.throws(() => a.accept(grant(2, clock.now)), denied);
  }
  const { clock, authorization: a } = fixture(t);
  clock.now = 31000;
  assert.throws(() => a.accept(grant(2, clock.now)), denied);
  const restarted = fixture(t, false);
  restarted.clock.now = 11000;
  assert.throws(() => restarted.authorization.accept(grant(2, 11000)), denied);
  const newDaemon = new Authorization(launch, 1, owner, 'new-daemon', () => 11000);
  t.after(() => newDaemon.close());
  // Even sequence 1 from a still-unexpired prior lifetime is not new authority.
  assert.throws(() => newDaemon.accept(grant()), denied);
  const delayed = fixture(t, false);
  delayed.clock.now = 31000;
  assert.throws(() => delayed.authorization.accept(grant()), denied);
});

test('all pinned provider effects are gated; cleanup remains callable; signals combine', t => {
  const { clock, authorization: a } = fixture(t);
  const methods = ['stream', 'streamSimple', 'fetchDeferred', 'generateImages', 'classify'];
  let calls = 0;
  const signals = [];
  const provider = Object.fromEntries(methods.map(name => [name, function (_model, _input, options) { assert.equal(this, provider); signals.push(options.signal); calls++; }]));
  provider.cancelDeferred = () => { calls++; };
  const guarded = guardProvider(provider, a);
  const parent = new AbortController();
  for (const method of methods) guarded[method]({}, {}, { signal: parent.signal });
  assert.equal(calls, 5);
  parent.abort();
  assert.ok(signals.every(s => s.aborted));
  assert.throws(() => guarded.stream({}, {}, { signal: parent.signal }));
  clock.now = 31000;
  for (const method of methods) assert.throws(() => guarded[method]({}, {}), denied);
  assert.equal(calls, 5);
  guarded.cancelDeferred({}, {});
  assert.equal(calls, 6);
});

test('tool boundary combines cancellation and refuses already cancelled callers', t => {
  const { authorization: a } = fixture(t);
  const caller = new AbortController();
  let calls = 0;
  let signal;
  const execute = guardTool((_args, _api, ctx) => { calls++; signal = ctx.abortSignal; }, a);
  execute({}, {}, withAbortSignal(caller.signal, context));
  caller.abort(); assert.equal(signal.aborted, true);
  assert.throws(() => execute({}, {}, withAbortSignal(caller.signal, context)));
  assert.equal(calls, 1);
  execute({}, {}, context);
  a.close(); assert.equal(signal.aborted, true);
  assert.throws(() => execute({}, {}, context), denied);
});

// Pinned real Harness/storage with a controlled provider. No ambient auth,
// network, Forge capability, production delivery or independent watchdog.
async function open(t, a, { path, responses = [], tools = [], hooks = [] } = {}) {
  if (!path) {
    const directory = await mkdtemp(join(tmpdir(), 'forge-t05-guard-'));
    t.after(() => rm(directory, { recursive: true, force: true }));
    path = join(directory, 'runtime.sqlite');
  }
  const provider = fauxProvider(); provider.setResponses(responses);
  const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
  models.setProvider(guardProvider(provider.provider, a));
  const registry = createRegistry();
  if (tools.length || hooks.length) registry.install(defineExtension({ name: 'controlled', hooks, tools: tools.map(tool => defineTool({
    parameters: { type: 'object', properties: {}, additionalProperties: false }, description: 'Controlled test effect', replay: 'unsafe', ...tool, execute: guardTool(tool.execute, a),
  })) }));
  const harness = await Harness.open(await openNodeSqliteStorage(path), { models, registry, settings: { retry: { enabled: false }, compaction: { enabled: false, reserveTokens: 1000, keepRecentTokens: 1, backgroundTokens: 0 } }, env: () => undefined }, context);
  t.after(() => harness.close(context));
  const root = await harness.root(context, { agent: { model: { provider: 'faux', modelId: 'faux-1' } } });
  return { harness, root, path };
}
const submit = async root => (await root.submit({ type: 'input', content: 'controlled input' }, context)).wait(context);

test('expired authority blocks initial and same-store resumed provider requests', async t => {
  const { clock, authorization: a } = fixture(t);
  let calls = 0;
  clock.now = 31000;
  const options = { responses: [() => { calls++; return answer('forbidden'); }] };
  const first = await open(t, a, options);
  await submit(first.root); await first.harness.close(context);
  const second = await open(t, a, { ...options, path: first.path });
  await submit(second.root);
  assert.equal(calls, 0);
});

test('connectivity loss cancels controlled in-flight generation and denies subsequent requests', async t => {
  const { clock, authorization: a } = fixture(t);
  const started = Promise.withResolvers(), cancelled = Promise.withResolvers();
  let calls = 0;
  const { root } = await open(t, a, { responses: [(_input, options) => new Promise((_resolve, reject) => {
    calls++;
    options.signal.addEventListener('abort', () => { cancelled.resolve(); reject(options.signal.reason); }, { once: true });
    started.resolve();
  })] });
  const pending = submit(root);
  await started.promise;
  clock.now = 31000; assert.throws(() => a.check(), denied);
  await cancelled.promise; await pending;
  await submit(root);
  assert.equal(calls, 1);
});

test('explicit compaction uses the same provider guard', async t => {
  const { clock, authorization: a } = fixture(t);
  let calls = 0;
  const { harness, root } = await open(t, a, { responses: Array.from({ length: 6 }, () => () => { calls++; return answer('x'.repeat(1000)); }) });
  for (const content of ['first', 'second', 'third']) await (await root.submit({ type: 'input', content: content.repeat(1000) }, context)).wait(context);
  const before = calls;
  await harness.waitForTask(await root.compact(undefined, context), context);
  assert.equal(calls, before + 1);
  await submit(root);
  const beforeDenied = calls;
  clock.now = 31000;
  await harness.waitForTask(await root.compact(undefined, context), context);
  assert.equal(calls, beforeDenied);
});

test('expiry in preflight denies execute, including a nested tool call', async t => {
  for (const nested of [false, true]) {
    const { clock, authorization: a } = fixture(t);
    let effects = 0, outer = 0;
    const tools = [{ name: 'effect', execute: async () => { effects++; return {}; } }];
    if (nested) tools.push({ name: 'outer', execute: async (_args, api, ctx) => { outer++; await api.executeTool('effect', {}, ctx); return {}; } });
    const hooks = [hook(ToolTask, { beforeTool: call => { if (call.name === 'effect') clock.now = 31000; } })];
    const { root } = await open(t, a, { tools, hooks, responses: [toolCall(nested ? 'outer' : 'effect')] });
    await submit(root);
    assert.equal(effects, 0);
    assert.equal(outer, nested ? 1 : 0);
  }
});

test('expiry cancels a cooperative real long-running child, not a universal exit guarantee', async t => {
  const { clock, authorization: a } = fixture(t);
  const started = Promise.withResolvers(), exited = Promise.withResolvers();
  let child, cancellations = 0;
  t.after(() => { child?.kill('SIGKILL'); });
  const { root } = await open(t, a, { tools: [{ name: 'effect', execute: async (_args, _api, ctx) => {
    child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { env: {}, stdio: 'ignore' });
    const closed = once(child, 'close');
    ctx.abortSignal.addEventListener('abort', () => { cancellations++; child.kill('SIGTERM'); }, { once: true });
    await once(child, 'spawn'); started.resolve();
    exited.resolve(await closed);
    return {};
  } }], responses: [toolCall('effect')] });
  const pending = submit(root); await started.promise;
  clock.now = 31000; assert.throws(() => a.check(), denied);
  const [, signal] = await exited.promise;
  assert.equal(signal, 'SIGTERM'); assert.equal(cancellations, 1);
  await pending;
});
