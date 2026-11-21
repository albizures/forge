import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createInterface } from 'node:readline';
import { mkdtemp, mkdir, rm, readFile, writeFile, stat, symlink, chmod, readlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { DatabaseSync } from 'node:sqlite';
import { compatibility, validateRequest } from '../src/contract.ts';
import { snapshot, readStorage } from '../src/storage.ts';

const root = fileURLToPath(new URL('../../', import.meta.url));
const host = resolve(root, 'target/debug/forge-runner-host');
const main = fileURLToPath(new URL('./synthetic-runner.ts', import.meta.url));
const inspect = fileURLToPath(new URL('../src/inspect.ts', import.meta.url));
async function fixture(t) {
  const base = await mkdtemp(join(tmpdir(), 'forge-runner-'));
  const directory = join(base, 'runtime');
  const worktree = join(base, 'worktree');
  await mkdir(directory, { mode: 0o700 });
  await mkdir(worktree, { mode: 0o700 });
  const cleanups = [];
  t.after(async () => {
    for (const cleanup of cleanups.reverse()) await cleanup();
    await rm(base, { recursive: true, force: true });
  });
  return { directory, worktree, base, cleanups };
}
function launch(id = 'a') {
  return { assignment_id: id, operation_id: 'launch-' + id, attempt_id: 'attempt-' + id, profile_revision: 1, runtime_id: 'runtime-' + id, incarnation_id: 'incarnation-' + id, conversation_id: 'conversation-' + id, store_id: 'store-' + id, forge_revision: 'fixture', component_hash: compatibility().component_hash, runtime_schema: 2 };
}
function request(launch, kind = 'admit') {
  return { compatibility: compatibility(), assignment_id: launch.assignment_id, runtime_id: launch.runtime_id, operation_id: kind === 'admit' ? launch.operation_id : 'observe', operation: kind === 'admit' ? { kind, launch } : { kind, admission_operation_id: launch.operation_id } };
}
function start(t, f) {
  const child = spawn(host, [f.directory, f.worktree, process.execPath, main], { stdio: ['pipe', 'pipe', 'pipe'] });
  let stderr = '';
  const diagnostics = [];
  child.stderr.on('data', data => {
    stderr += data;
    for (const waiter of diagnostics) if (waiter.match.test(stderr)) waiter.resolve();
  });
  const queue = [];
  const pending = [];
  const lines = createInterface({ input: child.stdout });
  let ownedIdentity;
  lines.on('line', line => {
    const value = JSON.parse(line);
    if (value.event === 'owned') ownedIdentity = value.identity;
    const index = pending.findIndex(p => p.match(value));
    if (index < 0) queue.push(value);
    else pending.splice(index, 1)[0].resolve(value);
  });
  const exited = once(child, 'exit');
  child.on('exit', () => { for (const p of pending.splice(0)) p.reject(new Error(stderr)); });
  f.cleanups.push(async () => {
    // Test-only controller terminates only processes whose direct spawn/evidence
    // it owns. This is NOT the production retained-resource retirement policy.
    const evidence = ownedIdentity;
    if (evidence?.owner.host_pid === child.pid) {
      const stat = await readFile(`/proc/${evidence.worker_pid}/stat`, 'utf8').catch(() => '');
      const start = stat.slice(stat.lastIndexOf(')') + 2).split(' ')[19];
      if (start === evidence.worker_start) {
        try { process.kill(evidence.worker_pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
      }
    }
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
    await exited;
    lines.close();
  });
  return {
    child, exited, stderr: () => stderr, identity: () => ownedIdentity,
    diagnostic(match) {
      if (match.test(stderr)) return Promise.resolve();
      return new Promise(resolve => diagnostics.push({ match, resolve }));
    },
    next(match) {
      const index = queue.findIndex(match);
      if (index >= 0) return Promise.resolve(queue.splice(index, 1)[0]);
      return new Promise((resolve, reject) => pending.push({ match, resolve, reject }));
    },
    send(value) { child.stdin.write(JSON.stringify(value) + '\n'); },
  };
}
const owned = p => p.next(x => x.event === 'owned');
const response = p => p.next(x => x.admission || x.error);
const settled = p => p.next(x => x.event === 'settled');
async function authorizationGrant(p, launch, lifetime = 30000) {
  const evidence = p.identity();
  const now = Math.floor(Number((await readFile('/proc/uptime', 'utf8')).split(' ')[0]) * 1000);
  const lease = {
    launch, generation: 1,
    owner: {
      boot_id: evidence.owner.boot_id, time_namespace: await readlink('/proc/self/ns/time'),
      host_pid: evidence.owner.host_pid, host_start: evidence.owner.host_start,
      worker_pid: evidence.worker_pid, worker_start: evidence.worker_start,
      lock_device: evidence.owner.lock_device, lock_inode: evidence.owner.lock_inode,
    },
    daemon_epoch: 'controlled-test-daemon', sequence: 1,
    issued_boottime_millis: now - (30000 - lifetime), expires_boottime_millis: now + lifetime,
  };
  return lease;
}
async function authorize(p, launch, lifetime = 30000) {
  const lease = await authorizationGrant(p, launch, lifetime);
  p.send({ host_control: 1, lease });
  assert.equal((await p.next(x => x.event === 'authorized')).sequence, 1);
  return lease;
}
async function runInspect(f) {
  const child = spawn(process.execPath, [inspect, f.directory, f.worktree], { env: {}, stdio: ['ignore', 'pipe', 'pipe'] });
  let out = '', err = '';
  child.stdout.on('data', d => { out += d; });
  child.stderr.on('data', d => { err += d; });
  const [code] = await once(child, 'exit');
  assert.equal(code, 0, err);
  return JSON.parse(out);
}

test('shared Rust/TypeScript v1 fixture and strict compatibility checks', async () => {
  const fixture = JSON.parse(await readFile(new URL('./wire-v1.json', import.meta.url), 'utf8'));
  fixture.compatibility = compatibility();
  fixture.operation.launch.component_hash = fixture.compatibility.component_hash;
  validateRequest(fixture);
  for (const [key, value, code] of [['protocol', 2, 'runner-protocol-incompatible'], ['runtime_schema', 3, 'runner-schema-incompatible'], ['component_hash', 'other', 'runner-component-incompatible']]) {
    const copy = structuredClone(fixture); copy.compatibility[key] = value;
    assert.throws(() => validateRequest(copy), { code });
  }
  const extra = structuredClone(fixture); extra.operation.launch.unknown = true;
  assert.throws(() => validateRequest(extra), { code: 'runner-request-invalid' });
});

test('identity-bound retirement cancels the actual runner, retains history and fences a fresh host', async t => {
  const f = await fixture(t), p = start(t, f);
  await owned(p);
  const id = launch();
  const lease = await authorize(p, id);
  p.send(request(id));
  assert.equal((await response(p)).admission.state, 'accepted');
  await settled(p);
  const before = await runInspect(f);
  const operation = { operation_id: 'retire-a', launch: id, owner: lease.owner };
  p.send({ host_retire: 1, operation });
  assert.equal((await p.exited)[0], 0, p.stderr());
  const receipt = JSON.parse(await readFile(join(f.directory, 'retirement-receipt.json'), 'utf8'));
  assert.deepEqual(receipt, { version: 1, operation, children_absent: true });
  assert.equal((await stat(join(f.directory, 'retirement-receipt.json'))).mode & 0o777, 0o600);
  assert.deepEqual(await runInspect(f), before);
  const restarted = start(t, f);
  await owned(restarted);
  restarted.send({ host_control: 1, lease: await authorizationGrant(restarted, id) });
  await restarted.diagnostic(/runner-authorization-denied/);
  assert.deepEqual(await runInspect(f), before);
});

test('retirement with mismatched owner or launch cannot cancel or signal a lookalike', async t => {
  for (const mismatch of ['owner', 'launch']) {
    const f = await fixture(t), p = start(t, f);
    const evidence = (await owned(p)).identity;
    const id = launch();
    const lease = await authorize(p, id);
    const operation = { operation_id: 'retire-a', launch: structuredClone(id), owner: structuredClone(lease.owner) };
    if (mismatch === 'owner') operation.owner.worker_start = 'not-the-owned-worker';
    else operation.launch.attempt_id = 'different-attempt';
    p.send({ host_retire: 1, operation });
    await p.diagnostic(/runner-retirement-unverified/);
    const current = await readFile(`/proc/${evidence.worker_pid}/stat`, 'utf8');
    assert.equal(current.slice(current.lastIndexOf(')') + 2).split(' ')[19], evidence.worker_start);
    await assert.rejects(readFile(join(f.directory, 'retirement-intent.json')), { code: 'ENOENT' });
    await assert.rejects(readFile(join(f.directory, 'retirement-receipt.json')), { code: 'ENOENT' });
  }
});

test('new execution requires a matching lease before any writable store open', async t => {
  const f = await fixture(t), p = start(t, f);
  await owned(p);
  const id = launch();
  p.send(request(id));
  assert.equal((await response(p)).error.code, 'assignment-lease-denied');
  await assert.rejects(readFile(join(f.directory, 'runtime.sqlite')), { code: 'ENOENT' });
  await authorize(p, id);
  p.send(request({ ...id, attempt_id: 'not-authorized' }));
  assert.equal((await response(p)).error.code, 'assignment-lease-denied');
  await assert.rejects(readFile(join(f.directory, 'runtime.sqlite')), { code: 'ENOENT' });
  p.send(request(id));
  assert.equal((await response(p)).admission.state, 'accepted');
  await settled(p);
});

test('independent watchdog expires with stopped JavaScript, escalates, and preserves read-only restart', async t => {
  const f = await fixture(t), p = start(t, f);
  const evidence = (await owned(p)).identity;
  const id = launch();
  await authorize(p, id, 1000);
  p.send(request(id)); const accepted = await response(p); await settled(p);
  const before = await runInspect(f);
  process.kill(evidence.worker_pid, 'SIGSTOP');
  // Fill the worker pipe: blocked forwarding must not hold the watchdog mutex.
  p.send({ padding: 'x'.repeat(131072) });
  await p.diagnostic(/runner-lease-expired/);
  const marker = await readFile(join(f.directory, 'watchdog.json'));
  assert.equal(JSON.parse(marker).cleanup, 'unverified');
  assert.equal((await stat(join(f.directory, 'watchdog.json'))).mode & 0o777, 0o600);
  await p.diagnostic(/runner-watchdog-kill-sent/);
  for (const stage of ['term-sent', 'kill-sent']) {
    const path = join(f.directory, `watchdog-${evidence.owner.host_pid}-${evidence.owner.host_start}-${stage}.json`);
    const retained = JSON.parse(await readFile(path, 'utf8'));
    assert.equal(retained.code, stage);
    assert.equal(retained.cleanup, 'unverified');
    assert.equal((await stat(path)).mode & 0o777, 0o600);
  }
  await p.diagnostic(/runner-worker-exited/);
  const contender = start(t, f);
  assert.equal((await contender.exited)[0], 1);
  assert.match(contender.stderr(), /runner-owner-busy/);
  assert.deepEqual(await runInspect(f), before);
  p.child.kill('SIGKILL'); await p.exited;
  const restarted = start(t, f); await owned(restarted);
  restarted.send(request(id));
  assert.deepEqual((await response(restarted)).admission, accepted.admission);
  restarted.send({ host_control: 1, lease: await authorizationGrant(restarted, id) });
  await restarted.diagnostic(/runner-authorization-denied/);
  assert.deepEqual(await readFile(join(f.directory, 'watchdog.json')), marker);
  assert.deepEqual(await runInspect(f), before);
});

test('owner uncertainty refuses signals and retains diagnostics rather than inferring exit', async t => {
  const f = await fixture(t), p = start(t, f);
  const evidence = (await owned(p)).identity;
  await authorize(p, launch());
  await writeFile(join(f.directory, 'owner.json'), JSON.stringify({ ...evidence, worker_start: 'foreign' }));
  await p.diagnostic(/runner-owner-uncertain/);
  await p.diagnostic(/runner-watchdog-kill-unverified/);
  const current = await readFile(`/proc/${evidence.worker_pid}/stat`, 'utf8');
  assert.equal(current.slice(current.lastIndexOf(')') + 2).split(' ')[19], evidence.worker_start);
  assert.equal(JSON.parse(await readFile(join(f.directory, 'watchdog.json'), 'utf8')).cleanup, 'unverified');
  const contender = start(t, f);
  assert.equal((await contender.exited)[0], 1);
  assert.match(contender.stderr(), /runner-owner-busy/);
});

test('trusted pipe renewal and exact delivery retries preserve one execution', async t => {
  const f = await fixture(t), p = start(t, f);
  await owned(p);
  const id = launch();
  const first = await authorize(p, id, 1000);
  const renewal = await authorizationGrant(p, id);
  renewal.sequence = 2;
  p.send({ host_control: 1, lease: renewal });
  assert.equal((await p.next(x => x.event === 'authorized')).sequence, 2);
  // Retry while the host polling loop competes for the policy lock.
  for (let i = 0; i < 20; i++) {
    p.send({ host_control: 1, lease: renewal });
    assert.equal((await p.next(x => x.event === 'authorized')).sequence, 2);
  }
  // Synchronize on the *old* absolute boottime deadline, not on a wall clock.
  while (Math.floor(Number((await readFile('/proc/uptime', 'utf8')).split(' ')[0]) * 1000) < first.expires_boottime_millis + 100) {
    await new Promise(resolve => setTimeout(resolve, 25));
  }
  const next = await authorizationGrant(p, id); next.sequence = 3;
  p.send({ host_control: 1, lease: next });
  assert.equal((await p.next(x => x.event === 'authorized')).sequence, 3);
  await assert.rejects(readFile(join(f.directory, 'watchdog.json')), { code: 'ENOENT' });
  p.send(request(id));
  assert.equal((await response(p)).admission.state, 'accepted');
  await settled(p);
});

test('foreign/malformed controls and retained marker entries cannot grant authority or leak payloads', async t => {
  for (const fault of ['owner', 'unknown-field', 'version', 'retained-malformed', 'retained-symlink']) {
    const f = await fixture(t);
    const target = join(f.base, 'must-not-be-created');
    if (fault === 'retained-malformed') await writeFile(join(f.directory, 'watchdog.json'), 'malformed retained evidence', { mode: 0o600 });
    if (fault === 'retained-symlink') await symlink(target, join(f.directory, 'watchdog.json'));
    const p = start(t, f);
    await owned(p);
    const lease = await authorizationGrant(p, launch());
    if (fault === 'unknown-field') lease.private = 'sensitive-test-sentinel';
    if (fault === 'owner') lease.owner.worker_start = 'foreign';
    p.send({ host_control: fault === 'version' ? 2 : 1, lease });
    await p.diagnostic(/runner-authorization-denied/);
    await assert.rejects(readFile(join(f.directory, 'runtime.sqlite')), { code: 'ENOENT' });
    assert.doesNotMatch(p.stderr(), /sensitive-test-sentinel|foreign/);
    if (fault === 'retained-symlink') {
      assert.equal(await readlink(join(f.directory, 'watchdog.json')), target);
      await assert.rejects(readFile(target), { code: 'ENOENT' });
    } else {
      const marker = await readFile(join(f.directory, 'watchdog.json'), 'utf8');
      assert.doesNotMatch(marker, /sensitive-test-sentinel|foreign/);
      if (fault === 'retained-malformed') assert.equal(marker, 'malformed retained evidence');
    }
  }
});

test('concurrent starts: one supervisor, one conversation, idempotent committed submission; read-only observation beside writer', async t => {
  const f = await fixture(t);
  const a = start(t, f), b = start(t, f);
  const winner = await Promise.race([owned(a).then(value => ({ p: a, value })).catch(() => undefined), owned(b).then(value => ({ p: b, value })).catch(() => undefined)].map(async p => {
    const value = await p;
    if (!value) return new Promise(() => {});
    return value;
  }));
  const loser = winner.p === a ? b : a;
  assert.equal((await loser.exited)[0], 1);
  assert.match(loser.stderr(), /runner-owner-busy/);
  const id = launch();
  await authorize(winner.p, id);
  winner.p.send(request(id));
  const accepted = await response(winner.p);
  assert.equal(accepted.admission.state, 'accepted');
  await settled(winner.p);
  const before = await readFile(join(f.directory, 'runtime.sqlite'));
  const view = await runInspect(f);
  assert.equal(view.identity.launch.conversation_id, id.conversation_id);
  assert.match(JSON.stringify(view.entries), /synthetic completion/);
  const reader = await readStorage(f.directory);
  await assert.rejects(reader.mintId(), /reader-mutation-forbidden/);
  await reader.close({});
  winner.p.send(request(id));
  assert.deepEqual((await response(winner.p)).admission, accepted.admission);
  winner.p.send(request(id, 'observe'));
  assert.deepEqual((await response(winner.p)).admission, accepted.admission);
  assert.deepEqual(await readFile(join(f.directory, 'runtime.sqlite')), before);
  for (const name of ['runtime.sqlite', 'runtime.sqlite-wal', 'runtime.sqlite-shm', 'owner.lock', 'owner.json']) assert.equal((await stat(join(f.directory, name))).mode & 0o777, 0o600);
  const db = new DatabaseSync(join(f.directory, 'runtime.sqlite'), { readOnly: true });
  try { assert.equal(db.prepare('SELECT count(*) AS n FROM forge_runner_identity').get().n, 1); } finally { db.close(); }
});

test('unknown protocol, wrong identities and conflicting IDs reject before opening/repairing storage', async t => {
  const f = await fixture(t), p = start(t, f);
  await owned(p);
  const id = launch();
  const wrong = request(id); wrong.compatibility.protocol = 2;
  p.send(wrong);
  assert.equal((await response(p)).error.code, 'runner-protocol-incompatible');
  await assert.rejects(readFile(join(f.directory, 'runtime.sqlite')), { code: 'ENOENT' });
  await authorize(p, id);
  p.send(request(id)); await response(p); await settled(p);
  const conflicting = { ...id, attempt_id: 'different' };
  p.send(request(conflicting));
  assert.equal((await response(p)).error.code, 'runner-operation-conflict');
  p.send(request(launch('other'), 'observe'));
  assert.equal((await response(p)).error.code, 'runner-identity-conflict');
  // Protocol 1 defines no request-level recover/transcript/follow-up/retire kinds.
  const undefinedKind = request(id); undefinedKind.operation = { kind: 'recover', incarnation_id: id.incarnation_id, authorization_id: 'not-authorized' };
  p.send(undefinedKind);
  assert.equal((await response(p)).error.code, 'runner-request-invalid');
});

test('SIGKILL retains supervisor ownership; restart only observes same accepted identity/content', async t => {
  const f = await fixture(t), p = start(t, f);
  const evidence = (await owned(p)).identity;
  const id = launch(); await authorize(p, id); p.send(request(id));
  const accepted = await response(p); await settled(p);
  const before = await runInspect(f);
  process.kill(evidence.worker_pid, 'SIGKILL');
  // The worker PID is our direct authenticated handshake, not a searched label.
  const contender = start(t, f);
  assert.equal((await contender.exited)[0], 1);
  assert.match(contender.stderr(), /runner-owner-busy/);
  assert.deepEqual(await runInspect(f), before);
  p.child.kill('SIGKILL'); await p.exited;
  const inode = (await stat(join(f.directory, 'owner.lock'))).ino;
  const restarted = start(t, f); await owned(restarted);
  restarted.send(request(id));
  assert.deepEqual((await response(restarted)).admission, accepted.admission);
  assert.deepEqual(await runInspect(f), before);
  assert.equal((await stat(join(f.directory, 'owner.lock'))).ino, inode);
});

test('interrupted handshake and stale PID evidence never adopt or signal another process', async t => {
  const f = await fixture(t);
  const unrelated = spawn(process.execPath, ['-e', 'process.stdin.resume()'], { stdio: ['pipe', 'ignore', 'ignore'] });
  const unrelatedExit = once(unrelated, 'exit');
  t.after(async () => { unrelated.kill('SIGKILL'); await unrelatedExit; });
  await writeFile(join(f.directory, 'owner.lock'), 'stale pid ' + unrelated.pid, { mode: 0o600 });
  await writeFile(join(f.directory, 'owner.json'), JSON.stringify({ worker_pid: unrelated.pid }), { mode: 0o600 });
  const p = start(t, f), identity = (await owned(p)).identity;
  process.kill(identity.worker_pid, 'SIGKILL');
  assert.equal(unrelated.exitCode, null);
  await assert.rejects(readFile(join(f.directory, 'runtime.sqlite')), { code: 'ENOENT' });
  const contender = start(t, f); assert.equal((await contender.exited)[0], 1);
});

test('one Assignment crashing does not interrupt the second synthetic model run', async t => {
  const f = await fixture(t), g = await fixture(t);
  const a = start(t, f), b = start(t, g);
  const [first, second] = await Promise.all([owned(a), owned(b)]);
  assert.notEqual(first.identity.worker_pid, second.identity.worker_pid);
  await authorize(a, launch('a'));
  a.send(request(launch('a'))); await response(a);
  process.kill(first.identity.worker_pid, 'SIGKILL');
  await authorize(b, launch('b'));
  b.send(request(launch('b'))); assert.equal((await response(b)).admission.state, 'accepted');
  await settled(b);
  const view = await runInspect(g);
  assert.equal(view.identity.launch.assignment_id, 'b');
  assert.match(JSON.stringify(view.entries), /synthetic completion/);
});

test('unsafe filesystem ownership, redirected locks and worktree overlap fail closed', async t => {
  const f = await fixture(t);
  const marker = join(f.base, 'marker'); await writeFile(marker, 'unchanged');
  await symlink(marker, join(f.directory, 'owner.lock'));
  const p = start(t, f); assert.equal((await p.exited)[0], 1);
  assert.equal(await readFile(marker, 'utf8'), 'unchanged');
  const g = await fixture(t); await chmod(g.directory, 0o755);
  const q = start(t, g); assert.equal((await q.exited)[0], 1);
  const h = await fixture(t); h.worktree = h.base;
  const r = start(t, h); assert.equal((await r.exited)[0], 1);
  const dangling = await fixture(t);
  const absent = join(dangling.base, 'must-not-be-created');
  await symlink(absent, join(dangling.directory, 'runtime.sqlite'));
  const s = start(t, dangling);
  // Filesystem preflight fails without a writable open, even for a dangling link.
  await s.diagnostic(/runner-worker-exited/);
  assert.match(s.stderr(), /runner-storage-ownership-invalid/);
  // Host intentionally retains ownership after the worker's failed preflight;
  // inspect stderr rather than interpreting absent readiness as cleanup.
  await assert.rejects(readFile(absent), { code: 'ENOENT' });
});

test('read adapter rejects missing and incompatible schemas without file creation or mutation', async t => {
  const missing = await fixture(t);
  await assert.rejects(readStorage(missing.directory));
  await assert.rejects(readFile(join(missing.directory, 'runtime.sqlite')), { code: 'ENOENT' });
  for (const version of [1, 3]) {
    const f = await fixture(t);
    const path = join(f.directory, 'runtime.sqlite');
    const db = new DatabaseSync(path);
    db.exec(`CREATE TABLE durable_schema(singleton INTEGER PRIMARY KEY, version INTEGER); INSERT INTO durable_schema VALUES(1,${version})`);
    db.close(); await chmod(path, 0o600);
    const before = await readFile(path);
    await assert.rejects(readStorage(f.directory), /reader-schema-incompatible/);
    assert.deepEqual(await readFile(path), before);
  }
});

test('supervisor death cannot free ownership while its worker survives', async t => {
  const f = await fixture(t), p = start(t, f);
  await owned(p);
  const id = launch(); await authorize(p, id); p.send(request(id)); await response(p); await settled(p);
  // Hold the actual worker alive independently of EOF/normal JS shutdown.
  process.kill(p.identity().worker_pid, 'SIGSTOP');
  p.child.kill('SIGKILL'); await p.exited;
  // The worker still owns an inherited descriptor for the same flock; no
  // writable contender is permitted even though the supervisor has died.
  const contender = start(t, f);
  assert.equal((await contender.exited)[0], 1);
  assert.match(contender.stderr(), /runner-owner-busy/);
  assert.match(JSON.stringify((await runInspect(f)).entries), /synthetic completion/);
});

test('partial admission intent and incompatible runtime schemas remain uncertain; inspection never initializes', async t => {
  const f = await fixture(t);
  const p = start(t, f); await owned(p);
  // Simulates a crash after the runtime intent commit, before Harness creation.
  const id = launch();
  const db = new DatabaseSync(join(f.directory, 'runtime.sqlite'));
  db.exec('CREATE TABLE forge_runner_identity(singleton INTEGER PRIMARY KEY, version INTEGER, launch TEXT, root INTEGER)');
  db.prepare('INSERT INTO forge_runner_identity VALUES(1,1,?,NULL)').run(JSON.stringify(id)); db.close();
  await chmod(join(f.directory, 'runtime.sqlite'), 0o600);
  const before = await readFile(join(f.directory, 'runtime.sqlite'));
  p.send(request(id)); assert.equal((await response(p)).admission.state, 'unknown');
  assert.deepEqual((await snapshot(f.directory)).entries, []);
  assert.deepEqual(await readFile(join(f.directory, 'runtime.sqlite')), before);
  await assert.rejects(readStorage(f.directory));
  assert.deepEqual(await readFile(join(f.directory, 'runtime.sqlite')), before);
});
