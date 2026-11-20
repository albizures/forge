import assert from 'node:assert/strict';
import { test } from 'node:test';
import { once } from 'node:events';
import { spawn } from 'node:child_process';
import { mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';
import { inspectRecoveryCandidate } from '../src/recovery.ts';

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
async function committed(t, type, status) {
  // Storage-contract fixtures: no model or scheduler is ever constructed.
  const directory = await fixture(t);
  persistIntent(directory, launch);
  const storage = await openNodeSqliteStorage(databasePath(directory));
  const root = await storage.mintId(), id = await storage.mintId();
  const value = { id, conversationId: root, requestId: launch.operation_id, type, status };
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
  for (let restart = 0; restart < 3; restart++) {
    assert.deepEqual(await inspectRecoveryCandidate(directory, launch, identity.admission_id), { ...identity, status: 'placed' });
    assert.deepEqual(await bytes(directory), before);
  }
  assert.equal(output.split('model-entered').length - 1, 1);
});
