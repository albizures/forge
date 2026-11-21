// Real model-process crash/reopen; Forge authorization/time are protocol fakes.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { once } from 'node:events';
import { spawn } from 'node:child_process';
import { mkdtemp, readFile, mkdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { createRegistry } from '@earendil-works/pi-durable';
import { createModels, fauxProvider, fauxAssistantMessage } from '@earendil-works/pi-ai';
import { Authorization, guardProvider } from '../src/authorization.ts';
import { componentHash } from '../src/contract.ts';
import { resumeAuthorizedRuntime } from '../src/resume-runtime.ts';
import { readStorage } from '../src/storage.ts';

async function crashed(t, safe = true) {
  const root = await mkdtemp(join(tmpdir(), 'forge-writable-recovery-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const directory = join(root, 'runtime'), worktree = join(root, 'worktree');
  await mkdir(directory, { mode: 0o700 }); await mkdir(worktree);
  const launch = { ...JSON.parse(await readFile(new URL('./wire-v1.json', import.meta.url))).operation.launch, component_hash: componentHash() };
  const child = spawn(process.execPath, [new URL('./recovery-worker.mjs', import.meta.url).pathname, directory, worktree, safe ? 'safe' : 'model']);
  let output = '', errors = '';
  const exit = once(child, 'exit');
  t.after(async () => { if (child.exitCode === null && child.signalCode === null) { child.kill('SIGKILL'); await exit; } });
  child.stderr.on('data', d => { errors += d; });
  await Promise.race([
    new Promise(resolve => child.stdout.on('data', d => { output += d; if (output.includes('model-entered\n')) resolve(); })),
    exit.then(() => { throw Error(`fixture exited: ${errors}`); }),
  ]);
  child.kill('SIGKILL'); await exit;
  return { directory, worktree, launch, ...JSON.parse(output.split('\n')[0]) };
}
function authority(f, mode = 'recovery') {
  const original = JSON.parse(fixtureLease);
  const grant = { claim: { operation_id: 'recover', daemon_epoch: 'new-daemon', launch: f.launch, candidate_sha256: 'a'.repeat(64) },
    lease: { ...original, launch: f.launch, daemon_epoch: 'new-daemon', sequence: 2, issued_boottime_millis: 1000, expires_boottime_millis: 31000 }, evidence_sha256: 'b'.repeat(64) };
  let now = 1000;
  const auth = mode === 'ordinary'
    ? new Authorization(f.launch, grant.lease.generation, grant.lease.owner, 'new-daemon', () => now)
    : Authorization.forRecovery(f.launch, grant.lease.generation, grant.lease.owner, 'new-daemon', grant, () => now);
  if (mode === 'ordinary') auth.accept({ ...grant.lease, sequence: 1 });
  else if (mode !== 'unarmed') auth.acceptRecovery(grant);
  return { auth, generation: grant.lease.generation, expire: () => { now = 31000; } };
}
const fixtureLease = await readFile(new URL('./authorization-lease.json', import.meta.url), 'utf8');

function runtime(auth, count) {
  const provider = fauxProvider();
  provider.setResponses([() => { count.calls++; return fauxAssistantMessage('Recovered original input, not a Forge Outcome.'); }]);
  const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
  models.setProvider(guardProvider(provider.provider, auth));
  return { models, registry: createRegistry(), settings: { retry: { enabled: false }, compaction: { enabled: false } }, env: () => undefined };
}

test('authorized writable recovery resumes the exact original admission/conversation without new input', { timeout: 15000 }, async t => {
  const f = await crashed(t), a = authority(f), count = { calls: 0 };
  t.after(() => a.auth.close());
  let preparations = 0;
  const recovered = await resumeAuthorizedRuntime(f.directory, f.worktree, f.launch, a.generation, f.admission_id, a.auth,
    async () => { preparations++; return runtime(a.auth, count); });
  t.after(() => recovered.harness.close(context));
  const submission = await recovered.harness.submission(Number(f.admission_id), context);
  const settled = await submission.wait(context);
  assert.equal(settled.status, 'done', JSON.stringify(settled));
  assert.equal(recovered.root, f.root); assert.equal(recovered.admission_id, f.admission_id);
  assert.equal(preparations, 1); assert.equal(count.calls, 1);
  await recovered.harness.close(context);
  const storage = await readStorage(f.directory);
  try {
    assert.equal((await storage.scanConversations({}, 100, undefined, context)).items.length, 1);
    const page = await storage.scanSubmissions({}, 100, undefined, context);
    assert.equal(page.items.length, 1); assert.equal(String(page.items[0].id), f.admission_id);
  } finally { await storage.close(context); }
});

test('interrupted in-flight model remains unanswered with retries disabled, never a synthetic continuation', { timeout: 15000 }, async t => {
  const f = await crashed(t, false), a = authority(f), count = { calls: 0 };
  t.after(() => a.auth.close());
  const recovered = await resumeAuthorizedRuntime(f.directory, f.worktree, f.launch, a.generation, f.admission_id, a.auth, async () => runtime(a.auth, count));
  t.after(() => recovered.harness.close(context));
  const submission = await recovered.harness.submission(Number(f.admission_id), context);
  const settled = await submission.wait(context);
  assert.equal(settled.status, 'unanswered'); assert.equal(settled.reason, 'interrupted');
  assert.equal(count.calls, 0);
});

for (const mode of ['ordinary', 'unarmed', 'expired', 'bootstrap-failed', 'wrong-worktree']) test(`writable recovery denies ${mode} before scheduling`, { timeout: 15000 }, async t => {
  const f = await crashed(t), a = authority(f, mode), count = { calls: 0 };
  t.after(() => a.auth.close());
  if (mode === 'expired') a.expire();
  let preparation = 0;
  await assert.rejects(resumeAuthorizedRuntime(f.directory, mode === 'wrong-worktree' ? f.directory : f.worktree, f.launch, a.generation, f.admission_id, a.auth,
    async () => { preparation++; if (mode === 'bootstrap-failed') throw Error('PRIVATE-CANARY'); return runtime(a.auth, count); }),
    { code: 'runner-recovery-failed' });
  assert.equal(count.calls, 0);
  assert.equal(preparation, mode === 'bootstrap-failed' ? 1 : 0);
});
