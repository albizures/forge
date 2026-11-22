import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, rm, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { DatabaseSync } from 'node:sqlite';
import { fork } from 'node:child_process';
import { once } from 'node:events';
import { openReadOnlyStorage } from './readonly-storage.mjs';
import { open, context, fauxAssistantMessage, fauxText } from './fixture.mjs';

async function directory(t) {
  const dir = await mkdtemp(join(tmpdir(), 'forge-t01-reader-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  return dir;
}

test('derived read adapter: no initialization writes, no scheduler, stable committed pagination beside writer', async t => {
  const path = join(await directory(t), 'runtime.sqlite');
  const writer = await open(path, { responses: Array.from({ length: 4 }, () => fauxAssistantMessage([fauxText('done')])) });
  t.after(() => writer.harness.close(context));
  for (const content of ['one', 'two', 'three']) {
    await (await writer.root.submit({ type: 'input', content }, context)).wait(context);
  }
  const sql = [];
  const observer = await open(path, { readOnly: true, reader: true, audit: (operation, text) => sql.push([operation, text]) });
  t.after(() => observer.harness.close(context));
  await assert.rejects(observer.storage.mintId(), /mintId forbidden/);
  await assert.rejects(observer.storage.commit([], () => {}, context), /commit forbidden/);
  assert.throws(() => observer.harness.resume(), /read.only/i);
  await assert.rejects(async () => observer.root.submit({ type: 'input', content: 'not allowed' }, context), /read.only/i);
  const allBefore = (await observer.root.entries({}, 100, undefined, context)).items;
  const first = await observer.root.entries({}, 2, undefined, context);
  assert.ok(first.next);
  await (await writer.root.submit({ type: 'input', content: 'four' }, context)).wait(context);
  const paged = [...first.items];
  let cursor = first.next;
  while (cursor) {
    const page = await observer.root.entries({}, 2, cursor, context);
    paged.push(...page.items);
    cursor = page.next;
  }
  assert.deepEqual(paged.map(entry => entry.id), allBefore.map(entry => entry.id));
  assert.ok((await observer.root.entries({}, 100, undefined, context)).items.length > allBefore.length);
  assert.ok(sql.length > 0);
  assert.ok(sql.every(([op, text]) => ['get', 'all'].includes(op) && /^SELECT\s/i.test(text)));
});

test('derived adapter rejects missing, older, newer and uninitialized schemas without modifying files', async t => {
  const dir = await directory(t);
  const absent = join(dir, 'absent.sqlite');
  await assert.rejects(openReadOnlyStorage(absent));
  await assert.rejects(readFile(absent), { code: 'ENOENT' });
  for (const version of [undefined, 1, 3]) {
    const path = join(dir, `${version}.sqlite`);
    const db = new DatabaseSync(path);
    if (version !== undefined) {
      db.exec('CREATE TABLE durable_schema (singleton INTEGER PRIMARY KEY, version INTEGER)');
      db.prepare('INSERT INTO durable_schema VALUES (1, ?)').run(version);
    }
    db.close();
    const before = await readFile(path);
    await assert.rejects(openReadOnlyStorage(path));
    assert.deepEqual(await readFile(path), before);
  }
});

test('derived adapter observes interrupted unsafe intent after process death without recovery writes', async t => {
  const dir = await directory(t);
  const path = join(dir, 'runtime.sqlite');
  const effects = join(dir, 'effects');
  const child = fork(new URL('./crash-worker.mjs', import.meta.url), [path, effects], { stdio: ['ignore', 'ignore', 'inherit', 'ipc'] });
  t.after(() => { if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL'); });
  await Promise.race([
    once(child, 'message'),
    once(child, 'exit').then(() => { throw new Error('worker exited before effect'); }),
  ]);
  const exited = once(child, 'exit');
  child.kill('SIGKILL');
  await exited;
  const source = new DatabaseSync(path, { readOnly: true });
  t.after(() => source.close());
  const before = source.prepare('SELECT record FROM tasks ORDER BY id').all();
  const observer = await open(path, { readOnly: true, reader: true });
  await observer.harness.inspect(context);
  await observer.root.entries({}, 100, undefined, context);
  await observer.harness.close(context);
  assert.deepEqual(source.prepare('SELECT record FROM tasks ORDER BY id').all(), before);
  assert.equal(await readFile(effects, 'utf8'), 'mutation\n');
});
