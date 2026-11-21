import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fork } from 'node:child_process';
import { once } from 'node:events';
import { createModels } from '@earendil-works/pi-ai';
import { openaiProvider } from '@earendil-works/pi-ai/providers/openai';
import { hook, GenerationTask } from '@earendil-works/pi-durable';
import { open, context, fauxAssistantMessage, fauxText } from './fixture.mjs';

async function isolated(t) {
  const dir = await mkdtemp(join(tmpdir(), 'forge-t01-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  return { path: join(dir, 'runtime.sqlite'), effects: join(dir, 'effects') };
}
const answer = () => fauxAssistantMessage([fauxText('done')]);

test('explicit isolated OpenAI API-key auth; capabilities do not supply provider auth', async () => {
  const configured = createModels({ authContext: {
    env: async name => name === 'OPENAI_API_KEY' ? 'synthetic-not-a-real-key' : undefined,
    fileExists: async () => false,
  } });
  configured.setProvider(openaiProvider());
  assert.equal((await configured.checkAuth('openai')).type, 'api_key');
  assert.equal((await configured.getAuth('openai')).auth.apiKey, 'synthetic-not-a-real-key');
  const absent = createModels({ authContext: {
    env: async name => name === 'FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE' ? '/unused' : undefined,
    fileExists: async () => false,
  } });
  absent.setProvider(openaiProvider());
  assert.equal(await absent.checkAuth('openai'), undefined);
  assert.equal(await absent.getAuth('openai'), undefined);
});

test('close propagates cancellation to an active model request', async t => {
  const { path } = await isolated(t);
  const started = Promise.withResolvers();
  const cancelled = Promise.withResolvers();
  const { harness, root } = await open(path, { responses: [(_messages, options) => new Promise((_resolve, reject) => {
    options.signal.addEventListener('abort', () => { cancelled.resolve(); reject(options.signal.reason); }, { once: true });
    started.resolve();
  })] });
  t.after(() => harness.close(context));
  await root.submit({ type: 'input', content: 'wait' }, context);
  await started.promise;
  await harness.close(context);
  await cancelled.promise;
});

test('throwing beforeRequest is NOT a fail-closed authorization boundary', async t => {
  const { path } = await isolated(t);
  let requests = 0;
  const { harness, root } = await open(path, {
    hooks: [hook(GenerationTask, { beforeRequest: () => { throw new Error('synthetic lease denial'); } })],
    responses: [() => { requests++; return answer(); }],
  });
  t.after(() => harness.close(context));
  await (await root.submit({ type: 'input', content: 'go' }, context)).wait(context);
  assert.equal(requests, 1);
});

test('same requestId deduplicates but DOES NOT reject changed content', async t => {
  const { path } = await isolated(t);
  const { harness, root } = await open(path, { responses: [answer()] });
  t.after(() => harness.close(context));
  const first = await root.submit({ type: 'input', content: 'original', requestId: 'one' }, context);
  await first.wait(context);
  const retry = await root.submit({ type: 'input', content: 'different', requestId: 'one' }, context);
  assert.equal(retry.id, first.id);
});

test('stock API limitation: physical read-only SQLite open attempts initialization writes', async t => {
  const { path } = await isolated(t);
  const writer = await open(path);
  t.after(() => writer.harness.close(context));
  await assert.rejects(open(path, { readOnly: true }), /attempt to write a readonly database/);
});

test('logical read-only Harness observes beside writer, but storage still opens writable', async t => {
  const { path } = await isolated(t);
  const writer = await open(path, { responses: [answer(), answer()] });
  t.after(() => writer.harness.close(context));
  await (await writer.root.submit({ type: 'input', content: 'first' }, context)).wait(context);
  const observer = await open(path, { readOnly: true, physicalReadOnly: false });
  t.after(() => observer.harness.close(context));
  assert.ok((await observer.root.entries({}, 100, undefined, context)).items.length > 0);
  assert.throws(() => observer.harness.resume(), /read.only/i);
  await assert.rejects(async () => observer.root.submit({ type: 'input', content: 'forbidden' }, context), /read.only/i);
  const before = (await observer.root.entries({}, 100, undefined, context)).items.length;
  await (await writer.root.submit({ type: 'input', content: 'second' }, context)).wait(context);
  assert.ok((await observer.root.entries({}, 100, undefined, context)).items.length > before);
});

test('SIGKILL after unsafe effect: open does not execute; resume reports interrupted without replay', async t => {
  const { path, effects } = await isolated(t);
  const child = fork(new URL('./crash-worker.mjs', import.meta.url), [path, effects], { stdio: ['ignore', 'ignore', 'inherit', 'ipc'] });
  t.after(() => { if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL'); });
  const message = await Promise.race([
    once(child, 'message'),
    once(child, 'exit').then(([code]) => { throw new Error(`worker exited early: ${code}`); }),
  ]);
  assert.equal(message[0], 'effect');
  const exit = once(child, 'exit');
  child.kill('SIGKILL');
  await exit;
  assert.equal(await readFile(effects, 'utf8'), 'mutation\n');
  let reruns = 0;
  const recovered = await open(path, { responses: [answer()], tool: async () => { reruns++; return {}; } });
  t.after(() => recovered.harness.close(context));
  const inspection = await recovered.harness.inspect(context);
  assert.ok(inspection.tasks.length > 0);
  assert.equal(reruns, 0);
  const input = await recovered.root.submit({ type: 'input', content: 'go', requestId: 'crash-input' }, context);
  await input.wait(context);
  assert.equal(reruns, 0);
  assert.equal(await readFile(effects, 'utf8'), 'mutation\n');
  const page = await recovered.root.entries({}, 100, undefined, context);
  assert.match(JSON.stringify(page.items), /interrupted/);
});
