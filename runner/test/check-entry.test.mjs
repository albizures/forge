// The PACKAGED entry (src/check-entry.ts), unsubstituted, under the real
// forge-runner-host. Every case here stops before any model request, so the real
// OpenAI provider is constructed but never called.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createInterface } from 'node:readline';
import { access, chmod, mkdir, mkdtemp, readFile, readdir, readlink, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compatibility } from '../src/contract.ts';

const root = fileURLToPath(new URL('../../', import.meta.url));
const host = resolve(root, 'target/debug/forge-runner-host');
const entry = fileURLToPath(new URL('../src/check-entry.ts', import.meta.url));
const KEY = 'T22-MODEL-KEY-CANARY';
const CAPABILITY = 'T22-CAPABILITY-CANARY';
const exists = path => access(path).then(() => true, () => false);

async function fixture(t, { bootstrap = 'ok', key = true, tools = ['read', 'bash'] } = {}) {
  const base = await mkdtemp(join(tmpdir(), 'forge-entry-'));
  t.after(() => rm(base, { recursive: true, force: true }));
  const directory = join(base, 'runtime'), worktree = join(base, 'worktree'), auth = join(base, 'auth'), bin = join(base, 'bin');
  for (const path of [directory, worktree, auth, bin]) await mkdir(path, { mode: 0o700 });
  const launch = { assignment_id: 'a', operation_id: 'launch-a', attempt_id: 'attempt-a', profile_revision: 1, runtime_id: 'runtime-a', incarnation_id: 'incarnation-a', conversation_id: 'conversation-a', store_id: 'store-a', forge_revision: 'fixture', component_hash: compatibility().component_hash, runtime_schema: 2 };
  const bound = {
    assignmentId: 'a', recordId: 7, profileRevision: 1, profileRecordType: 'task', provider: 'pi-durable', agentKind: 'pi', model: 'openai/gpt-4.1', attemptNumber: 1,
    checkAttempt: { id: 'attempt-a', record_id: 7, check_id: 9, generation: 1, evidence: { kind: 'repository', repository: '/approved', commit: 'a'.repeat(40) }, actor: 'forge', body: 'Bound work', continuation: null, recovery: null },
    executionDirectory: worktree, providerWorkspace: null,
  };
  const reply = { schemaVersion: 1, context: { schemaVersion: 1, bound, record: { id: 7, record_type: 'task', status: 'in_progress' }, trustedConstraints: [], freshness: 'current' }, behavior: 'T22-PRIVATE-BEHAVIOR' };
  const marker = join(base, 'bootstrap-called');
  const cli = join(bin, 'forge');
  await writeFile(join(bin, 'package.json'), '{"type":"module"}');
  await writeFile(cli, `#!${process.execPath}\nimport {writeFileSync} from 'node:fs';\nwriteFileSync(${JSON.stringify(marker)}, 'called');\n${bootstrap === 'ok' ? `process.stdout.write(${JSON.stringify(JSON.stringify(reply))});` : 'process.exit(1);'}\n`, { mode: 0o700 });
  const secret = join(base, 'capability');
  await writeFile(secret, CAPABILITY, { mode: 0o600 });
  const credential = join(auth, 'key');
  if (key) await writeFile(credential, KEY, { mode: 0o600 });
  const input = {
    version: 1, launch, generation: 1, bound,
    configuration: { version: 1, model: 'openai/gpt-4.1', thinkingLevel: 'off', tools, context: { instructions: 'Approved profile', resources: [] }, cwd: worktree },
    credential_file: credential,
    capability: { FORGE_ASSIGNMENT_ID: 'a', FORGE_ASSIGNMENT_DAEMON_URL: 'http://127.0.0.1:1', FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE: secret, FORGE_ASSIGNMENT_FORGE_CLI: cli },
  };
  const file = join(directory, 'check-runtime.json');
  await writeFile(file, JSON.stringify(input), { mode: 0o600 });
  return { base, directory, worktree, launch, input, file, marker };
}

function start(t, f, runner = entry) {
  const child = spawn(host, [f.directory, f.worktree, process.execPath, runner], { stdio: ['pipe', 'pipe', 'pipe'] });
  let output = '';
  child.stderr.on('data', data => { output += data; });
  const queue = [], pending = [];
  let identity;
  const lines = createInterface({ input: child.stdout });
  lines.on('line', line => {
    output += line + '\n';
    const value = JSON.parse(line);
    if (value.event === 'owned') identity = value.identity;
    const index = pending.findIndex(p => p.match(value));
    if (index < 0) queue.push(value); else pending.splice(index, 1)[0].resolve(value);
  });
  const exited = once(child, 'exit');
  t.after(async () => {
    // Test-only teardown of this directly spawned, still-verified worker/host.
    if (identity?.owner.host_pid === child.pid) {
      const stat = await readFile(`/proc/${identity.worker_pid}/stat`, 'utf8').catch(() => '');
      if (stat.slice(stat.lastIndexOf(')') + 2).split(' ')[19] === identity.worker_start) {
        try { process.kill(identity.worker_pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
      }
    }
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
    await exited;
    lines.close();
  });
  return {
    output: () => output,
    send: value => child.stdin.write(JSON.stringify(value) + '\n'),
    next(match) {
      const index = queue.findIndex(match);
      if (index >= 0) return Promise.resolve(queue.splice(index, 1)[0]);
      return new Promise(resolve => pending.push({ match, resolve }));
    },
    seen: match => queue.some(match),
    identity: () => identity,
  };
}
const request = (launch, kind = 'admit') => ({ compatibility: compatibility(), assignment_id: launch.assignment_id, runtime_id: launch.runtime_id, operation_id: kind === 'admit' ? launch.operation_id : 'observe', operation: kind === 'admit' ? { kind, launch } : { kind, admission_operation_id: launch.operation_id } });
const reply = p => p.next(x => x.admission || x.error);
async function authorize(p, launch) {
  const evidence = p.identity();
  const now = Math.floor(Number((await readFile('/proc/uptime', 'utf8')).split(' ')[0]) * 1000);
  p.send({ host_control: 1, lease: {
    launch, generation: 1,
    owner: { boot_id: evidence.owner.boot_id, time_namespace: await readlink('/proc/self/ns/time'), host_pid: evidence.owner.host_pid, host_start: evidence.owner.host_start, worker_pid: evidence.worker_pid, worker_start: evidence.worker_start, lock_device: evidence.owner.lock_device, lock_inode: evidence.owner.lock_inode },
    daemon_epoch: 'controlled-test-daemon', sequence: 1, issued_boottime_millis: now, expires_boottime_millis: now + 30000,
  } });
  assert.equal((await p.next(x => x.event === 'authorized')).sequence, 1);
}
function noSecrets(p) {
  for (const canary of [KEY, CAPABILITY, 'T22-PRIVATE-BEHAVIOR']) assert.ok(!p.output().includes(canary), canary);
}

test('packaged source tree contains no faux/synthetic provider and the entry substitutes nothing', async () => {
  const src = fileURLToPath(new URL('../src/', import.meta.url));
  for (const name of await readdir(src)) {
    const text = await readFile(join(src, name), 'utf8');
    assert.ok(!/faux/i.test(text.replace(/\/\/.*$/gm, '')), `${name} must not reference a faux provider`);
  }
  const source = await readFile(entry, 'utf8');
  assert.match(source, /process\.exitCode = await runCheckRunner\(directory, worktree\);/);
});

test('packaged entry refuses a missing, unprotected, malformed or inconsistent input before ownership', async t => {
  const cases = [
    ['missing', f => rm(f.file)],
    ['group-readable', f => chmod(f.file, 0o640)],
    ['extra key', f => writeFile(f.file, JSON.stringify({ ...f.input, behavior: 'injected' }), { mode: 0o600 })],
    ['generation drift', f => writeFile(f.file, JSON.stringify({ ...f.input, generation: 2 }), { mode: 0o600 })],
    ['wrong worktree', f => writeFile(f.file, JSON.stringify({ ...f.input, bound: { ...f.input.bound, executionDirectory: f.base } }), { mode: 0o600 })],
    ['credential is the capability', f => writeFile(f.file, JSON.stringify({ ...f.input, credential_file: f.input.capability.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE }), { mode: 0o600 })],
    ['not JSON', f => writeFile(f.file, '{', { mode: 0o600 })],
  ];
  for (const [name, change] of cases) {
    const f = await fixture(t);
    await change(f);
    const child = spawn(process.execPath, [entry, f.directory, f.worktree], { env: {}, stdio: ['pipe', 'pipe', 'pipe'] });
    let out = '', err = '';
    child.stdout.on('data', d => { out += d; });
    child.stderr.on('data', d => { err += d; });
    const [code] = await once(child, 'exit');
    assert.equal(code, 1, name);
    assert.equal(out, '', name);
    assert.equal(err.trim(), 'runner-check-input-invalid', name);
    assert.ok(!(await exists(f.marker)) && !(await exists(join(f.directory, 'runtime.sqlite'))), name);
  }
});

test('packaged entry admits nothing without a lease; observation stays read-only', async t => {
  const f = await fixture(t), p = start(t, f);
  await p.next(x => x.event === 'owned');
  p.send(request(f.launch));
  assert.equal((await reply(p)).error.code, 'assignment-lease-denied');
  p.send(request(f.launch, 'observe'));
  assert.deepEqual((await reply(p)).admission, { state: 'unknown' });
  assert.ok(!(await exists(f.marker)), 'bootstrap must not run without a lease');
  assert.ok(!(await exists(join(f.directory, 'runtime.sqlite'))));
  noSecrets(p);
});

test('positive control: the same fixture with only the provider substituted bootstraps, admits and settles', async t => {
  const f = await fixture(t), p = start(t, f, fileURLToPath(new URL('./faux-check-entry.mjs', import.meta.url)));
  await p.next(x => x.event === 'owned');
  await authorize(p, f.launch);
  p.send(request(f.launch));
  await p.next(x => x.event === 'bootstrapped');
  const response = await reply(p);
  assert.equal(response.admission.state, 'accepted');
  assert.equal((await p.next(x => x.event === 'settled')).status, 'done');
  assert.ok(await exists(f.marker));
  assert.ok(await exists(join(f.directory, 'runtime.sqlite')));
  // Exact replay is read-only and returns the same admission.
  p.send(request(f.launch));
  assert.deepEqual((await reply(p)).admission, response.admission);
  noSecrets(p);
});

for (const [name, options, called] of [
  ['failed bootstrap', { bootstrap: 'fail' }, true],
  ['missing model authentication', { key: false }, true],
  ['incompatible profile', { tools: ['read', 'shell'] }, true],
]) {
  test(`packaged entry rejects admission on ${name}, before any model, tool or store`, async t => {
    const f = await fixture(t, options), p = start(t, f);
    await p.next(x => x.event === 'owned');
    await authorize(p, f.launch);
    p.send(request(f.launch));
    const response = await reply(p);
    assert.deepEqual(response.admission, { state: 'rejected', code: 'assignment-runtime-failed' });
    assert.equal(response.operation_id, f.launch.operation_id);
    await p.next(x => x.event === 'runtime-failed');
    assert.ok(!p.seen(x => x.event === 'bootstrapped'));
    assert.equal(await exists(f.marker), called);
    assert.ok(!(await exists(join(f.directory, 'runtime.sqlite'))), 'no runtime store before a complete runtime');
    // The failure fences this incarnation: a retry cannot re-run bootstrap.
    await rm(f.marker, { force: true });
    p.send(request(f.launch));
    assert.equal((await reply(p)).error.code, 'assignment-lease-denied');
    assert.ok(!(await exists(f.marker)));
    noSecrets(p);
  });
}
