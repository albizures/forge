// T04 controlled synthetic runner. No real providers, tools or production launch.
import { createInterface } from 'node:readline';
import { readFileSync, statSync, fstatSync, readlinkSync, lstatSync } from 'node:fs';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context, withAbortSignal } from '@earendil-works/chord/context';
import { createModels, fauxProvider, fauxAssistantMessage, fauxText } from '@earendil-works/pi-ai';
import { Harness, createRegistry } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { canonical, compatibility, Denied, exactKeys, requireCondition, validateRequest, type Launch } from './contract.ts';
import { Authorization, guardProvider, type Owner } from './authorization.ts';
import { databasePath, journal, observe, persistIntent, persistRoot, validateDirectory } from './storage.ts';

process.umask(0o077);
const [directory, worktree] = process.argv.slice(2);
const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
let harness: Harness | undefined;
let authorization: Authorization | undefined;
let authorizedLaunch: Launch | undefined;

function watchdogBlocked(): boolean {
  try { lstatSync(join(directory, 'watchdog.json')); return true; }
  catch (error) { return (error as NodeJS.ErrnoException).code !== 'ENOENT'; }
}

// Only the directly spawned supervisor's protected input pipe delivers this
// control. Never load a persisted grant, expose a tool or accept an agent RPC.
function acceptAuthorization(control: unknown): void {
  try {
    exactKeys(control, ['host_control', 'lease']);
    requireCondition(control.host_control === 1 && !watchdogBlocked(), 'assignment-lease-denied');
    const lease = control.lease;
    requireCondition(lease && typeof lease === 'object', 'assignment-lease-denied');
    if (!authorization) {
      const evidence = ownerIdentity();
      const owner: Owner = {
        boot_id: evidence.owner.boot_id,
        time_namespace: readlinkSync('/proc/self/ns/time'),
        host_pid: evidence.owner.host_pid,
        host_start: evidence.owner.host_start,
        worker_pid: process.pid,
        worker_start: evidence.worker_start,
        lock_device: evidence.owner.lock_device,
        lock_inode: evidence.owner.lock_inode,
      };
      requireCondition(readlinkSync(`/proc/${process.ppid}/ns/time`) === owner.time_namespace, 'assignment-lease-denied');
      authorization = new Authorization(lease.launch, lease.generation, owner, lease.daemon_epoch);
      authorizedLaunch = structuredClone(lease.launch);
    }
    authorization.accept(lease);
    console.log(JSON.stringify({ event: 'authorized', sequence: lease.sequence }));
  } catch {
    authorization?.close();
    throw new Denied('assignment-lease-denied');
  }
}

function processStart(pid: number): string {
  return readFileSync(`/proc/${pid}/stat`, 'utf8').split(/\) (?=[RSDZTtXxKWPI] )/).at(-1)!.split(' ')[19];
}
function ownerIdentity() {
  const owner = JSON.parse(process.env.FORGE_RUNNER_OWNER ?? 'null');
  requireCondition(owner && owner.host_pid === process.ppid, 'runner-owner-parent-invalid');
  requireCondition(owner.host_start === processStart(process.ppid), 'runner-owner-start-invalid');
  requireCondition(owner.boot_id === readFileSync('/proc/sys/kernel/random/boot_id', 'utf8').trim(), 'runner-owner-boot-invalid');
  requireCondition(Number.isInteger(owner.lock_fd) && owner.lock_fd >= 3, 'runner-owner-descriptor-invalid');
  const inherited = fstatSync(owner.lock_fd, { bigint: true });
  const lock = statSync(join(directory, 'owner.lock'), { bigint: true });
  requireCondition(inherited.dev === lock.dev && inherited.ino === lock.ino, 'runner-owner-descriptor-invalid');
  requireCondition(owner.lock_device === String(lock.dev) && owner.lock_inode === String(lock.ino), 'runner-owner-invalid');
  const evidence = JSON.parse(readFileSync(join(directory, 'owner.json'), 'utf8'));
  requireCondition(canonical(evidence.owner) === canonical(owner), 'runner-owner-evidence-invalid');
  requireCondition(evidence.worker_pid === process.pid && evidence.worker_start === processStart(process.pid) && evidence.process_group === process.pid, 'runner-owner-worker-invalid');
  return { ...evidence, worker_start: processStart(process.pid) };
}
async function admit(launch: Launch) {
  const prior = journal(directory);
  if (prior) {
    requireCondition(canonical(prior.launch) === canonical(launch), 'runner-operation-conflict');
    // Exact replay is read-only. Reopening writable storage could repair tasks;
    // that requires T05/T08/T10 authorization, never an ordinary retry.
    return observe(directory, launch.assignment_id, launch.runtime_id, launch.operation_id);
  }
  requireCondition(authorization && canonical(authorizedLaunch) === canonical(launch) && !watchdogBlocked(), 'assignment-lease-denied');
  authorization.admit();
  const executionContext = withAbortSignal(authorization.signal, context);
  persistIntent(directory, launch);
  const provider = fauxProvider();
  provider.setResponses([fauxAssistantMessage([fauxText('Forge T04 synthetic completion; not an Outcome.')])]);
  const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
  models.setProvider(guardProvider(provider.provider, authorization));
  const storage = await openNodeSqliteStorage(databasePath(directory));
  authorization.admit();
  harness = await Harness.open(storage, { models, registry: createRegistry(), settings: { retry: { enabled: false }, compaction: { enabled: false } }, env: () => undefined }, executionContext);
  authorization.admit();
  const root = await harness.root(executionContext, { agent: { model: { provider: 'faux', modelId: 'faux-1' }, cwd: worktree } });
  persistRoot(directory, root.id);
  authorization.admit();
  const submission = await root.submit({ type: 'input', requestId: launch.operation_id, content: 'Forge T04 synthetic input.' }, executionContext);
  void submission.wait(context).then(
    () => console.log(JSON.stringify({ event: 'settled', admission_id: String(submission.id) })),
    () => console.log(JSON.stringify({ event: 'execution-failed' })),
  );
  return { state: 'accepted', admission_id: String(submission.id) };
}
try {
  validateDirectory(directory, worktree);
  let hostReady = false;
  for await (const line of lines) {
    if (!hostReady) {
      requireCondition(line === '{"host_ready":true}', 'runner-owner-invalid');
      hostReady = true;
      console.log(JSON.stringify({ event: 'owned', identity: ownerIdentity(), compatibility: compatibility() }));
      continue;
    }
    try {
      const request: unknown = JSON.parse(line);
      if (request && typeof request === 'object' && 'host_control' in request) {
        acceptAuthorization(request);
        continue;
      }
      validateRequest(request);
      const retained = journal(directory);
      if (retained) requireCondition(retained.launch.assignment_id === request.assignment_id && retained.launch.runtime_id === request.runtime_id, 'runner-identity-conflict');
      const admission = request.operation.kind === 'admit' ? await admit(request.operation.launch) : await observe(directory, request.assignment_id, request.runtime_id, request.operation.admission_operation_id);
      console.log(JSON.stringify({ compatibility: compatibility(), assignment_id: request.assignment_id, runtime_id: request.runtime_id, operation_id: request.operation_id, admission }));
    } catch (error) {
      // No input, paths, credentials or upstream errors in protocol diagnostics.
      console.log(JSON.stringify({ error: { code: error instanceof Denied ? error.code : 'runner-request-failed' } }));
    }
  }
  if (harness) await harness.close(context);
} catch (error) {
  console.error(error instanceof Denied ? error.code : 'runner-start-failed');
  process.exitCode = 1;
} finally {
  authorization?.close();
  lines.close();
}
