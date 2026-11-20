import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawn } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { componentHash } from '../src/contract.ts';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';

const fixture = JSON.parse(await readFile(new URL('./authorization-lease.json', import.meta.url), 'utf8'));
async function inspect(directory, input) {
  const child = spawn(process.execPath, [new URL('../src/inspect-effects.ts', import.meta.url).pathname, directory], { env: {}, stdio: ['pipe', 'pipe', 'pipe'] });
  let stdout = '', stderr = '';
  child.stdout.on('data', bytes => { stdout += bytes; });
  child.stderr.on('data', bytes => { stderr += bytes; });
  const done = new Promise((resolve, reject) => { child.once('error', reject); child.once('close', code => resolve({ code, stdout, stderr })); });
  child.stdin.end(typeof input === 'string' ? input : JSON.stringify(input));
  return done;
}

test('read-only effect wire fails closed, bounds input and redacts errors', async t => {
  const directory = await mkdtemp(join(tmpdir(), 'forge-effect-wire-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const launch = { ...fixture.launch, component_hash: componentHash() };
  const request = { protocol: 1, launch };
  const rejected = async input => {
    const result = await inspect(directory, input);
    assert.equal(result.code, 1);
    assert.equal(result.stdout, '');
    assert.match(result.stderr, /runner-effect-inspection-failed/);
    assert.ok(!result.stderr.includes('SECRET-CANARY'));
  };
  // A missing runtime is NOT a completed empty scan.
  await rejected(request);
  await rejected({ ...request, protocol: 2 });
  await rejected({ ...request, extra: 'SECRET-CANARY' });
  await rejected({ ...request, launch: { ...launch, component_hash: 'other' } });
  await rejected('SECRET-CANARY'.repeat(2000));
  await rejected('not-json SECRET-CANARY');
  persistIntent(directory, launch);
  await rejected(request); // interrupted initialization
  const storage = await openNodeSqliteStorage(databasePath(directory));
  persistRoot(directory, 1);
  const task = { id: await storage.mintId(), conversationId: 1, kind: 'pi.tool', version: 3, background: false, abortRequested: false,
    input: { kind: 'model', callId: 'effect-call', assistant: 1 },
    state: { status: 'running', checkpoint: { phase: 'execute', replay: 'unsafe', arguments: { command: 'SECRET-CANARY' } } } };
  await storage.commit([[{ type: 'task', value: task }]], () => {}, context);
  await storage.close(context);
  const before = await readFile(databasePath(directory));
  await rejected({ ...request, launch: { ...launch, incarnation_id: 'foreign' } });
  for (let retry = 0; retry < 2; retry++) {
    const result = await inspect(directory, request);
    assert.equal(result.code, 0, result.stderr);
    assert.ok(!result.stdout.includes('SECRET-CANARY'));
    assert.deepEqual(JSON.parse(result.stdout), { protocol: 1, launch, effects: [{ task_id: String(task.id), conversation_id: '1', call_id: 'effect-call' }] });
    assert.deepEqual(await readFile(databasePath(directory)), before);
  }
  // Corruption is not repaired or interpreted as absence.
  await writeFile(databasePath(directory), 'SECRET-CANARY corrupt runtime');
  await rejected(request);
  assert.equal(await readFile(databasePath(directory), 'utf8'), 'SECRET-CANARY corrupt runtime');
});
