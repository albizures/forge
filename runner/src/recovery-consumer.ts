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
export type PrepareRecovery = (candidate: Candidate, authorization: Authorization) => Promise<HarnessOptions>;

export function workerOwner(directory: string): Owner {
  const evidence = ownerIdentity(directory);
  const time = readlinkSync('/proc/self/ns/time');
  requireCondition(readlinkSync(`/proc/${process.ppid}/ns/time`) === time, 'assignment-lease-denied');
  return { boot_id: evidence.owner.boot_id, time_namespace: time, host_pid: evidence.owner.host_pid,
    host_start: evidence.owner.host_start, worker_pid: process.pid, worker_start: evidence.worker_start,
    lock_device: evidence.owner.lock_device, lock_inode: evidence.owner.lock_inode };
}
export function unblocked(directory: string): void {
  try { lstatSync(join(directory, 'watchdog.json')); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return; }
  throw new Denied('assignment-lease-denied');
}

export const isRecoveryControl = (control: Record<string, unknown>) =>
  'host_prepare_recovery' in control || 'host_recovery' in control;

/** Host owns components/credentials, initial unarmed preparation and committed
 * delivery. prepare must independently bootstrap and build guarded pinned tools.
 * Renewal input stays readable while recovered model/tool execution is active.
 * Any rejected control is terminal for this incarnation: the caller must cancel.
 */
export class RecoveryConsumer {
  #authorization: Authorization | undefined;
  #candidate: Candidate | undefined;
  #expectation: string | undefined;
  #started = false;
  #execution: Promise<void> | undefined;
  #harness: Harness | undefined;
  readonly #directory: string;
  readonly #worktree: string;
  readonly #prepare: PrepareRecovery;
  readonly #emit: (value: object) => void;

  constructor(directory: string, worktree: string, prepare: PrepareRecovery, emit: (value: object) => void) {
    this.#directory = directory; this.#worktree = worktree; this.#prepare = prepare; this.#emit = emit;
  }

  /** True once a preparation was accepted: ordinary renewals now belong here. */
  get active(): boolean { return this.#expectation !== undefined; }

  async handle(control: Record<string, unknown>): Promise<void> {
    if ('host_prepare_recovery' in control) await this.#prepareRecovery(control);
    else if ('host_recovery' in control) this.#acceptRecovery(control);
    else if ('host_followup' in control) await this.#followUp(control);
    else {
      exactKeys(control, ['host_control', 'lease']);
      requireCondition(control.host_control === 1 && this.#started && this.#authorization, 'assignment-lease-denied');
      this.#authorization.accept(control.lease);
      this.#emit({ event: 'authorized', sequence: (control.lease as { sequence: number }).sequence });
    }
  }

  async #prepareRecovery(control: Record<string, any>): Promise<void> {
    exactKeys(control, ['host_prepare_recovery', 'candidate', 'grant']);
    requireCondition(control.host_prepare_recovery === 1 && !this.#started, 'assignment-lease-denied');
    if (this.#expectation) { requireCondition(canonical(control) === this.#expectation, 'assignment-lease-denied'); return; }
    exactKeys(control.candidate, ['launch', 'admission_id', 'attempt', 'execution_directory', 'last_lease']);
    validateLaunch(control.candidate.launch);
    const c = structuredClone(control.candidate) as Candidate;
    requireCondition(c.execution_directory === this.#worktree && c.attempt?.generation === control.grant?.lease?.generation, 'assignment-lease-denied');
    requireCondition(canonical(c.last_lease.launch) === canonical(c.launch) && control.grant.lease.sequence === c.last_lease.sequence + 1 && c.last_lease.daemon_epoch !== control.grant.lease.daemon_epoch, 'assignment-lease-denied');
    await inspectRecoveryCandidate(this.#directory, c.launch, c.admission_id);
    unblocked(this.#directory);
    this.#authorization = Authorization.forRecovery(c.launch, c.attempt.generation, workerOwner(this.#directory), control.grant.lease.daemon_epoch, control.grant);
    this.#candidate = c; this.#expectation = canonical(control);
    this.#emit({ event: 'recovery-prepared' });
  }

  #acceptRecovery(control: Record<string, any>): void {
    exactKeys(control, ['host_recovery', 'grant']);
    requireCondition(control.host_recovery === 1 && this.#authorization && this.#candidate && this.#expectation, 'assignment-lease-denied');
    this.#authorization.acceptRecovery(control.grant);
    this.#emit({ event: 'authorized', sequence: control.grant.lease.sequence });
    if (this.#started) return;
    this.#started = true;
    const auth = this.#authorization, c = this.#candidate;
    this.#execution = (async () => {
      try {
        const recovered = await resumeAuthorizedRuntime(this.#directory, this.#worktree, c.launch, c.attempt.generation, c.admission_id, auth, () => this.#prepare(c, auth));
        this.#harness = recovered.harness;
        this.#emit({ event: 'resumed', root: recovered.root, admission_id: recovered.admission_id });
        const submission = await this.#harness.submission(Number(c.admission_id) as SubmissionId, context);
        const settled = await submission!.wait(context);
        this.#emit({ event: 'settled', status: settled.status });
      } catch { auth.close(); this.#emit({ event: 'runtime-failed' }); }
    })();
  }

  // Whole-envelope operator input from the protected supervisor pipe only.
  // Delivery requires the resumed harness and current guarded authority.
  async #followUp(control: Record<string, any>): Promise<void> {
    exactKeys(control, ['host_followup', 'followup']);
    requireCondition(control.host_followup === 1 && this.#harness && this.#authorization && this.#candidate, 'assignment-lease-denied');
    await followUp(this.#harness, this.#directory, this.#candidate.launch, this.#candidate.attempt.generation, this.#authorization, control.followup, this.#emit);
  }

  cancel(): void { this.#authorization?.close(); }

  async finish(): Promise<void> {
    this.cancel();
    if (this.#execution) await this.#execution;
    if (this.#harness) await this.#harness.close(context);
  }
}

/** Shared follow-up delivery event shape for admission and recovery sessions. */
export async function followUp(harness: Harness, directory: string, launch: Launch, generation: number,
  authorization: Authorization, value: unknown, emit: (value: object) => void): Promise<void> {
  const operation = (value as { operation_id?: unknown } | null)?.operation_id;
  try {
    const delivered = await deliverFollowUp(harness, directory, launch, generation, authorization, value as never);
    emit({ event: 'followup-accepted', operation_id: (value as { operation_id: string }).operation_id, ...delivered });
  } catch { emit({ event: 'followup-failed', operation_id: typeof operation === 'string' ? operation : null }); }
}

/** Recovery-only session loop over the same consumer the production Check entry uses. */
export async function serveRecovery(directory: string, worktree: string, prepare: PrepareRecovery): Promise<void> {
  process.umask(0o077);
  validateDirectory(directory, worktree);
  const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
  const emit = (value: object) => console.log(JSON.stringify(value));
  const consumer = new RecoveryConsumer(directory, worktree, prepare, emit);
  let ready = false, cancelled = false;
  const cancel = () => { cancelled = true; consumer.cancel(); };
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
      requireCondition(control !== null && typeof control === 'object' && !Array.isArray(control), 'runner-request-invalid');
      await consumer.handle(control);
    }
  } catch { cancel(); throw new Denied('runner-recovery-failed'); }
  finally {
    cancel(); lines.close(); process.off('SIGTERM', cancel);
    await consumer.finish();
  }
}
