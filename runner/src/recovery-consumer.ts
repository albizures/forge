// Private executable recovery channel, not a public runner/agent operation.
import { createInterface } from 'node:readline';
import { lstatSync, readlinkSync } from 'node:fs';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import type { Harness, HarnessOptions, SubmissionId } from '@earendil-works/pi-durable';
import { Authorization, type Owner } from './authorization.ts';
import { canonical, compatibility, Denied, exactKeys, requireCondition, validateLaunch, type Launch } from './contract.ts';
import { deliverFollowUp } from './followup.ts';
import { ownerIdentity } from './owner.ts';
import { inspectRecoveryCandidate } from './recovery.ts';
import { resumeAuthorizedRuntime } from './resume-runtime.ts';
import { validateDirectory } from './storage.ts';

export interface Candidate {
  launch: Launch;
  admission_id: string;
  attempt: { generation: number; [key: string]: unknown };
  execution_directory: string;
  last_lease: { launch: Launch; sequence: number; daemon_epoch: string };
}

function owner(directory: string): Owner {
  const evidence = ownerIdentity(directory);
  const time = readlinkSync('/proc/self/ns/time');
  requireCondition(readlinkSync(`/proc/${process.ppid}/ns/time`) === time, 'assignment-lease-denied');
  return { boot_id: evidence.owner.boot_id, time_namespace: time, host_pid: evidence.owner.host_pid,
    host_start: evidence.owner.host_start, worker_pid: process.pid, worker_start: evidence.worker_start,
    lock_device: evidence.owner.lock_device, lock_inode: evidence.owner.lock_inode };
}
function unblocked(directory: string): void {
  try { lstatSync(join(directory, 'watchdog.json')); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return; }
  throw new Denied('assignment-lease-denied');
}

/** Host owns components/credentials, initial unarmed preparation and committed
 * delivery. prepare must independently bootstrap and build guarded pinned tools.
 * Renewal input stays readable while recovered model/tool execution is active.
 */
export async function serveRecovery(directory: string, worktree: string,
  prepare: (candidate: Candidate, authorization: Authorization) => Promise<HarnessOptions>): Promise<void> {
  process.umask(0o077);
  validateDirectory(directory, worktree);
  const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
  const emit = (value: object) => console.log(JSON.stringify(value));
  let authorization: Authorization | undefined, candidate: Candidate | undefined;
  let expectation: string | undefined, started = false, ready = false, cancelled = false;
  let execution: Promise<void> | undefined, harness: Harness | undefined;
  const cancel = () => { cancelled = true; authorization?.close(); };
  process.on('SIGTERM', cancel);
  try {
    for await (const line of lines) {
      if (!ready) {
        requireCondition(line === '{"host_ready":true}', 'runner-owner-invalid'); ready = true;
        emit({ event: 'owned', identity: ownerIdentity(directory), compatibility: compatibility() }); continue;
      }
      requireCondition(!cancelled, 'assignment-lease-denied');
      unblocked(directory);
      const control = JSON.parse(line);
      if ('host_prepare_recovery' in control) {
        exactKeys(control, ['host_prepare_recovery', 'candidate', 'grant']);
        requireCondition(control.host_prepare_recovery === 1 && !started, 'assignment-lease-denied');
        if (expectation) { requireCondition(canonical(control) === expectation, 'assignment-lease-denied'); continue; }
        exactKeys(control.candidate, ['launch', 'admission_id', 'attempt', 'execution_directory', 'last_lease']);
        validateLaunch(control.candidate.launch);
        const c = structuredClone(control.candidate) as Candidate;
        requireCondition(c.execution_directory === worktree && c.attempt?.generation === control.grant?.lease?.generation, 'assignment-lease-denied');
        requireCondition(canonical(c.last_lease.launch) === canonical(c.launch) && control.grant.lease.sequence === c.last_lease.sequence + 1 && c.last_lease.daemon_epoch !== control.grant.lease.daemon_epoch, 'assignment-lease-denied');
        await inspectRecoveryCandidate(directory, c.launch, c.admission_id);
        unblocked(directory);
        requireCondition(!cancelled, 'assignment-lease-denied');
        authorization = Authorization.forRecovery(c.launch, c.attempt.generation, owner(directory), control.grant.lease.daemon_epoch, control.grant);
        candidate = c; expectation = canonical(control);
        emit({ event: 'recovery-prepared' });
      } else if ('host_recovery' in control) {
        exactKeys(control, ['host_recovery', 'grant']);
        requireCondition(control.host_recovery === 1 && authorization && candidate && expectation, 'assignment-lease-denied');
        authorization.acceptRecovery(control.grant);
        emit({ event: 'authorized', sequence: control.grant.lease.sequence });
        if (!started) {
          started = true;
          const auth = authorization, c = candidate;
          execution = (async () => {
            try {
              const recovered = await resumeAuthorizedRuntime(directory, worktree, c.launch, c.attempt.generation, c.admission_id, auth, () => prepare(c, auth));
              harness = recovered.harness;
              emit({ event: 'resumed', root: recovered.root, admission_id: recovered.admission_id });
              const submission = await harness.submission(Number(c.admission_id) as SubmissionId, context);
              const settled = await submission!.wait(context);
              emit({ event: 'settled', status: settled.status });
            } catch { auth.close(); emit({ event: 'runtime-failed' }); }
          })();
        }
      } else if ('host_followup' in control) {
        // Whole-envelope operator input from the protected supervisor pipe only.
        // Delivery requires the resumed harness and current guarded authority.
        exactKeys(control, ['host_followup', 'followup']);
        requireCondition(control.host_followup === 1 && harness && authorization && candidate, 'assignment-lease-denied');
        try {
          const delivered = await deliverFollowUp(harness, directory, candidate.launch, candidate.attempt.generation, authorization, control.followup);
          emit({ event: 'followup-accepted', operation_id: control.followup.operation_id, ...delivered });
        } catch { emit({ event: 'followup-failed', operation_id: typeof control.followup?.operation_id === 'string' ? control.followup.operation_id : null }); }
      } else {
        exactKeys(control, ['host_control', 'lease']);
        requireCondition(control.host_control === 1 && started && authorization, 'assignment-lease-denied');
        authorization.accept(control.lease);
        emit({ event: 'authorized', sequence: control.lease.sequence });
      }
    }
  } catch { cancel(); throw new Denied('runner-recovery-failed'); }
  finally {
    cancel(); lines.close(); process.off('SIGTERM', cancel);
    if (execution) await execution;
    if (harness) await harness.close(context);
  }
}
