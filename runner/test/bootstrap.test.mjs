import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, mkdir, writeFile, readFile, rm, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Authorization } from '../src/authorization.ts';
import { bootstrapCheck } from '../src/bootstrap.ts';
import { createCheckRuntime } from '../src/check-runtime.ts';

const grant = JSON.parse(await readFile(new URL('./authorization-lease.json', import.meta.url), 'utf8'));
const denied = { code: 'assignment-bootstrap-failed' };
const canary = 'T07-SECRET-MUST-NOT-APPEAR';

async function fixture(t, source) {
  const directory = await mkdtemp(join(tmpdir(), 'forge-bootstrap-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const cwd = join(directory, 'worktree');
  await mkdir(cwd);
  const cli = join(directory, 'forge');
  const marker = join(directory, 'called.json');
  const replyFile = join(directory, 'reply.json');
  const bound = {
    assignmentId: grant.launch.assignment_id, recordId: 7,
    profileRevision: grant.launch.profile_revision, profileRecordType: 'task',
    provider: 'pi-durable', agentKind: 'pi', model: 'openai/gpt-4.1', attemptNumber: 1,
    checkAttempt: { id: grant.launch.attempt_id, record_id: 7, check_id: 9, generation: grant.generation, evidence: { kind: 'repository', repository: '/approved', commit: 'a'.repeat(40) }, actor: 'forge', body: 'Bound work', continuation: null, recovery: null },
    executionDirectory: cwd, providerWorkspace: null,
  };
  const reply = { schemaVersion: 1, context: { schemaVersion: 1, bound: structuredClone(bound), record: { id: 7, record_type: 'task', status: 'in_progress' }, trustedConstraints: ['Only bound work'], freshness: 'current' }, behavior: 'PRIVATE-BOUND-BEHAVIOR' };
  await writeFile(replyFile, JSON.stringify(reply));
  await writeFile(cli, `#!${process.execPath}\nimport {writeFileSync,readFileSync} from 'node:fs';\nwriteFileSync(${JSON.stringify(marker)}, JSON.stringify({argv:process.argv.slice(2),cwd:process.cwd(),env:process.env}));\n${source ?? `process.stdout.write(readFileSync(${JSON.stringify(replyFile)}));`}\n`, { mode: 0o700 });
  // No .js extension: make the fake CLI's module syntax unambiguous.
  await writeFile(join(directory, 'package.json'), '{"type":"module"}');
  const capability = { FORGE_ASSIGNMENT_ID: grant.launch.assignment_id, FORGE_ASSIGNMENT_DAEMON_URL: 'http://127.0.0.1:1', FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE: join(directory, 'capability'), FORGE_ASSIGNMENT_FORGE_CLI: cli };
  await writeFile(capability.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE, canary, { mode: 0o600 });
  let now = 1000;
  const authorization = new Authorization(grant.launch, grant.generation, grant.owner, grant.daemon_epoch, () => now);
  authorization.accept({ ...grant, sequence: 1, issued_boottime_millis: 1000, expires_boottime_millis: 31000 });
  t.after(() => authorization.close());
  return { bound, reply, replyFile, marker, capability, authorization, expire: () => { now = 31000; }, run: () => bootstrapCheck(grant.launch, grant.generation, bound, capability, authorization) };
}

async function runtimeFixture(t) {
  const f = await fixture(t);
  const home = join(f.bound.executionDirectory, 'auth');
  await mkdir(home, { mode: 0o700 });
  const key = join(home, 'key');
  await writeFile(key, 'T07-MODEL-KEY', { mode: 0o600 });
  const config = { version: 1, model: 'openai/gpt-4.1', thinkingLevel: 'off', tools: ['bash'], context: { instructions: 'Approved profile', resources: [] }, cwd: f.bound.executionDirectory };
  return { ...f, key, config, runtime: () => createCheckRuntime(grant.launch, grant.generation, f.bound, f.capability, config, key, f.authorization) };
}

test('Check runtime composes authenticated behavior/context only after bootstrap', async t => {
  const f = await runtimeFixture(t);
  const pending = f.runtime();
  // Mutate caller-owned inputs while the actual child bootstrap is pending.
  f.config.cwd = '/';
  f.config.context.instructions = 'MUTATED-CALLER-CANARY';
  f.capability.FORGE_ASSIGNMENT_ID = 'foreign';
  const runtime = await pending;
  assert.ok(!runtime.agent.instructions.includes('MUTATED-CALLER-CANARY'));
  assert.ok(runtime.agent.instructions.includes(f.reply.behavior));
  assert.ok(runtime.agent.instructions.includes(JSON.stringify(f.reply.context)));
  assert.ok(runtime.agent.instructions.startsWith('Approved profile'));
  assert.equal(runtime.agent.cwd, f.bound.executionDirectory);
  assert.deepEqual(runtime.agent.tools.map(tool => tool.name), ['bash']);
  assert.equal(runtime.agent.tools[0].replay, 'unsafe');
  assert.ok(!JSON.stringify(runtime.agent).includes(canary));
  assert.ok(!JSON.stringify(runtime.agent).includes('T07-MODEL-KEY'));
  f.authorization.admit();
});

test('Check runtime fences bootstrap, configuration and model-auth failures without a usable runtime', async t => {
  for (const change of [
    f => { f.config.cwd = '/'; },
    f => f.expire(),
    f => writeFile(f.replyFile, '{}'),
    f => { f.config.tools = ['unsupported']; },
    f => rm(f.key),
  ]) {
    const f = await runtimeFixture(t);
    await change(f);
    await assert.rejects(f.runtime(), { code: 'assignment-runtime-failed' });
    fenced(f);
  }
});

async function notCalled(f) { await assert.rejects(access(f.marker), { code: 'ENOENT' }); }
function fenced(f) { assert.throws(() => f.authorization.admit(), { code: 'assignment-lease-denied' }); }

test('Check bootstrap invokes only the delivered CLI with exact argv, cwd and allowlisted metadata', async t => {
  const f = await fixture(t);
  // An extra caller-owned field must not reach the child; no global env mutation.
  f.capability.OPENAI_API_KEY = canary;
  const result = await f.run();
  assert.deepEqual(result, { context: f.reply.context, behavior: f.reply.behavior });
  const called = JSON.parse(await readFile(f.marker, 'utf8'));
  assert.deepEqual(called.argv, ['assignment', 'bootstrap', '--json']);
  assert.equal(called.cwd, f.bound.executionDirectory);
  assert.equal(called.env.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE, f.capability.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE);
  assert.equal(called.env.OPENAI_API_KEY, undefined);
  assert.equal(called.env.HOME, undefined);
  assert.equal(called.env.PI_CODING_AGENT_DIR, undefined);
  assert(!JSON.stringify(called).includes(canary));
  f.authorization.admit();
});

test('lease absence, expiry and another Check grant deny before spawning', async t => {
  for (const change of [f => f.authorization.close(), f => f.expire(), f => { f.bound.checkAttempt.generation++; }, f => { f.bound.assignmentId = 'other'; }, f => { f.bound.provider = 'herdr'; }, f => { f.bound.checkAttempt.check_id = Number.MAX_SAFE_INTEGER + 1; }, f => { f.bound.recordId = 0; }]) {
    const f = await fixture(t);
    change(f);
    await assert.rejects(f.run(), denied);
    await notCalled(f); fenced(f);
  }
  const f = await fixture(t);
  const other = { ...grant.launch, runtime_id: 'different-runtime' };
  await assert.rejects(bootstrapCheck(other, grant.generation, f.bound, f.capability, f.authorization), denied);
  await notCalled(f); fenced(f);
});

test('wrong or stale immutable identity, worktree and evidence never release behavior', async t => {
  for (const change of [
    r => { r.context.bound.assignmentId = 'other'; },
    r => { r.context.bound.profileRevision++; },
    r => { r.context.bound.checkAttempt.generation++; },
    r => { r.context.bound.checkAttempt.evidence.commit = 'b'.repeat(40); },
    r => { r.context.bound.executionDirectory = '/'; },
    r => { r.context.bound.checkAttempt.recovery = { message: 'unapproved' }; },
    r => { r.context.record.id++; },
    r => { r.context.record.status = 'done'; },
    r => { r.schemaVersion++; },
    r => { r.context.schemaVersion++; },
    r => { r.behavior = ''; },
  ]) {
    const f = await fixture(t);
    change(f.reply);
    await writeFile(f.replyFile, JSON.stringify(f.reply));
    await assert.rejects(f.run(), denied);
    fenced(f);
  }
});

test('nonzero, malformed, truncated, oversized and spawn failures are fixed redacted errors', async t => {
  for (const source of [
    `process.stdout.write(${JSON.stringify(canary)}); process.stderr.write(${JSON.stringify(canary)}); process.exitCode=1;`,
    `process.stdout.write('{"behavior":"${canary}"');`,
    `process.stdout.write('null');`,
    `process.stdout.write(Buffer.from([0xff]));`,
    `process.stdout.write('x'.repeat(1024*1024+1));`,
  ]) {
    const f = await fixture(t, source);
    await assert.rejects(f.run(), error => error.code === denied.code && error.message === denied.code && !String(error).includes(canary));
    fenced(f);
  }
  const f = await fixture(t);
  await writeFile(f.capability.FORGE_ASSIGNMENT_FORGE_CLI, '#!/does-not-exist\n');
  await assert.rejects(f.run(), denied);
  fenced(f);
});

test('an unresponsive CLI times out without retrying or accepting partial output', async t => {
  const f = await fixture(t, `process.stdout.write('{'); setInterval(()=>{},1000);`);
  await assert.rejects(f.run(), denied);
  fenced(f);
  assert.deepEqual(JSON.parse(await readFile(f.marker, 'utf8')).argv, ['assignment', 'bootstrap', '--json']);
});

test('lease cancellation during bootstrap rejects even a previously printed complete reply', async t => {
  // Explicit ready handshake: do not depend on process startup speed.
  const f = await fixture(t);
  const ready = join(f.bound.executionDirectory, 'ready');
  await writeFile(f.capability.FORGE_ASSIGNMENT_FORGE_CLI, `#!${process.execPath}\nimport {writeFileSync,readFileSync} from 'node:fs'; process.stdout.write(readFileSync(${JSON.stringify(f.replyFile)})); writeFileSync(${JSON.stringify(ready)}, 'ready'); setInterval(()=>{},1000);\n`);
  const pending = assert.rejects(f.run(), denied);
  const deadline = Date.now() + 5000;
  for (;;) {
    try { await access(ready); break; } catch (error) { if (error.code !== 'ENOENT' || Date.now() > deadline) throw error; }
    await new Promise(resolve => setTimeout(resolve, 5));
  }
  f.authorization.close();
  await pending;
  fenced(f);
});
