import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createInterface } from 'node:readline';
import { mkdtemp, mkdir, rm, readFile, writeFile, stat, symlink, chmod } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { DatabaseSync } from 'node:sqlite';
import { compatibility, validateRequest } from '../src/contract.ts';
import { snapshot, readStorage } from '../src/storage.ts';

const root = fileURLToPath(new URL('../../', import.meta.url));
const host = resolve(root, 'target/debug/forge-runner-host');
const main = fileURLToPath(new URL('../src/main.ts', import.meta.url));
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
    child, exited, stderr: () => stderr,
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
  p.send(request(id)); await response(p); await settled(p);
  const conflicting = { ...id, attempt_id: 'different' };
  p.send(request(conflicting));
  assert.equal((await response(p)).error.code, 'runner-operation-conflict');
  p.send(request(launch('other'), 'observe'));
  assert.equal((await response(p)).error.code, 'runner-identity-conflict');
  const unimplemented = request(id); unimplemented.operation = { kind: 'recover', incarnation_id: id.incarnation_id, authorization_id: 'not-authorized' };
  p.send(unimplemented);
  assert.equal((await response(p)).error.code, 'runner-operation-unavailable');
});

test('SIGKILL retains supervisor ownership; restart only observes same accepted identity/content', async t => {
  const f = await fixture(t), p = start(t, f);
  const evidence = (await owned(p)).identity;
  const id = launch(); p.send(request(id));
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
  a.send(request(launch('a'))); await response(a);
  process.kill(first.identity.worker_pid, 'SIGKILL');
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
  const id = launch(); p.send(request(id)); await response(p); await settled(p);
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
