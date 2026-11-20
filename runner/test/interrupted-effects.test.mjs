import assert from 'node:assert/strict';
import { test } from 'node:test';
import { once } from 'node:events';
import { createServer } from 'node:http';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { componentHash } from '../src/contract.ts';
import { guardEffectTool, inspectInterruptedEffects, requireNoInterruptedEffects } from '../src/interrupted-effects.ts';
import { createCheckRuntime } from '../src/check-runtime.ts';
import { Authorization, guardProvider } from '../src/authorization.ts';
import { Harness, createRegistry, defineExtension, defineTool } from '@earendil-works/pi-durable';
import { createModels, fauxProvider, fauxAssistantMessage, fauxToolCall } from '@earendil-works/pi-ai';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';

const fixtureGrant = JSON.parse(await readFile(new URL('./authorization-lease.json', import.meta.url), 'utf8'));

for (const failure of ['throw', 'cancel']) test(`live ${failure} fences further model/tool requests`, { timeout: 15000 }, async t => {
  const directory = await mkdtemp(join(tmpdir(), 'forge-live-effect-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const launch = { ...fixtureGrant.launch, component_hash: componentHash() };
  const authorization = new Authorization(launch, fixtureGrant.generation, fixtureGrant.owner, fixtureGrant.daemon_epoch, () => 1000);
  t.after(() => authorization.close());
  authorization.accept({ ...fixtureGrant, launch, sequence: 1, issued_boottime_millis: 1000, expires_boottime_millis: 31000 });
  const model = fauxProvider();
  model.setResponses([fauxAssistantMessage(fauxToolCall('effect', {}), { stopReason: 'toolUse' }), () => { throw Error('must not ask model to retry'); }]);
  const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
  models.setProvider(guardProvider(model.provider, authorization));
  let effects = 0;
  const tool = defineTool({ name: 'effect', description: 'Controlled uncertain effect', parameters: { type: 'object', properties: {}, additionalProperties: false }, replay: 'unsafe', execute: guardEffectTool(async () => {
    effects++;
    if (failure === 'cancel') throw new DOMException('cancelled secret canary', 'AbortError');
    throw Error('upstream secret canary');
  }, authorization) });
  const registry = createRegistry();
  registry.install(defineExtension({ name: 'effect-fixture', tools: [tool] }));
  persistIntent(directory, launch);
  const harness = await Harness.open(await openNodeSqliteStorage(databasePath(directory)), { models, registry, settings: { retry: { enabled: false }, compaction: { enabled: false } }, env: () => undefined }, context);
  try {
    const root = await harness.root(context, { agent: { model: { provider: 'faux', modelId: 'faux-1' } } });
    persistRoot(directory, root.id);
    for (const requestId of ['first', 'fresh-input']) {
      const submission = await root.submit({ type: 'input', requestId, content: 'Execute controlled effect.' }, context);
      await submission.wait(context);
    }
    assert.equal(effects, 1);
    assert.equal(model.state.callCount, 1);
    assert.equal(authorization.signal.aborted, true);
  } finally { await harness.close(context); }
  // The pinned Harness records thrown tool exceptions as failed receipts. The
  // same read-only reopen fence must retain them, not just an in-memory abort.
  assert.equal((await inspectInterruptedEffects(directory, launch)).length, 1);
  const bytes = await readFile(databasePath(directory));
  assert.ok(!bytes.includes(Buffer.from('secret canary')));
});

for (const interrupted of [true, false]) test(`external mutation ${interrupted ? 'interrupted before result' : 'with committed result'}`, { timeout: 20000 }, async t => {
  const home = await mkdtemp(join(tmpdir(), 'forge-effect-'));
  t.after(() => rm(home, { recursive: true, force: true }));
  const directory = join(home, 'runtime'), worktree = join(home, 'worktree');
  await mkdir(directory, { mode: 0o700 });
  await mkdir(worktree, { mode: 0o700 });
  const key = join(directory, 'model-key');
  await writeFile(key, 'EFFECT-MODEL-KEY-CANARY', { mode: 0o600 });
  const git = (...args) => execFileSync('/usr/bin/git', ['-C', worktree, ...args]);
  git('init', '-q');
  git('config', 'user.name', 'Test'); git('config', 'user.email', 'test@example.invalid');
  await writeFile(join(worktree, 'tracked'), 'base\n');
  git('add', 'tracked'); git('commit', '-qm', 'base');
  await writeFile(join(worktree, 'tracked'), 'staged\n'); git('add', 'tracked');
  await writeFile(join(worktree, 'tracked'), 'unstaged\n');
  await writeFile(join(worktree, 'untracked'), 'untracked\n');
  const snapshot = async () => ({ status: git('status', '--porcelain=v1').toString(), index: git('show', ':tracked').toString(), tracked: await readFile(join(worktree, 'tracked'), 'utf8'), untracked: await readFile(join(worktree, 'untracked'), 'utf8') });
  const before = await snapshot();
  let mutations = 0, observed;
  const effect = new Promise(resolve => { observed = resolve; });
  const server = createServer((request, response) => {
    assert.equal(request.method, 'POST');
    mutations++;
    // The endpoint records the effect before allowing any result to return.
    observed();
    if (!interrupted) response.end('mutation accepted');
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => { server.closeAllConnections(); server.close(); });
  const launch = { ...fixtureGrant.launch, component_hash: componentHash() };
  const child = spawn(process.execPath, [new URL('./interrupted-effect-worker.mjs', import.meta.url).pathname], { detached: true, stdio: ['pipe', 'pipe', 'pipe'] });
  let output = '';
  child.stdout.on('data', data => { output += data; });
  child.stderr.on('data', data => { output += data; });
  t.after(() => { try { process.kill(-child.pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; } });
  const exited = once(child, 'exit');
  child.stdin.end(JSON.stringify({ directory, worktree, key, launch, endpoint: `http://127.0.0.1:${server.address().port}/publication` }));
  await Promise.race([effect, exited.then(() => { throw Error(`worker exited before mutation: ${output}`); })]);
  if (interrupted) process.kill(-child.pid, 'SIGKILL');
  const [code, signal] = await exited;
  if (interrupted) assert.equal(signal, 'SIGKILL');
  else assert.equal(code, 0, output);
  assert.equal(mutations, 1);
  assert.ok(!output.includes('EFFECT-MODEL-KEY-CANARY'));
  const bytes = await readFile(join(directory, 'runtime.sqlite'));
  for (let restart = 0; restart < 2; restart++) {
    const effects = await inspectInterruptedEffects(directory, launch);
    assert.equal(effects.length, interrupted ? 1 : 0);
    if (interrupted) {
      assert.deepEqual(Object.keys(effects[0]).sort(), ['call_id', 'conversation_id', 'task_id']);
      await assert.rejects(requireNoInterruptedEffects(directory, launch), { code: 'runner-interrupted-effect' });
      // The real Check entry boundary must fence before bootstrap, not merely
      // rely on the Harness refusing to replay this particular tool task.
      const grant = fixtureGrant;
      const authorization = new Authorization(launch, grant.generation, grant.owner, grant.daemon_epoch, () => 1000);
      authorization.accept({ ...grant, launch, sequence: 1, issued_boottime_millis: 1000, expires_boottime_millis: 31000 });
      t.after(() => authorization.close());
      // Deliberately invalid bootstrap metadata: the effect fence must be the
      // first awaited gate and terminally close the authorization consumer.
      await assert.rejects(createCheckRuntime(launch, grant.generation, { executionDirectory: worktree }, {}, { cwd: worktree }, key, authorization, directory), { code: 'assignment-runtime-failed' });
      assert.throws(() => authorization.admit());
    } else await requireNoInterruptedEffects(directory, launch);
  }
  await assert.rejects(inspectInterruptedEffects(directory, { ...launch, assignment_id: 'foreign' }), { code: 'runner-identity-conflict' });
  assert.deepEqual(await readFile(join(directory, 'runtime.sqlite')), bytes);
  assert.deepEqual(await snapshot(), before);
  assert.equal(mutations, 1);
});

test('committed task fixtures: pagination, nested calls, failed receipts and unknown contracts fail closed', async t => {
  // Storage-contract fixtures, not additional external-effect/process evidence.
  const directory = await mkdtemp(join(tmpdir(), 'forge-effect-fixtures-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const launch = { ...fixtureGrant.launch, component_hash: componentHash() };
  assert.deepEqual(await inspectInterruptedEffects(directory, launch), []);
  persistIntent(directory, launch);
  await assert.rejects(inspectInterruptedEffects(directory, launch), { code: 'runner-recovery-required' });
  const storage = await openNodeSqliteStorage(databasePath(directory));
  t.after(() => storage.close(context));
  persistRoot(directory, 1);
  const tasks = [];
  for (let index = 0; index < 101; index++) tasks.push({
    id: await storage.mintId(), conversationId: 1, kind: 'pi.tool', version: 3,
    background: false, abortRequested: false,
    input: { kind: 'model', callId: `call-${index}`, assistant: 1 },
    state: { status: 'pending', checkpoint: { phase: 'call' } },
  });
  const save = async values => storage.commit([values.map(value => ({ type: 'task', value }))], () => {}, context);
  await save(tasks);
  assert.deepEqual(await inspectInterruptedEffects(directory, launch), []);
  const last = tasks.at(-1);
  await save([{ ...last, conversationId: 999, input: { kind: 'nested', call: { id: 'nested-effect', name: 'bash', arguments: {} } }, state: { status: 'running', checkpoint: { phase: 'execute', replay: 'safe', arguments: {} } } }]);
  assert.deepEqual(await inspectInterruptedEffects(directory, launch), [{ task_id: String(last.id), conversation_id: '999', call_id: 'nested-effect' }]);
  // Even a purported replay-safe stored call has no approved automatic exception.
  for (const status of ['failed', 'aborted', 'faulted', 'orphaned']) {
    await save([{ ...last, state: { status: 'terminal', outcome: { status, error: { message: 'unknown external result' }, reason: 'restart' } } }]);
    assert.equal((await inspectInterruptedEffects(directory, launch)).length, 1);
  }
  await save([{ ...last, state: { status: 'terminal', outcome: { status: 'completed', result: {} } } }]);
  assert.deepEqual(await inspectInterruptedEffects(directory, launch), []);
  await save([{ ...last, version: 4 }]);
  await assert.rejects(inspectInterruptedEffects(directory, launch), { code: 'runner-tool-contract-incompatible' });
  await save([{ ...last, state: { status: 'running', checkpoint: { phase: 'unknown' } } }]);
  await assert.rejects(inspectInterruptedEffects(directory, launch), { code: 'runner-tool-contract-incompatible' });
});
