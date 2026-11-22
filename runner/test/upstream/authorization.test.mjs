import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { Clock, Lease, guardProvider, hostClockUpperBound } from './authorization.mjs';
import { open, context, fauxAssistantMessage, fauxText, fauxToolCall } from './fixture.mjs';

async function fixture(t) {
  const dir = await mkdtemp(join(tmpdir(), 'forge-t01-lease-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const clock = new Clock();
  const lease = new Lease(clock);
  t.after(() => lease.dispose());
  return { path: join(dir, 'runtime.sqlite'), clock, lease };
}
const answer = text => fauxAssistantMessage([fauxText(text)]);

test('Linux admission clock rounds boottime up and fails closed on invalid data', () => {
  assert.equal(hostClockUpperBound(() => '123.45 999.00\n'), 123460);
  assert.equal(hostClockUpperBound(() => '123.99 999.00\n'), 124000);
  for (const input of ['', 'NaN 0', '1.001 1.00', '-1.00 1.00']) {
    assert.throws(() => hostClockUpperBound(() => input), /boottime/);
  }
  assert.throws(() => hostClockUpperBound(() => { throw new Error('unavailable'); }), /unavailable/);
  assert.ok(hostClockUpperBound() > 0);
});

test('provider boundary denies all request paths after expiry, permits cancellation', () => {
  const clock = new Clock();
  const lease = new Lease(clock);
  let calls = 0;
  const methods = ['stream', 'streamSimple', 'fetchDeferred', 'generateImages', 'classify'];
  const provider = Object.fromEntries([...methods, 'cancelDeferred'].map(name => [name, () => calls++]));
  const guarded = guardProvider(provider, lease);
  clock.advance(30000);
  for (const method of methods) assert.throws(() => guarded[method]({}, {}), /Lease expired/);
  assert.equal(calls, 0);
  guarded.cancelDeferred({}, {});
  assert.equal(calls, 1);
  lease.dispose();
});

test('expired lease blocks generation and same-store resumed work before provider request', async t => {
  const { path, clock, lease } = await fixture(t);
  clock.advance(30000);
  let requests = 0;
  const options = { lease, responses: [() => { requests++; return answer('forbidden'); }], settings: { retry: { enabled: false } } };
  let opened = await open(path, options);
  const input = await opened.root.submit({ type: 'input', content: 'go', requestId: 'stable' }, context);
  const result = await input.wait(context);
  assert.equal(result.status, 'unanswered');
  await opened.harness.close(context);
  opened = await open(path, options);
  t.after(() => opened.harness.close(context));
  await (await opened.root.submit({ type: 'input', content: 'again' }, context)).wait(context);
  assert.equal(requests, 0);
});

test('fake-clock expiry aborts active generation without a subsequent provider call', async t => {
  const { path, clock, lease } = await fixture(t);
  const started = Promise.withResolvers();
  const cancelled = Promise.withResolvers();
  let requests = 0;
  const { harness, root } = await open(path, { lease, settings: { retry: { enabled: false } }, responses: [(_transcript, options) => new Promise((_resolve, reject) => {
    requests++;
    options.signal.addEventListener('abort', () => { cancelled.resolve(); reject(options.signal.reason); }, { once: true });
    started.resolve();
  })] });
  t.after(() => harness.close(context));
  const input = await root.submit({ type: 'input', content: 'go' }, context);
  await started.promise;
  clock.advance(30000);
  await cancelled.promise;
  await input.wait(context);
  assert.equal(requests, 1);
});

test('compaction crosses the guarded provider boundary, including denial after expiry', async t => {
  const { path, clock, lease } = await fixture(t);
  let requests = 0;
  const respond = () => { requests++; return answer('x'.repeat(1000)); };
  const { harness, root } = await open(path, { lease,
    settings: { retry: { enabled: false }, compaction: { enabled: false, reserveTokens: 1000, keepRecentTokens: 1, backgroundTokens: 0 } },
    responses: Array.from({ length: 6 }, () => respond),
  });
  t.after(() => harness.close(context));
  for (const content of ['first', 'second', 'third']) {
    await (await root.submit({ type: 'input', content: content.repeat(1000) }, context)).wait(context);
  }
  const before = requests;
  const compacted = await root.compact(undefined, context);
  await harness.waitForTask(compacted, context);
  assert.equal(requests, before + 1);
  // Add another turn so another compaction has material to summarize.
  await (await root.submit({ type: 'input', content: 'fourth'.repeat(1000) }, context)).wait(context);
  const beforeDenied = requests;
  clock.advance(30000);
  const denied = await root.compact(undefined, context);
  await harness.waitForTask(denied, context);
  assert.equal(requests, beforeDenied);
});

test('lease expiry during tool preflight blocks actual execute even though hook errors are skipped', async t => {
  const { path, clock, lease } = await fixture(t);
  let tools = 0;
  const { hook, ToolTask } = await import('@earendil-works/pi-durable');
  const { harness, root } = await open(path, { lease,
    hooks: [hook(ToolTask, { beforeTool: () => { clock.advance(30000); } })],
    tool: async () => { tools++; return {}; },
    responses: [fauxAssistantMessage([fauxToolCall('effect', {}, { id: 'call' })], { stopReason: 'toolUse' })],
    settings: { retry: { enabled: false } },
  });
  t.after(() => harness.close(context));
  await (await root.submit({ type: 'input', content: 'go' }, context)).wait(context);
  assert.equal(tools, 0);
});

test('expiry cancels a cooperative tool child; signal alone is not child-exit evidence', async t => {
  const { path, clock, lease } = await fixture(t);
  const started = Promise.withResolvers();
  const exited = Promise.withResolvers();
  let child;
  const { harness, root } = await open(path, { lease,
    tool: async (_args, _api, callContext) => {
      child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
      const closed = once(child, 'close');
      callContext.abortSignal.addEventListener('abort', () => { child.kill('SIGTERM'); }, { once: true });
      await once(child, 'spawn');
      started.resolve();
      const result = await closed;
      exited.resolve(result);
      return {};
    },
    responses: [fauxAssistantMessage([fauxToolCall('effect', {}, { id: 'call' })], { stopReason: 'toolUse' })],
    settings: { retry: { enabled: false } },
  });
  t.after(async () => { child?.kill('SIGKILL'); await harness.close(context); });
  const input = await root.submit({ type: 'input', content: 'go' }, context);
  await started.promise;
  clock.advance(30000);
  const [, signal] = await exited.promise;
  assert.equal(signal, 'SIGTERM');
  await input.wait(context);
});
