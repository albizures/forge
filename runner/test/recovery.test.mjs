import assert from 'node:assert/strict';
import { test } from 'node:test';
import { once } from 'node:events';
import { spawn } from 'node:child_process';
import { mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';
import { inspectRecoveryCandidate } from '../src/recovery.ts';
import { componentHash } from '../src/contract.ts';

const launch = JSON.parse(await readFile(new URL('./wire-v1.json', import.meta.url), 'utf8')).operation.launch;
async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'forge-recovery-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  return directory;
}
async function bytes(directory) {
  const files = await Promise.all((await readdir(directory)).filter(name => !name.endsWith('-shm')).map(async name => [name, await readFile(join(directory, name))]));
  return Object.fromEntries(files.filter(([name, data]) => !name.endsWith('-wal') || data.length > 0));
}
async function committed(t, type, status, boundLaunch = launch) {
  // Storage-contract fixtures: no model or scheduler is ever constructed.
  const directory = await fixture(t);
  persistIntent(directory, boundLaunch);
  const storage = await openNodeSqliteStorage(databasePath(directory));
  const root = await storage.mintId(), id = await storage.mintId();
  const value = { id, conversationId: root, requestId: boundLaunch.operation_id, type, status };
  if (status === 'done') value.answer = 100;
  if (status === 'unanswered') value.reason = 'controlled-failure';
  if (status === 'placed' || status === 'done') value.entry = 99;
  await storage.commit([[
    { type: 'conversation', value: { id: root } },
    ...(type === undefined ? [] : [{ type: 'submission', value }]),
  ]], () => {}, context);
  await storage.close(context);
  persistRoot(directory, root);
  return { directory, root, id: String(id) };
}

test('absent and interrupted initialization remain unchanged, never initialized by inspection', async t => {
  const directory = await fixture(t);
  await assert.rejects(inspectRecoveryCandidate(directory, launch, '1'), { code: 'runner-recovery-required' });
  assert.deepEqual(await readdir(directory), []);
  persistIntent(directory, launch);
  const before = await bytes(directory);
  await assert.rejects(inspectRecoveryCandidate(directory, launch, '1'), { code: 'runner-recovery-required' });
  assert.deepEqual(await bytes(directory), before);
});

test('a valid root without the original admission is not permission to submit another input', async t => {
  const f = await committed(t, undefined, 'queued');
  const before = await bytes(f.directory);
  await assert.rejects(inspectRecoveryCandidate(f.directory, launch, f.id), { code: 'runner-recovery-required' });
  assert.deepEqual(await bytes(f.directory), before);
});

for (const status of ['queued', 'placed', 'done', 'unanswered']) test(`only pending original input is a candidate: ${status}`, async t => {
  const f = await committed(t, 'input', status);
  const before = await bytes(f.directory);
  if (status === 'queued' || status === 'placed') {
    assert.deepEqual(await inspectRecoveryCandidate(f.directory, launch, f.id), { root: f.root, admission_id: f.id, status });
  } else {
    await assert.rejects(inspectRecoveryCandidate(f.directory, launch, f.id), { code: 'runner-recovery-intervention-required' });
  }
  assert.deepEqual(await bytes(f.directory), before);
});

for (const type of ['write', 'continue']) test(`recovery cannot reinterpret ${type} as the bound input`, async t => {
  const f = await committed(t, type, type === 'write' ? 'queued' : 'placed');
  const before = await bytes(f.directory);
  await assert.rejects(inspectRecoveryCandidate(f.directory, launch, f.id), { code: 'runner-admission-conflict' });
  assert.deepEqual(await bytes(f.directory), before);
});

test('full immutable launch and exact accepted admission are required, including across await', async t => {
  const f = await committed(t, 'input', 'queued');
  const before = await bytes(f.directory);
  for (const field of Object.keys(launch)) {
    const foreign = { ...launch, [field]: typeof launch[field] === 'number' ? launch[field] + 1 : launch[field] + '-foreign' };
    await assert.rejects(inspectRecoveryCandidate(f.directory, foreign, f.id), { code: 'runner-identity-conflict' });
  }
  await assert.rejects(inspectRecoveryCandidate(f.directory, launch, ''), { code: 'runner-admission-invalid' });
  await assert.rejects(inspectRecoveryCandidate(f.directory, launch, 'foreign'), { code: 'runner-admission-conflict' });
  const changing = structuredClone(launch);
  const pending = inspectRecoveryCandidate(f.directory, changing, f.id);
  changing.operation_id = 'caller-mutation';
  assert.deepEqual(await pending, { root: f.root, admission_id: f.id, status: 'queued' });
  assert.deepEqual(await bytes(f.directory), before);
});

test('actual model-process death leaves one pending admission; repeated inspection neither repairs nor executes it', { timeout: 15000 }, async t => {
  const directory = await fixture(t);
  const child = spawn(process.execPath, [new URL('./recovery-worker.mjs', import.meta.url).pathname, directory], { stdio: ['ignore', 'pipe', 'pipe'] });
  const exited = once(child, 'exit');
  t.after(async () => { if (child.exitCode === null && child.signalCode === null) { child.kill('SIGKILL'); await exited; } });
  let output = '', errors = '';
  child.stderr.on('data', data => { errors += data; });
  await Promise.race([
    new Promise(resolve => child.stdout.on('data', data => { output += data; if (output.includes('model-entered\n')) resolve(); })),
    exited.then(() => { throw Error(`worker exited before model request: ${errors}`); }),
  ]);
  const identity = JSON.parse(output.split('\n')[0]);
  child.kill('SIGKILL');
  assert.deepEqual(await exited, [null, 'SIGKILL']);
  const before = await bytes(directory);
  const bound = { ...launch, component_hash: componentHash() };
  for (let restart = 0; restart < 3; restart++) {
    assert.deepEqual(await inspectRecoveryCandidate(directory, bound, identity.admission_id), { ...identity, status: 'placed' });
    const response = await inspectWire(t, directory, { protocol: 1, launch: bound, admission_id: identity.admission_id });
    assert.equal(response.code, 0, response.errors);
    assert.deepEqual(JSON.parse(response.output), { protocol: 1, launch: bound, candidate: { ...identity, status: 'placed' } });
    assert.deepEqual(await bytes(directory), before);
  }
  assert.equal(output.split('model-entered').length - 1, 1);
});

test('recovery wire blocks committed effect intent, even outside the original conversation', { timeout: 15000 }, async t => {
  const bound = { ...launch, component_hash: componentHash() };
  const f = await committed(t, 'input', 'queued', bound);
  const storage = await openNodeSqliteStorage(databasePath(f.directory));
  try {
    const child = await storage.mintId();
    const task = await storage.mintId();
    await storage.commit([[
      { type: 'conversation', value: { id: child, parent: { conversationId: f.root, at: 100 } } },
      { type: 'task', value: { id: task, conversationId: child, kind: 'pi.tool', version: 3, background: false, abortRequested: false,
        input: { kind: 'model', callId: 'private-call', assistant: 1 },
        state: { status: 'running', checkpoint: { phase: 'execute', replay: 'unsafe', arguments: { command: 'private-command-canary' } } } } },
    ]], () => {}, context);
  } finally { await storage.close(context); }
  const before = await bytes(f.directory);
  for (let retry = 0; retry < 2; retry++) {
    assert.deepEqual(await inspectWire(t, f.directory, { protocol: 1, launch: bound, admission_id: f.id }), { code: 1, output: '', errors: 'runner-recovery-inspection-failed\n' });
    assert.deepEqual(await bytes(f.directory), before);
  }
});

test('recovery wire does not repair corrupted runtime storage', { timeout: 15000 }, async t => {
  const bound = { ...launch, component_hash: componentHash() };
  const f = await committed(t, 'input', 'queued', bound);
  await writeFile(databasePath(f.directory), 'private-corrupt-runtime-canary');
  const before = await bytes(f.directory);
  assert.deepEqual(await inspectWire(t, f.directory, { protocol: 1, launch: bound, admission_id: f.id }), { code: 1, output: '', errors: 'runner-recovery-inspection-failed\n' });
  assert.deepEqual(await bytes(f.directory), before);
});

async function inspectWire(t, directory, request, extra = []) {
  const child = spawn(process.execPath, [new URL('../src/inspect-recovery.ts', import.meta.url).pathname, directory, ...extra], { env: {}, stdio: ['pipe', 'pipe', 'pipe'] });
  const exited = once(child, 'exit');
  t.after(async () => { if (child.exitCode === null && child.signalCode === null) { child.kill('SIGKILL'); await exited; } });
  let output = '', errors = '';
  child.stdout.on('data', data => { output += data; });
  child.stderr.on('data', data => { errors += data; });
  // A rejected oversized request may close stdin before all bytes are written.
  child.stdin.on('error', error => { if (error.code !== 'EPIPE') throw error; });
  child.stdin.end(typeof request === 'string' ? request : JSON.stringify(request));
  await once(child, 'close');
  return { code: child.exitCode, output, errors };
}

for (const status of ['queued', 'placed']) test(`read-only recovery wire reports only the exact pending input: ${status}`, { timeout: 15000 }, async t => {
  const bound = { ...launch, component_hash: componentHash() };
  const f = await committed(t, 'input', status, bound);
  const before = await bytes(f.directory);
  const request = { protocol: 1, launch: bound, admission_id: f.id };
  for (let retry = 0; retry < 2; retry++) {
    const response = await inspectWire(t, f.directory, request);
    assert.equal(response.code, 0, response.errors);
    assert.equal(response.errors, '');
    assert.deepEqual(JSON.parse(response.output), { protocol: 1, launch: bound, candidate: { root: f.root, admission_id: f.id, status } });
    assert.deepEqual(await bytes(f.directory), before);
  }
});

test('recovery wire rejects malformed, incompatible and foreign requests without content disclosure or writes', { timeout: 15000 }, async t => {
  const bound = { ...launch, component_hash: componentHash() };
  const f = await committed(t, 'input', 'queued', bound);
  const before = await bytes(f.directory);
  const request = { protocol: 1, launch: bound, admission_id: f.id };
  const secret = 'must-not-disclose-private-input';
  const cases = [
    '{' + secret,
    JSON.stringify(request) + ' trailing-' + secret,
    { ...request, protocol: 2 },
    { ...request, extra: secret },
    { ...request, admission_id: secret },
    { ...request, admission_id: '' },
    { ...request, admission_id: 1 },
    { ...request, launch: { ...bound, component_hash: secret } },
    { ...request, launch: { ...bound, attempt_id: secret } },
    { ...request, launch: { ...bound, extra: secret } },
    { ...request, launch: undefined },
    ' '.repeat(16385) + secret,
  ];
  for (const input of cases) {
    assert.deepEqual(await inspectWire(t, f.directory, input), { code: 1, output: '', errors: 'runner-recovery-inspection-failed\n' });
    assert.deepEqual(await bytes(f.directory), before);
  }
  assert.deepEqual(await inspectWire(t, f.directory, request, [secret]), { code: 1, output: '', errors: 'runner-recovery-inspection-failed\n' });
  assert.deepEqual(await bytes(f.directory), before);
});

test('recovery wire cannot initialize missing storage or reinterpret finished inputs as resumable', { timeout: 15000 }, async t => {
  const bound = { ...launch, component_hash: componentHash() };
  const directory = await fixture(t);
  const request = { protocol: 1, launch: bound, admission_id: '1' };
  assert.deepEqual(await inspectWire(t, directory, request), { code: 1, output: '', errors: 'runner-recovery-inspection-failed\n' });
  assert.deepEqual(await readdir(directory), []);
  for (const status of ['done', 'unanswered']) {
    const f = await committed(t, 'input', status, bound);
    const before = await bytes(f.directory);
    assert.deepEqual(await inspectWire(t, f.directory, { ...request, admission_id: f.id }), { code: 1, output: '', errors: 'runner-recovery-inspection-failed\n' });
    assert.deepEqual(await bytes(f.directory), before);
  }
});
