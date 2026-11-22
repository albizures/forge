// Production Check runner composition. The packaged entry (`check-entry.ts`)
// runs `runCheckRunner` with no substitution; controlled tests substitute only the
// selected model provider. Bootstrap, profile validation, model-key loading, the
// approved coding tools, lease guards, effect inspection and storage are shipped.
//
// Runner protocol 1, as seen by this process (one versioned contract):
//   stdin  (only from the directly spawned forge-runner-host):
//     {"host_ready":true}                        once, first line
//     SupervisorControl  {host_control, lease}   grant / renewal
//     RunnerRequest      {compatibility, …, operation: admit | observe}
//     SupervisorRecoveryPreparation / SupervisorRecovery   same-attempt recovery
//     SupervisorFollowUp {host_followup, followup}         recorded operator input
//   (host_retire / host_quiesce are consumed by the host: retirement is SIGTERM.)
//   stdout: one JSON line per RunnerEvent ({"event": …}), RunnerResponse, or
//     RunnerRefusal ({"error":{"code"}}) for an uncorrelatable request.
// The input binding is the owner-only `check-runtime.json` written before spawn by
// the trusted Rust host (`forge_runner_host::check_runtime::write`).
import { createInterface } from 'node:readline';
import { closeSync, constants, fstatSync, openSync, readSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { BACKGROUND_CONTEXT as context, withAbortSignal } from '@earendil-works/chord/context';
import { createModels, type Provider } from '@earendil-works/pi-ai';
import { Harness, type AgentChange, type HarnessOptions } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { Authorization, guardProvider } from './authorization.ts';
import type { CapabilityEnvironment, CheckBinding } from './bootstrap.ts';
import { createCheckRuntime } from './check-runtime.ts';
import { canonical, compatibility, Denied, exactKeys, requireCondition, validateLaunch, validateRequest, type Launch } from './contract.ts';
import { ownerIdentity } from './owner.ts';
import type { Configuration } from './profiles.ts';
import { followUp, isRecoveryControl, RecoveryConsumer, unblocked, workerOwner } from './recovery-consumer.ts';
import { databasePath, journal, observe, persistIntent, persistRoot, protectedPath, validateDirectory } from './storage.ts';

export const CHECK_RUNTIME_FILE = 'check-runtime.json';
export const CHECK_RUNTIME_VERSION = 1;
const MAX_INPUT = 1024 * 1024;
const ADMISSION_INPUT = 'Execute the authenticated bound Check only.';
const boundKeys = ['assignmentId', 'recordId', 'profileRevision', 'profileRecordType', 'provider', 'agentKind', 'model', 'attemptNumber', 'checkAttempt', 'executionDirectory', 'providerWorkspace'];
const capabilityKeys = ['FORGE_ASSIGNMENT_ID', 'FORGE_ASSIGNMENT_DAEMON_URL', 'FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE', 'FORGE_ASSIGNMENT_FORGE_CLI'];

/** The original Check binding of one runtime. It is used for both the first
 * admission and any later same-attempt recovery; it never carries key bytes,
 * capability bytes, behavior or a lease. */
export interface CheckRuntimeInput {
  version: 1;
  launch: Launch;
  generation: number;
  bound: CheckBinding;
  configuration: Configuration;
  credential_file: string;
  capability: CapabilityEnvironment;
}

/** Test-only seam: replace the selected model provider and nothing else. */
export interface ProviderSubstitution {
  provider: Provider;
  model: NonNullable<AgentChange['model']>;
}
export type SubstituteProvider = (authorization: Authorization) => ProviderSubstitution;

const invalid = () => new Denied('runner-check-input-invalid');

/** Strict structural validation. Bootstrap re-validates the binding against Forge;
 * the profile module re-validates configuration, resources and authentication. */
export function validateCheckRuntime(value: unknown, worktree: string): CheckRuntimeInput {
  try {
    exactKeys(value, ['version', 'launch', 'generation', 'bound', 'configuration', 'credential_file', 'capability']);
    requireCondition(value.version === CHECK_RUNTIME_VERSION, 'runner-check-input-invalid');
    validateLaunch(value.launch);
    const { launch, generation, bound, capability } = value;
    requireCondition(Number.isSafeInteger(generation) && generation > 0, 'runner-check-input-invalid');
    exactKeys(bound, boundKeys);
    requireCondition(bound.assignmentId === launch.assignment_id && bound.profileRevision === launch.profile_revision, 'runner-check-input-invalid');
    requireCondition(bound.checkAttempt !== null && typeof bound.checkAttempt === 'object' && bound.checkAttempt.id === launch.attempt_id && bound.checkAttempt.generation === generation, 'runner-check-input-invalid');
    requireCondition(bound.executionDirectory === worktree && value.configuration?.cwd === worktree, 'runner-check-input-invalid');
    exactKeys(capability, capabilityKeys);
    requireCondition(capabilityKeys.every(key => typeof capability[key] === 'string' && capability[key].length > 0 && !capability[key].includes('\0')), 'runner-check-input-invalid');
    requireCondition(capability.FORGE_ASSIGNMENT_ID === launch.assignment_id, 'runner-check-input-invalid');
    requireCondition(typeof value.credential_file === 'string' && isAbsolute(value.credential_file) && value.credential_file !== capability.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE, 'runner-check-input-invalid');
    return structuredClone(value) as CheckRuntimeInput;
  } catch { throw invalid(); }
}

/** Read the protected, owner-only, single-link input. Never from the worktree,
 * environment, model, follow-up or an operator payload. */
export function readCheckRuntime(directory: string, worktree: string): CheckRuntimeInput {
  let fd: number | undefined;
  try {
    const path = join(directory, CHECK_RUNTIME_FILE);
    protectedPath(path);
    fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
    requireCondition(fstatSync(fd).size <= MAX_INPUT, 'runner-check-input-invalid');
    const bytes = Buffer.alloc(MAX_INPUT + 1);
    let used = 0;
    for (let count; used < bytes.length && (count = readSync(fd, bytes, used, bytes.length - used, null));) used += count;
    requireCondition(used <= MAX_INPUT, 'runner-check-input-invalid');
    return validateCheckRuntime(JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes.subarray(0, used))), worktree);
  } catch { throw invalid(); }
  finally { if (fd !== undefined) closeSync(fd); }
}

/** Effect inspection, then bootstrap, then the explicit profile's guarded model
 * and tools. A substitution replaces only the provider behind the same guard. */
export async function composeCheckRuntime(input: CheckRuntimeInput, launch: Launch, generation: number,
  authorization: Authorization, directory: string, emit: (value: object) => void, substitute?: SubstituteProvider): Promise<{ options: HarnessOptions; agent: AgentChange }> {
  const runtime = await createCheckRuntime(launch, generation, input.bound, input.capability, input.configuration, input.credential_file, authorization, directory);
  emit({ event: 'bootstrapped' });
  if (!substitute) return runtime;
  const { provider, model } = substitute(authorization);
  const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
  models.setProvider(guardProvider(provider, authorization));
  return { options: { ...runtime.options, models }, agent: { ...runtime.agent, model } };
}

export interface CheckAdmission {
  harness: Harness;
  admission_id: string;
  settled: Promise<string>;
}

/** First admission of the bound input. The caller has validated identity and
 * holds a current lease. Bootstrap precedes every model/tool construction and the
 * runtime store; a failure before intent is a rejected admission, after it unknown. */
export async function admitCheck(input: CheckRuntimeInput, authorization: Authorization, directory: string,
  emit: (value: object) => void, substitute?: SubstituteProvider): Promise<CheckAdmission> {
  const launch = structuredClone(input.launch);
  const generation = input.generation;
  authorization.admitFor(launch, generation);
  const runtime = await composeCheckRuntime(input, launch, generation, authorization, directory, emit, substitute);
  const executionContext = withAbortSignal(authorization.signal, context);
  authorization.admitFor(launch, generation);
  persistIntent(directory, launch);
  let harness: Harness | undefined;
  try {
    harness = await Harness.open(await openNodeSqliteStorage(databasePath(directory)), runtime.options, executionContext);
    authorization.admitFor(launch, generation);
    const root = await harness.root(executionContext, { agent: runtime.agent });
    persistRoot(directory, root.id);
    authorization.admitFor(launch, generation);
    const submission = await root.submit({ type: 'input', requestId: launch.operation_id, content: ADMISSION_INPUT }, executionContext);
    return { harness, admission_id: String(submission.id), settled: submission.wait(context).then(result => result.status) };
  } catch {
    authorization.close();
    if (harness) await harness.close(context).catch(() => {});
    throw new AdmissionUncertain();
  }
}
/** Intent is durable: the admission outcome is unknown, never rejected. */
export class AdmissionUncertain extends Denied { constructor() { super('runner-admission-uncertain'); } }

/** The packaged Check runner process. Returns the process exit code. */
export async function runCheckRunner(directory: string, worktree: string, substitute?: SubstituteProvider): Promise<number> {
  process.umask(0o077);
  const emit = (value: object) => console.log(JSON.stringify(value));
  const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
  let input: CheckRuntimeInput;
  let authorization: Authorization | undefined;
  let fenced = false;
  let admission: CheckAdmission | undefined;
  let settling: Promise<void> | undefined;
  const recovery = new RecoveryConsumer(directory, worktree, async (candidate, auth) => {
    requireCondition(canonical(candidate.launch) === canonical(input.launch) && canonical(candidate.attempt) === canonical(input.bound.checkAttempt), 'runner-recovery-binding-invalid');
    return (await composeCheckRuntime(input, candidate.launch, candidate.attempt.generation, auth, directory, emit, substitute)).options;
  }, emit);
  // Trusted supervisor cancellation: close admission and abort guarded execution.
  // The independent host bounds shutdown and verifies descendants.
  const cancel = () => { fenced = true; authorization?.close(); recovery.cancel(); };
  process.on('SIGTERM', cancel);

  // Fresh grant/renewal. Never loaded from disk, a tool or an agent RPC.
  function authorize(control: Record<string, any>): void {
    try {
      exactKeys(control, ['host_control', 'lease']);
      requireCondition(control.host_control === 1 && !fenced, 'assignment-lease-denied');
      const lease = control.lease;
      requireCondition(lease && typeof lease === 'object' && canonical(lease.launch) === canonical(input.launch) && lease.generation === input.generation, 'assignment-lease-denied');
      authorization ??= new Authorization(input.launch, input.generation, workerOwner(directory), lease.daemon_epoch);
      authorization.accept(lease);
      emit({ event: 'authorized', sequence: lease.sequence });
    } catch {
      fenced = true;
      authorization?.close();
      throw new Denied('assignment-lease-denied');
    }
  }

  async function admit(launch: Launch) {
    const prior = journal(directory);
    if (prior) {
      // Exact replay is read-only; reopening writable storage needs recovery.
      requireCondition(canonical(prior.launch) === canonical(launch), 'runner-operation-conflict');
      return observe(directory, launch.assignment_id, launch.runtime_id, launch.operation_id);
    }
    requireCondition(canonical(launch) === canonical(input.launch) && authorization && !fenced && !admission, 'assignment-lease-denied');
    try {
      admission = await admitCheck(input, authorization, directory, emit, substitute);
    } catch (error) {
      // Terminal for this incarnation: no retry may re-run bootstrap or reopen.
      fenced = true;
      emit({ event: 'runtime-failed' });
      if (error instanceof AdmissionUncertain) return { state: 'unknown' };
      return { state: 'rejected', code: error instanceof Denied ? error.code : 'runner-admission-failed' };
    }
    const auth = authorization;
    settling = admission.settled.then(
      status => emit({ event: 'settled', status }),
      () => { auth.close(); emit({ event: 'runtime-failed' }); },
    );
    return { state: 'accepted', admission_id: admission.admission_id };
  }

  try {
    validateDirectory(directory, worktree);
    input = readCheckRuntime(directory, worktree);
    let ready = false;
    for await (const line of lines) {
      if (!ready) {
        requireCondition(line === '{"host_ready":true}', 'runner-owner-invalid');
        ready = true;
        emit({ event: 'owned', identity: ownerIdentity(directory), compatibility: compatibility() });
        continue;
      }
      let control: Record<string, any>;
      try {
        control = JSON.parse(line);
        requireCondition(control !== null && typeof control === 'object' && !Array.isArray(control), 'runner-request-invalid');
      } catch { emit({ error: { code: 'runner-request-invalid' } }); continue; }
      // Recovery and first admission are mutually exclusive in one incarnation.
      if (isRecoveryControl(control) || (recovery.active && ('host_control' in control || 'host_followup' in control))) {
        requireCondition(!authorization && !fenced, 'assignment-lease-denied');
        unblocked(directory);
        await recovery.handle(control); // Rejection is terminal for this process.
        continue;
      }
      try {
        if ('host_control' in control) { unblocked(directory); authorize(control); continue; }
        if ('host_followup' in control) {
          exactKeys(control, ['host_followup', 'followup']);
          requireCondition(control.host_followup === 1 && admission && authorization && !fenced, 'assignment-lease-denied');
          await followUp(admission.harness, directory, input.launch, input.generation, authorization, control.followup, emit);
          continue;
        }
        validateRequest(control);
        const retained = journal(directory);
        if (retained) requireCondition(retained.launch.assignment_id === control.assignment_id && retained.launch.runtime_id === control.runtime_id, 'runner-identity-conflict');
        const result = control.operation.kind === 'admit'
          ? await admit(control.operation.launch)
          : await observe(directory, control.assignment_id, control.runtime_id, control.operation.admission_operation_id);
        emit({ compatibility: compatibility(), assignment_id: control.assignment_id, runtime_id: control.runtime_id, operation_id: control.operation_id, admission: result });
      } catch (error) {
        // No input, paths, credentials or upstream errors in protocol diagnostics.
        emit({ error: { code: error instanceof Denied ? error.code : 'runner-request-failed' } });
      }
    }
    return 0;
  } catch (error) {
    console.error(error instanceof Denied ? error.code : 'runner-start-failed');
    return 1;
  } finally {
    cancel();
    lines.close();
    process.off('SIGTERM', cancel);
    await recovery.finish().catch(() => {});
    // Cancellation aborted guarded execution; do not wait for a settled answer.
    void settling;
    if (admission) await admission.harness.close(context).catch(() => {});
  }
}
