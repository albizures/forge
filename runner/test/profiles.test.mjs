import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, mkdir, chmod, writeFile, readFile, readdir, rm, symlink, link } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { Harness } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { Authorization } from '../src/authorization.ts';
import { createProfileRuntime, loadModelKey } from '../src/profiles.ts';

const wire = JSON.parse(await readFile(new URL('./profile-v1.json', import.meta.url), 'utf8'));
const grant = JSON.parse(await readFile(new URL('./authorization-lease.json', import.meta.url), 'utf8'));
const secret = 'sk-T06-PRIVATE-MODEL-CANARY';
const denied = { code: 'durable-model-auth-unavailable' };
async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'forge-t06-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const cwd = join(directory, 'worktree');
  const home = join(directory, 'home');
  await mkdir(cwd); await mkdir(home, { mode: 0o700 });
  const key = join(home, 'openai-key');
  await writeFile(key, secret + '\n', { mode: 0o600 });
  const config = { ...structuredClone(wire), cwd };
  let now = 1000;
  const authorization = new Authorization(grant.launch, grant.generation, grant.owner, grant.daemon_epoch, () => now);
  authorization.accept({ ...grant, sequence: 1, issued_boottime_millis: 1000, expires_boottime_millis: 31000 });
  t.after(() => authorization.close());
  return { directory, cwd, home, key, config, authorization, expire: () => { now = 31000; } };
}

test('explicit raw key file works; missing, malformed and unsafe sources fail redacted', async t => {
  const f = await fixture(t);
  assert.equal(loadModelKey(f.key), secret);
  for (const value of ['', ' ', 'key\nsecond', 'key\r\n', 'x'.repeat(4097), '\u00ff']) {
    await writeFile(f.key, value);
    assert.throws(() => loadModelKey(f.key), denied);
  }
  await writeFile(f.key, secret);
  await chmod(f.key, 0o644); assert.throws(() => loadModelKey(f.key), denied);
  await chmod(f.key, 0o600);
  await chmod(f.home, 0o755); assert.throws(() => loadModelKey(f.key), denied);
  await chmod(f.home, 0o700);
  const alias = join(f.home, 'alias'); await symlink(f.key, alias);
  assert.throws(() => loadModelKey(alias), denied);
  await rm(alias); await link(f.key, alias);
  assert.throws(() => loadModelKey(f.key), denied);
  await rm(alias);
  for (const path of [undefined, 'relative', f.home, join(f.home, 'absent')]) assert.throws(() => loadModelKey(path), denied);
  assert.throws(() => createProfileRuntime(f.config, undefined, f.authorization, { FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE: f.key }), denied);
  assert.throws(() => createProfileRuntime(f.config, f.key, f.authorization, { FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE: f.key }), denied);
});

test('runner configuration validates approved resource bytes and rejects unsupported inputs', async t => {
  const f = await fixture(t);
  const resource = join(f.cwd, 'SKILL.md');
  await writeFile(resource, 'Approved resource');
  f.config.context.resources = [{ name: 'approved', path: resource, sha256: createHash('sha256').update('Approved resource').digest('hex') }];
  f.config.context.instructions += '\n\nApproved resource';
  const runtime = createProfileRuntime(f.config, f.key, f.authorization);
  assert.deepEqual(runtime.agent.tools.map(tool => tool.name), wire.tools);
  assert.equal(runtime.agent.instructions, f.config.context.instructions);
  assert.equal(runtime.agent.cwd, f.cwd);
  assert.deepEqual(runtime.options.settings, { retry: { enabled: false }, compaction: { enabled: false } });
  for (const change of [
    { version: 2 }, { model: 'other/model' }, { thinkingLevel: 'high' }, { tools: ['grep'] },
    { tools: ['bash', 'bash'] }, { cwd: 'relative' }, { extensions: [] },
    { context: { instructions: 'x'.repeat(256 * 1024 + 1), resources: [] } },
    { context: { instructions: 'modified rendering', resources: f.config.context.resources } },
  ]) assert.throws(() => createProfileRuntime({ ...f.config, ...change }, f.key, f.authorization), { code: 'durable-profile-incompatible' });
  await writeFile(resource, 'drift');
  assert.throws(() => createProfileRuntime(f.config, f.key, f.authorization), { code: 'durable-profile-incompatible' });
  assert.throws(() => createProfileRuntime(wire, f.key, f.authorization), { code: 'durable-profile-incompatible' });
  const empty = createProfileRuntime({ ...f.config, context: wire.context, tools: [] }, f.key, f.authorization);
  assert.deepEqual(empty.agent.tools, []);
  for (const env of [{ OPENAI_API_KEY: secret }, { FORGE_ASSIGNMENT_CAPABILITY_SECRET: secret }, { FORGE_ASSIGNMENT_FORGE_CLI: 'relative' }]) {
    assert.throws(() => createProfileRuntime({ ...f.config, context: wire.context }, f.key, f.authorization, env), { code: 'durable-capability-env-invalid' });
  }
});

test('all four actual coding tools use explicit cwd/env, remain unsafe and deny expired admission', async t => {
  const f = await fixture(t);
  const capability = { FORGE_ASSIGNMENT_ID: 'assignment-fixture', FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE: join(f.home, 'capability-path') };
  const runtime = createProfileRuntime(f.config, f.key, f.authorization, capability);
  const env = runtime.options.env({ cwd: f.cwd }, context);
  assert.throws(() => runtime.options.env({ cwd: f.home }, context), { code: 'durable-cwd-mismatch' });
  const tools = runtime.options.registry.snapshot().extension('forge-coding').tools;
  let output = '';
  const api = { env, agent: runtime.agent, models: runtime.options.models, output: text => { output += text; }, diagnostic: () => {}, retainedOutput: () => ({ text: output, truncated: false }) };
  const execute = (name, args) => tools.find(tool => tool.name === name).execute(args, api, context);
  for (const tool of tools) assert.equal(tool.replay, 'unsafe');
  await execute('write', { path: 'example.txt', content: 'before' });
  await execute('edit', { path: 'example.txt', edits: [{ oldText: 'before', newText: 'after' }] });
  const result = await execute('read', { path: 'example.txt' });
  assert.ok(JSON.stringify(result).includes('after'));
  assert.equal(await readFile(join(f.cwd, 'example.txt'), 'utf8'), 'after');
  await execute('bash', { command: 'printf "%s|%s|%s|%s" "$PWD" "$OPENAI_API_KEY" "$FORGE_ASSIGNMENT_ID" "$FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE"' });
  assert.equal(output, `${f.cwd}||assignment-fixture|${capability.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE}`);
  assert.ok(!JSON.stringify(result).includes(secret));
  assert.ok(!output.includes(secret));
  f.expire();
  for (const tool of tools) assert.throws(() => tool.execute({}, api, context), { code: 'assignment-lease-denied' });
  assert.equal(await readFile(join(f.cwd, 'example.txt'), 'utf8'), 'after');
});

test('actual OpenAI auth/request path disables retries and redacts provider errors', async t => {
  const f = await fixture(t);
  const runtime = createProfileRuntime(f.config, f.key, f.authorization);
  const models = runtime.options.models;
  assert.deepEqual(models.getProviders().map(p => p.id), ['openai']);
  assert.equal(models.getProvider('openai').auth.oauth, undefined);
  const auth = await models.getAuth('openai');
  assert.equal(auth.auth.apiKey, secret);
  const model = models.getModel('openai', 'gpt-4.1');
  let requests = 0;
  const message = await models.completeSimple(model, { messages: [{ role: 'user', content: 'controlled input', timestamp: Date.now() }] }, {
    maxRetries: 4,
    fetch: async (_url, options) => {
      requests++;
      assert.equal(new Headers(options.headers).get('authorization'), `Bearer ${secret}`);
      return new Response(JSON.stringify({ error: { message: `error echoes ${secret}` } }), { status: 429, headers: { 'content-type': 'application/json' } });
    },
  });
  assert.equal(requests, 1);
  assert.equal(message.stopReason, 'error');
  assert.ok(!JSON.stringify(message).includes(secret));
  const event = { type: 'response.completed', response: { id: 'fixture-response', status: 'completed', output: [{ type: 'message', id: 'fixture-message', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: 'Controlled success', annotations: [] }] }], usage: { input_tokens: 1, output_tokens: 1, total_tokens: 2 } } };
  const success = await models.completeSimple(model, { messages: [{ role: 'user', content: 'controlled success', timestamp: Date.now() }] }, {
    fetch: async () => {
      const events = [
        { type: 'response.output_item.added', output_index: 0, item: { type: 'message', id: 'fixture-message', role: 'assistant', content: [] } },
        { type: 'response.output_text.delta', output_index: 0, content_index: 0, delta: 'Controlled success' },
        { type: 'response.output_item.done', output_index: 0, item: event.response.output[0] },
        event,
      ];
      return new Response(events.map(item => `event: ${item.type}\ndata: ${JSON.stringify(item)}\n\n`).join(''), { headers: { 'content-type': 'text/event-stream' } });
    },
  });
  assert.equal(success.stopReason, 'stop');
  assert.ok(JSON.stringify(success).includes('Controlled success'));
  assert.ok(!JSON.stringify(success).includes(secret));
  const provider = models.getProvider('openai');
  for (const name of ['fetchDeferred', 'generateImages', 'classify']) assert.throws(() => provider[name](model, {}), { code: 'durable-request-unsupported' });
  assert.throws(() => provider.streamSimple({ ...model, id: 'gpt-other' }, {}), { code: 'durable-model-unsupported' });
  f.expire();
  assert.throws(() => provider.streamSimple(model, {}), { code: 'assignment-lease-denied' });
  assert.equal(requests, 1);
});

test('pinned Harness persists redacted request failure, not model credentials', async t => {
  const f = await fixture(t);
  const approved = 'Approved Markdown context';
  const pathResource = join(f.cwd, 'approved.md');
  await writeFile(pathResource, approved);
  await writeFile(join(f.cwd, 'AGENTS.md'), 'ambient-canary');
  f.config.context.resources = [{ name: 'approved', path: pathResource, sha256: createHash('sha256').update(approved).digest('hex') }];
  f.config.context.instructions += '\n\n' + approved;
  const runtime = createProfileRuntime(f.config, f.key, f.authorization);
  let calls = 0, payload;
  t.mock.method(globalThis, 'fetch', async (_url, options) => {
    calls++; payload = JSON.parse(options.body);
    return new Response(JSON.stringify({ error: { message: `private ${secret}` } }), { status: 401, headers: { 'content-type': 'application/json' } });
  });
  const path = join(f.directory, 'runtime.sqlite');
  const harness = await Harness.open(await openNodeSqliteStorage(path), runtime.options, context);
  try {
    const root = await harness.root(context, { agent: runtime.agent });
    await (await root.submit({ type: 'input', content: 'Controlled input' }, context)).wait(context);
    assert.equal(calls, 1);
    assert.ok(JSON.stringify(payload).includes('Explicit profile behavior'));
    assert.ok(JSON.stringify(payload).includes(approved));
    assert.ok(!JSON.stringify(payload).includes('ambient-canary'));
    assert.ok(!JSON.stringify(payload).includes(secret));
    for (const name of (await readdir(f.directory)).filter(name => name.startsWith('runtime.sqlite'))) {
      assert.equal((await readFile(join(f.directory, name))).includes(Buffer.from(secret)), false);
    }
  } finally { await harness.close(context); }
  const database = await readFile(path);
  assert.equal(database.includes(Buffer.from(secret)), false);
});

test('isolated ambient HOME, credentials and Pi settings cannot supply auth or alter configuration', async t => {
  const f = await fixture(t);
  await mkdir(join(f.home, '.pi/agent'), { recursive: true });
  await writeFile(join(f.home, '.pi/agent/settings.json'), JSON.stringify({ defaultModel: 'ambient-canary', extensions: ['ambient-canary'] }));
  await writeFile(join(f.home, '.pi/agent/auth.json'), JSON.stringify({ openai: { type: 'api_key', key: 'ambient-canary' } }));
  await writeFile(join(f.cwd, 'AGENTS.md'), 'ambient-canary');
  const script = `
    import {createProfileRuntime} from ${JSON.stringify(new URL('../src/profiles.ts', import.meta.url).href)};
    import {Authorization} from ${JSON.stringify(new URL('../src/authorization.ts', import.meta.url).href)};
    const grant = ${JSON.stringify(grant)};
    const a = new Authorization(grant.launch, grant.generation, grant.owner, grant.daemon_epoch, () => 1000);
    try {
      try { createProfileRuntime(${JSON.stringify(f.config)}, undefined, a); throw Error('ambient auth admitted'); }
      catch (error) { if(error.code !== 'durable-model-auth-unavailable') throw error; }
      const result = createProfileRuntime(${JSON.stringify(f.config)}, ${JSON.stringify(f.key)}, a);
      console.log(JSON.stringify({agent:result.agent,auth:(await result.options.models.getAuth('openai')).source}));
    } finally { a.close(); }
  `;
  const child = spawn(process.execPath, ['--input-type=module', '-e', script], { env: { HOME: f.home, OPENAI_API_KEY: 'ambient-canary', FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE: f.key }, stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '', stderr = '';
  child.stdout.on('data', chunk => { stdout += chunk; }); child.stderr.on('data', chunk => { stderr += chunk; });
  const [code] = await once(child, 'close');
  assert.equal(code, 0, stderr);
  assert.ok(!stdout.includes('ambient-canary') && !stdout.includes(secret));
  assert.equal(JSON.parse(stdout).agent.model.modelId, 'gpt-4.1');
});
