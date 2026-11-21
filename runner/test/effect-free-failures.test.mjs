// Real pinned coding tools under the real Harness: a tool that throws before it
// started any possibly-effectful env operation returns an ordinary error result
// (the model continues and recovery is not blocked); a tool that started one,
// or a cancelled/lease-lost call, stays a fatal interrupted effect.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { componentHash } from '../src/contract.ts';
import { EffectMeter, guardEffectTool, inspectInterruptedEffects, meteredEnv } from '../src/interrupted-effects.ts';
import { Authorization, guardProvider } from '../src/authorization.ts';
import { Harness, createRegistry, defineExtension, defineTool } from '@earendil-works/pi-durable';
import { NodeExecutionEnv } from '@earendil-works/pi-durable/env/node';
import { createEditTool, createReadTool, createWriteTool } from '@earendil-works/pi-durable/tools';
import { createModels, fauxProvider, fauxAssistantMessage, fauxToolCall } from '@earendil-works/pi-ai';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';

const fixtureGrant = JSON.parse(await readFile(new URL('./authorization-lease.json', import.meta.url), 'utf8'));

async function fixture(t) {
  const home = await mkdtemp(join(tmpdir(), 'forge-effect-free-'));
  t.after(() => rm(home, { recursive: true, force: true }));
  const directory = join(home, 'runtime'), worktree = join(home, 'worktree');
  await mkdir(directory, { mode: 0o700 });
  await mkdir(worktree, { mode: 0o700 });
  const launch = { ...fixtureGrant.launch, component_hash: componentHash() };
  const authorization = new Authorization(launch, fixtureGrant.generation, fixtureGrant.owner, fixtureGrant.daemon_epoch, () => 1000);
  t.after(() => authorization.close());
  authorization.accept({ ...fixtureGrant, launch, sequence: 1, issued_boottime_millis: 1000, expires_boottime_millis: 31000 });
  const meter = new EffectMeter();
  const env = meteredEnv(new NodeExecutionEnv({ cwd: worktree, shellPath: '/bin/sh', shellEnv: { PATH: '/usr/bin:/bin' } }), meter);
  return { directory, worktree, launch, authorization, meter, env };
}

/** Run one conversation whose faux model issues `calls` in turn, then answers. */
async function converse(f, tools, calls) {
  const model = fauxProvider();
  const results = [];
  model.setResponses([
    ...calls.map((call, index) => transcript => {
      if (index > 0) results.push(transcript.messages.at(-1));
      return fauxAssistantMessage(fauxToolCall(call.name, call.args), { stopReason: 'toolUse' });
    }),
    transcript => { results.push(transcript.messages.at(-1)); return fauxAssistantMessage('done'); },
  ]);
  const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
  models.setProvider(guardProvider(model.provider, f.authorization));
  const registry = createRegistry();
  registry.install(defineExtension({ name: 'forge-coding', tools: tools.map(tool => ({ ...tool, replay: 'unsafe', execute: guardEffectTool(tool.execute, f.authorization, f.meter) })) }));
  persistIntent(f.directory, f.launch);
  const harness = await Harness.open(await openNodeSqliteStorage(databasePath(f.directory)), { models, registry, settings: { retry: { enabled: false }, compaction: { enabled: false } }, env: () => f.env }, context);
  try {
    const root = await harness.root(context, { agent: { model: { provider: 'faux', modelId: 'faux-1' }, cwd: f.worktree } });
    persistRoot(f.directory, root.id);
    const submission = await root.submit({ type: 'input', requestId: 'first', content: 'Use the tools.' }, context);
    await submission.wait(context);
  } finally { await harness.close(context); }
  return { results, calls: model.state.callCount };
}

test('effect-free read/edit failures are ordinary results; the model continues', { timeout: 20000 }, async t => {
  const f = await fixture(t);
  const file = join(f.worktree, 'hello.txt');
  await writeFile(file, 'Hello\n');
  const { results, calls } = await converse(f, [createReadTool(), createEditTool()], [
    { name: 'read', args: { path: join(f.worktree, '.git', 'HEAD') } }, // `.git` absent: ENOENT/ENOTDIR
    { name: 'read', args: { path: file, offset: 99 } },                  // beyond end of file
    { name: 'edit', args: { path: file, edits: [{ oldText: 'absent text', newText: 'x' }] } },
    { name: 'edit', args: { path: join(f.worktree, 'missing.txt'), edits: [{ oldText: 'a', newText: 'b' }] } },
  ]);
  assert.equal(calls, 5, 'the model is asked again after every effect-free failure');
  assert.equal(results.length, 4);
  for (const result of results) {
    assert.equal(result.role, 'toolResult');
    assert.equal(result.isError, true);
    assert.match(result.content[0].text, /^Tool failed without any effect: /);
  }
  assert.equal(f.authorization.signal.aborted, false);
  assert.equal(f.meter.started, 0);
  assert.equal(await readFile(file, 'utf8'), 'Hello\n');
  // Committed as completed calls: no interrupted effect blocks recovery.
  assert.deepEqual(await inspectInterruptedEffects(f.directory, f.launch), []);
});

test('a failure after a possibly-effectful operation started stays fatal', { timeout: 20000 }, async t => {
  const f = await fixture(t);
  await mkdir(join(f.worktree, 'a-directory'));
  // `writeFile` onto a directory starts the write, then fails.
  const { calls } = await converse(f, [createWriteTool()], [
    { name: 'write', args: { path: join(f.worktree, 'a-directory'), content: 'x' } },
    { name: 'write', args: { path: join(f.worktree, 'never.txt'), content: 'x' } },
  ]);
  assert.equal(calls, 1, 'no model retry after an uncertain effect');
  assert.equal(f.authorization.signal.aborted, true);
  assert.equal((await inspectInterruptedEffects(f.directory, f.launch)).length, 1);
});

for (const failure of ['abort-error', 'lease-lost']) test(`with a meter, ${failure} stays fatal`, { timeout: 20000 }, async t => {
  const f = await fixture(t);
  const tool = defineTool({ name: 'effect', description: 'Controlled failure', parameters: { type: 'object', properties: {}, additionalProperties: false }, execute: async () => {
    if (failure === 'lease-lost') { f.authorization.close(); throw Error('after lease loss'); }
    throw new DOMException('cancelled', 'AbortError');
  } });
  const { calls } = await converse(f, [tool], [{ name: 'effect', args: {} }]);
  assert.equal(calls, 1);
  assert.equal(f.authorization.signal.aborted, true);
  assert.equal((await inspectInterruptedEffects(f.directory, f.launch)).length, 1);
});

test('meteredEnv counts every non-read-only operation and keeps the real env bound', async t => {
  const home = await mkdtemp(join(tmpdir(), 'forge-meter-'));
  t.after(() => rm(home, { recursive: true, force: true }));
  const meter = new EffectMeter();
  const env = meteredEnv(new NodeExecutionEnv({ cwd: home, shellPath: '/bin/sh', shellEnv: { PATH: '/usr/bin:/bin' } }), meter);
  const file = join(home, 'f.txt');
  assert.equal((await env.exists(file, context)).value, false);
  await env.fileInfo(home, context);
  await env.listDir(home, context);
  await env.readTextFile(file, context);
  assert.equal(meter.started, 0, 'reads are not effects');
  await env.writeFile(file, 'x', context);
  assert.equal(meter.started, 1);
  assert.equal((await env.readTextFile(file, context)).value, 'x');
  await env.createDir(join(home, 'd'), { recursive: true }, context);
  await env.exec(['/bin/true'], undefined, context);
  assert.equal(meter.started, 3);
  // A method the read-only list does not name counts, even a hypothetical one.
  const fake = meteredEnv({ futureMutation() { return 1; }, readTextFile() { return 2; } }, meter);
  fake.futureMutation(); fake.readTextFile();
  assert.equal(meter.started, 4);
});
