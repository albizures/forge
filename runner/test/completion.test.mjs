import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, mkdir, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { Harness, createRegistry } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { createModels, fauxProvider, fauxAssistantMessage } from '@earendil-works/pi-ai';
import { databasePath, observeAnswer, persistIntent, persistRoot, validateDirectory } from '../src/storage.ts';

const wire = JSON.parse(await readFile(new URL('./wire-v1.json', import.meta.url), 'utf8'));
const launch = wire.operation.launch;
async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'forge-answer-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const directory = join(root, 'runtime');
  const worktree = join(root, 'worktree');
  await mkdir(directory, { mode: 0o700 });
  await mkdir(worktree);
  validateDirectory(directory, worktree);
  const provider = fauxProvider();
  const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
  models.setProvider(provider.provider);
  return { directory, provider, async open() {
    persistIntent(directory, launch);
    const harness = await Harness.open(await openNodeSqliteStorage(databasePath(directory)), { models, registry: createRegistry(), env: () => undefined }, context);
    t.after(() => harness.close(context));
    const conversation = await harness.root(context, { agent: { model: { provider: 'faux', modelId: 'faux-1' }, cwd: worktree } });
    persistRoot(directory, conversation.id);
    return { harness, conversation };
  } };
}
async function bytes(directory) {
  // SQLite may create empty WAL/SHM coordination files even for read-only DB
  // handles. Compare committed database/WAL bytes, not transient reader locks.
  const files = await Promise.all((await readdir(directory)).filter(name => !name.endsWith('-shm')).map(async name => [name, await readFile(join(directory, name))]));
  return Object.fromEntries(files.filter(([name, data]) => !name.endsWith('-wal') || data.length > 0));
}

test('answer inspection distinguishes absent intent/root/input without writable initialization', async t => {
  const f = await fixture(t);
  assert.equal(await observeAnswer(f.directory, launch, '1'), 'unknown');
  assert.deepEqual(await readdir(f.directory), []);
  persistIntent(f.directory, launch);
  const before = await bytes(f.directory);
  assert.equal(await observeAnswer(f.directory, launch, '1'), 'unknown');
  assert.deepEqual(await bytes(f.directory), before);
});

test('committed input answer is readable beside a writer and after close without execution or writes', async t => {
  const f = await fixture(t);
  let release, started;
  const entered = new Promise(resolve => { started = resolve; });
  t.after(() => release?.());
  f.provider.setResponses([async () => {
    started();
    await new Promise(resolve => { release = resolve; });
    return fauxAssistantMessage('Plausible final prose, not an Outcome.');
  }]);
  const { harness, conversation } = await f.open();
  const submission = await conversation.submit({ type: 'input', requestId: launch.operation_id, content: 'Bound input' }, context);
  const waiting = submission.wait(context);
  await entered;
  assert.equal(await observeAnswer(f.directory, launch, String(submission.id)), 'unknown');
  assert.equal(f.provider.state.callCount, 1);
  release();
  const settled = await waiting;
  assert.equal(settled.status, 'done');
  assert.equal(await observeAnswer(f.directory, launch, String(submission.id)), 'answered');
  const changing = structuredClone(launch);
  const pending = observeAnswer(f.directory, changing, String(submission.id));
  changing.operation_id = 'caller-mutation';
  assert.equal(await pending, 'answered');
  await harness.close(context);
  const before = await bytes(f.directory);
  assert.equal(await observeAnswer(f.directory, launch, String(submission.id)), 'answered');
  assert.deepEqual(await bytes(f.directory), before);
  assert.equal(f.provider.state.callCount, 1);
});

test('failed runs are unanswered, not answers or permission to restart', async t => {
  const f = await fixture(t);
  f.provider.setResponses([fauxAssistantMessage([], { stopReason: 'error', errorMessage: 'Controlled provider failure' })]);
  const { harness, conversation } = await f.open();
  const submission = await conversation.submit({ type: 'input', requestId: launch.operation_id, content: 'Bound input' }, context);
  assert.equal((await submission.wait(context)).status, 'unanswered');
  await harness.close(context);
  const before = await bytes(f.directory);
  assert.equal(await observeAnswer(f.directory, launch, String(submission.id)), 'unanswered');
  assert.deepEqual(await bytes(f.directory), before);
  assert.equal(f.provider.state.callCount, 1);
});

test('answer inspection rejects mismatched full launch/admission and passive writes', async t => {
  const f = await fixture(t);
  const { harness, conversation } = await f.open();
  assert.equal(await observeAnswer(f.directory, launch, '1'), 'unknown');
  const passive = await conversation.submit({ type: 'write', requestId: launch.operation_id, entry: { kind: 'fixture', data: {} } }, context);
  assert.equal((await passive.wait(context)).status, 'done');
  await harness.close(context);
  const before = await bytes(f.directory);
  for (const field of Object.keys(launch)) {
    const changed = { ...launch, [field]: typeof launch[field] === 'number' ? launch[field] + 1 : launch[field] + '-foreign' };
    await assert.rejects(observeAnswer(f.directory, changed, String(passive.id)), { code: 'runner-identity-conflict' });
  }
  await assert.rejects(observeAnswer(f.directory, launch, ''), { code: 'runner-admission-invalid' });
  await assert.rejects(observeAnswer(f.directory, launch, String(passive.id)), { code: 'runner-admission-conflict' });
  await assert.rejects(observeAnswer(f.directory, launch, 'foreign'), { code: 'runner-admission-conflict' });
  assert.deepEqual(await bytes(f.directory), before);
  assert.equal(f.provider.state.callCount, 0);
});
