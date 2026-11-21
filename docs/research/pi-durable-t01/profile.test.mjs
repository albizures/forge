import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, rm, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { NodeExecutionEnv } from '@earendil-works/pi-durable/env/node';
import { createModels } from '@earendil-works/pi-ai';
import { openaiProvider } from '@earendil-works/pi-ai/providers/openai';
import { Clock, Lease } from './authorization.mjs';
import { open, context, fauxAssistantMessage, fauxText, fauxToolCall } from './fixture.mjs';

test('explicit instructions and coding tools use bound cwd without ambient repository discovery', async t => {
  const dir = await mkdtemp(join(tmpdir(), 'forge-t01-profile-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  await writeFile(join(dir, 'AGENTS.md'), 'UNAPPROVED-AMBIENT-CANARY');
  const lease = new Lease(new Clock());
  t.after(() => lease.dispose());
  const call = (name, args) => fauxAssistantMessage([fauxToolCall(name, args, { id: name })], { stopReason: 'toolUse' });
  let prompt;
  const { harness, root } = await open(join(dir, 'runtime.sqlite'), { lease, coding: true,
    agent: { cwd: dir, thinkingLevel: 'off', instructions: 'EXPLICIT-APPROVED-INSTRUCTIONS' },
    env: new NodeExecutionEnv({ cwd: dir, shellPath: '/bin/sh', shellEnv: { PATH: '/usr/bin:/bin' } }),
    responses: [
      transcript => { prompt = JSON.stringify(transcript); return call('write', { path: 'bound.txt', content: 'before' }); },
      call('read', { path: 'bound.txt' }),
      call('edit', { path: 'bound.txt', oldText: 'before', newText: 'after' }),
      call('bash', { command: 'pwd' }),
      fauxAssistantMessage([fauxText('done')]),
    ],
  });
  t.after(() => harness.close(context));
  assert.equal((await (await root.submit({ type: 'input', content: 'go' }, context)).wait(context)).status, 'done');
  assert.equal(await readFile(join(dir, 'bound.txt'), 'utf8'), 'after');
  assert.match(prompt, /EXPLICIT-APPROVED-INSTRUCTIONS/);
  assert.doesNotMatch(prompt, /UNAPPROVED-AMBIENT-CANARY/);
  const transcript = JSON.stringify((await root.entries({}, 100, undefined, context)).items);
  assert.ok(transcript.includes(dir));
  assert.doesNotMatch(transcript, /isError":true/);
});

test('selected initial real-provider model exists in pinned catalog, thinking is off', () => {
  const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
  models.setProvider(openaiProvider());
  const model = models.getModel('openai', 'gpt-4.1');
  assert.ok(model);
  assert.equal(model.reasoning, false);
});
