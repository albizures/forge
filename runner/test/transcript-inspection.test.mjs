// Subprocess wire fixtures for read-only transcript pages, not execution authority.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, readFile, readdir, rm, writeFile, chmod } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { componentHash } from '../src/contract.ts';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';
const launch = { ...JSON.parse(await readFile(new URL('./wire-v1.json', import.meta.url))).operation.launch, component_hash: componentHash() };
async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'forge-transcript-wire-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  await chmod(directory, 0o700); return directory;
}
async function inspect(t, directory, request) {
  const child = spawn(process.execPath, [new URL('../src/inspect-transcript.ts', import.meta.url).pathname, directory], { env: {} });
  let output = '', errors = ''; const exit = once(child, 'close');
  t.after(async () => { if (child.exitCode === null && child.signalCode === null) { child.kill('SIGKILL'); await exit; } });
  child.stdout.on('data', d => { output += d; }); child.stderr.on('data', d => { errors += d; });
  child.stdin.on('error', e => { if (e.code !== 'EPIPE') throw e; });
  child.stdin.end(typeof request === 'string' ? request : JSON.stringify(request));
  await exit; return { code: child.exitCode, output, errors };
}
const page = (after, limit = 2) => ({ protocol: 1, launch, after, limit });
async function seed(directory, count) {
  persistIntent(directory, launch); const storage = await openNodeSqliteStorage(databasePath(directory));
  const root = await storage.mintId();
  await storage.commit([[{ type: 'conversation', value: { id: root } }]], () => {}, context);
  persistRoot(directory, root);
  return { storage, root, ids: await append(storage, root, count, 0) };
}
async function append(storage, root, count, offset) {
  const ids = [];
  for (let i = 0; i < count; i++) {
    const id = await storage.mintId(); ids.push(id);
    await storage.commit([[{ type: 'entry', value: { id, conversationId: root, kind: 'test.message', data: { n: offset + i, text: i === 0 && offset === 0 ? 'x'.repeat(9000) : `entry-${offset + i}` } } }]], () => {}, context);
  }
  return ids;
}
async function bytes(directory) {
  const files = await Promise.all((await readdir(directory)).filter(n => !n.endsWith('-shm')).map(async n => [n, await readFile(join(directory, n))]));
  return Object.fromEntries(files.filter(([n, data]) => !n.endsWith('-wal') || data.length));
}

test('transcript pages preserve identity/order across live appends beside an open writer', async t => {
  const directory = await fixture(t);
  const { storage, root, ids } = await seed(directory, 5); // writer stays open
  t.after(() => storage.close(context));
  const first = await inspect(t, directory, page(null));
  assert.equal(first.code, 0); assert.equal(first.errors, '');
  const one = JSON.parse(first.output);
  assert.deepEqual(one.entries.map(e => e.id), ids.slice(0, 2).map(String));
  assert.equal(one.history, 'available'); assert.equal(one.more, true); assert.equal(one.next_after, String(ids[1]));
  assert.equal(one.entries[0].truncated, true); assert.ok(one.entries[0].content.length <= 8192);
  assert.equal(one.entries[1].truncated, false);
  const more = await append(storage, root, 2, 5); // live appends between pages
  const seen = [...one.entries.map(e => e.id)]; let after = one.next_after, guard = 0;
  for (;;) {
    const next = JSON.parse((await inspect(t, directory, page(after))).output);
    seen.push(...next.entries.map(e => e.id)); after = next.next_after;
    if (!next.more) break; assert.ok(++guard < 10);
  }
  assert.deepEqual(seen, [...ids, ...more].map(String));
  const idle = JSON.parse((await inspect(t, directory, page(after))).output);
  assert.deepEqual([idle.entries, idle.more, idle.next_after], [[], false, after]);
});

test('transcript reads change no bytes, create nothing for absent runtime and report unstarted history', async t => {
  const empty = await fixture(t);
  const absent = JSON.parse((await inspect(t, empty, page(null))).output);
  assert.deepEqual([absent.history, absent.entries], ['not-started', []]); assert.deepEqual(await readdir(empty), []);
  const directory = await fixture(t); persistIntent(directory, launch);
  assert.equal(JSON.parse((await inspect(t, directory, page(null))).output).history, 'not-started');
  const seeded = await fixture(t); const { storage } = await seed(seeded, 3); await storage.close(context);
  const before = await bytes(seeded);
  assert.equal(JSON.parse((await inspect(t, seeded, page(null, 100))).output).entries.length, 3);
  assert.deepEqual(await bytes(seeded), before);
});

test('corrupt or foreign storage is unavailable with a fixed code and no secret disclosure', async t => {
  const directory = await fixture(t); const { storage } = await seed(directory, 2); await storage.close(context);
  const foreign = JSON.parse((await inspect(t, directory, { ...page(null), launch: { ...launch, attempt_id: 'PRIVATE-CANARY' } })).output);
  assert.equal(foreign.history, 'unavailable'); assert.doesNotMatch(JSON.stringify(foreign.code), /CANARY/);
  await writeFile(databasePath(directory), 'PRIVATE-CANARY not sqlite', { mode: 0o600 });
  const before = await bytes(directory);
  const corrupt = await inspect(t, directory, page(null));
  assert.equal(corrupt.code, 0); assert.equal(corrupt.errors, '');
  const body = JSON.parse(corrupt.output);
  assert.deepEqual([body.history, body.entries, body.code], ['unavailable', [], 'runner-history-unavailable']);
  assert.doesNotMatch(corrupt.output, /CANARY/); assert.deepEqual(await bytes(directory), before);
});

test('transcript wire rejects malformed, oversized and extra requests', async t => {
  const directory = await fixture(t); const { storage } = await seed(directory, 1); await storage.close(context); const before = await bytes(directory);
  for (const input of ['PRIVATE-CANARY', 'x'.repeat(16385), { ...page(null), protocol: 2 }, { ...page(null), extra: 1 },
    page(null, 0), page(null, 101), page('0'), page('1; DROP'), page('01')]) {
    assert.deepEqual(await inspect(t, directory, input), { code: 1, output: '', errors: 'runner-transcript-inspection-failed\n' });
  }
  assert.deepEqual(await bytes(directory), before);
});
