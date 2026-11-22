// T04 controlled synthetic runner TEST FIXTURE. Never packaged (src/ only is shipped):
// no real provider, bootstrap, profile or tools. The production entry is src/check-entry.ts.
import { createInterface } from 'node:readline';
import { readlinkSync, lstatSync } from 'node:fs';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context, withAbortSignal } from '@earendil-works/chord/context';
import { createModels, fauxProvider, fauxAssistantMessage, fauxText } from '@earendil-works/pi-ai';
import { Harness, createRegistry } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { canonical, compatibility, Denied, exactKeys, requireCondition, validateRequest, type Launch } from '../src/contract.ts';
import { Authorization, guardProvider, type Owner } from '../src/authorization.ts';
import { ownerIdentity } from '../src/owner.ts';
import { inspectRecoveryCandidate } from '../src/recovery.ts';
import { databasePath, journal, observe, persistIntent, persistRoot, validateDirectory } from '../src/storage.ts';

process.umask(0o077);
const [directory, worktree] = process.argv.slice(2);
const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
let harness: Harness | undefined;
let authorization: Authorization | undefined;
let authorizedLaunch: Launch | undefined;
let cancelled = false;
let authorizationFenced = false;
let recoveryPreparation: string | undefined;
let recoveryDelivered = false;
// Trusted supervisor cancellation: close admission and abort active guarded
// execution. The independent host bounds shutdown and verifies descendants.
process.on('SIGTERM', () => { cancelled = true; authorization?.close(); });

function watchdogBlocked(): boolean {
  try { lstatSync(join(directory, 'watchdog.json')); return true; }
  catch (error) { return (error as NodeJS.ErrnoException).code !== 'ENOENT'; }
}

function currentOwner(): Owner {
  const evidence = ownerIdentity(directory);
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
  return owner;
}

// Preparation is not authority. The host first verifies original quiescence and
// the new owner, then forwards a fresh expectation on this private channel. This
// consumer independently checks original pending input/effects read-only. It does
// not bootstrap, open Harness, resubmit input or resume any execution here.
async function prepareRecovery(control: unknown): Promise<void> {
  try {
    exactKeys(control, ['host_prepare_recovery', 'candidate', 'grant']);
    requireCondition(control.host_prepare_recovery === 1 && !cancelled && !authorizationFenced && !watchdogBlocked(), 'assignment-lease-denied');
    if (recoveryPreparation !== undefined) {
      requireCondition(!recoveryDelivered && canonical(control) === recoveryPreparation, 'assignment-lease-denied');
      return;
    }
    requireCondition(!authorization, 'assignment-lease-denied');
    exactKeys(control.candidate, ['launch', 'admission_id', 'attempt', 'execution_directory', 'last_lease']);
    const { candidate, grant } = control;
    requireCondition(candidate.execution_directory === worktree && grant?.lease?.generation === candidate.attempt?.generation, 'assignment-lease-denied');
    requireCondition(canonical(candidate.launch) === canonical(grant.lease.launch) && canonical(candidate.last_lease?.launch) === canonical(candidate.launch), 'assignment-lease-denied');
    requireCondition(Number.isSafeInteger(candidate.last_lease.sequence) && grant.lease.sequence === candidate.last_lease.sequence + 1 && candidate.last_lease.daemon_epoch !== grant.lease.daemon_epoch, 'assignment-lease-denied');
    const identity = canonical(control);
    await inspectRecoveryCandidate(directory, candidate.launch, candidate.admission_id);
    requireCondition(!cancelled && !authorizationFenced && !watchdogBlocked(), 'assignment-lease-denied');
    authorization = Authorization.forRecovery(candidate.launch, candidate.attempt.generation, currentOwner(), grant.lease.daemon_epoch, grant);
    authorizedLaunch = structuredClone(candidate.launch);
    recoveryPreparation = identity;
    console.log(JSON.stringify({ event: 'recovery-prepared' }));
  } catch {
    authorizationFenced = true;
    authorization?.close();
    throw new Denied('assignment-lease-denied');
  }
}

function acceptRecovery(control: unknown): void {
  try {
    exactKeys(control, ['host_recovery', 'grant']);
    requireCondition(control.host_recovery === 1 && recoveryPreparation && authorization && !cancelled && !authorizationFenced && !watchdogBlocked(), 'assignment-lease-denied');
    authorization.acceptRecovery(control.grant);
    recoveryDelivered = true;
    console.log(JSON.stringify({ event: 'authorized', sequence: control.grant.lease.sequence }));
  } catch {
    authorizationFenced = true;
    authorization?.close();
    throw new Denied('assignment-lease-denied');
  }
}

// Only the directly spawned supervisor's protected input pipe delivers this
// control. Never load a persisted grant, expose a tool or accept an agent RPC.
function acceptAuthorization(control: unknown): void {
  try {
    exactKeys(control, ['host_control', 'lease']);
    requireCondition(control.host_control === 1 && !cancelled && !authorizationFenced && !watchdogBlocked(), 'assignment-lease-denied');
    const lease = control.lease;
    requireCondition(lease && typeof lease === 'object', 'assignment-lease-denied');
    if (!authorization) {
      const owner = currentOwner();
      authorization = new Authorization(lease.launch, lease.generation, owner, lease.daemon_epoch);
      authorizedLaunch = structuredClone(lease.launch);
    }
    authorization.accept(lease);
    console.log(JSON.stringify({ event: 'authorized', sequence: lease.sequence }));
  } catch {
    authorizationFenced = true;
    authorization?.close();
    throw new Denied('assignment-lease-denied');
  }
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
      console.log(JSON.stringify({ event: 'owned', identity: ownerIdentity(directory), compatibility: compatibility() }));
      continue;
    }
    try {
      const request: unknown = JSON.parse(line);
      if (request && typeof request === 'object' && 'host_prepare_recovery' in request) {
        await prepareRecovery(request);
        continue;
      }
      if (request && typeof request === 'object' && 'host_recovery' in request) {
        acceptRecovery(request);
        continue;
      }
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
