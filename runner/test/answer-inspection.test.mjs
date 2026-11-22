// Subprocess wire fixtures, not completion or execution authority.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { componentHash } from '../src/contract.ts';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';
const launch = { ...JSON.parse(await readFile(new URL('./wire-v1.json', import.meta.url))).operation.launch, component_hash: componentHash() };
async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'forge-answer-wire-'));
  t.after(() => rm(directory, { recursive: true, force: true })); return directory;
}
async function inspect(t, directory, request) {
  const child = spawn(process.execPath, [new URL('../src/inspect-answer.ts', import.meta.url).pathname, directory], { env: {} });
  let output = '', errors = ''; const exit = once(child, 'close');
  t.after(async () => { if (child.exitCode === null && child.signalCode === null) { child.kill('SIGKILL'); await exit; } });
  child.stdout.on('data', d => { output += d; }); child.stderr.on('data', d => { errors += d; });
  child.stdin.on('error', e => { if (e.code !== 'EPIPE') throw e; });
  child.stdin.end(typeof request === 'string' ? request : JSON.stringify(request));
  await exit; return { code: child.exitCode, output, errors };
}
async function seed(directory) {
  persistIntent(directory, launch); const storage = await openNodeSqliteStorage(databasePath(directory));
  const root = await storage.mintId(), id = await storage.mintId();
  await storage.commit([[{ type: 'conversation', value: { id: root } }, { type: 'submission', value: {
    id, conversationId: root, requestId: launch.operation_id, type: 'input', status: 'done', entry: 99, answer: 100,
  } }]], () => {}, context);
  await storage.close(context); persistRoot(directory, root); return String(id);
}
async function bytes(directory) {
  const files = await Promise.all((await readdir(directory)).filter(n => !n.endsWith('-shm')).map(async n => [n, await readFile(join(directory, n))]));
  return Object.fromEntries(files.filter(([n, data]) => !n.endsWith('-wal') || data.length));
}

test('answer wire observes committed identity only and never initializes absent runtime', async t => {
  const directory = await fixture(t);
  const request = { protocol: 1, launch, admission_id: '2' };
  const absent = await inspect(t, directory, request);
  assert.equal(absent.code, 0); assert.equal(JSON.parse(absent.output).answer, 'unknown');
  assert.deepEqual(await readdir(directory), []);
  const id = await seed(directory), before = await bytes(directory);
  const result = await inspect(t, directory, { ...request, admission_id: id });
  assert.equal(result.code, 0); assert.equal(result.errors, '');
  assert.deepEqual(JSON.parse(result.output), { protocol: 1, launch, answer: 'answered' });
  assert.deepEqual(await bytes(directory), before);
});

test('answer wire rejects incompatible, oversized and foreign data without disclosure or writes', async t => {
  const directory = await fixture(t), id = await seed(directory), before = await bytes(directory);
  const request = { protocol: 1, launch, admission_id: id };
  for (const input of ['PRIVATE-CANARY', 'x'.repeat(16385), { ...request, protocol: 2 }, { ...request, extra: 'PRIVATE-CANARY' },
    { ...request, launch: { ...launch, attempt_id: 'PRIVATE-CANARY' } }, { ...request, admission_id: 'PRIVATE-CANARY' }]) {
    assert.deepEqual(await inspect(t, directory, input), { code: 1, output: '', errors: 'runner-answer-inspection-failed\n' });
    assert.deepEqual(await bytes(directory), before);
  }
});
