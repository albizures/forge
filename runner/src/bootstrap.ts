// Trusted Check-bound bootstrap composition, not an agent tool or public RPC.
// No model, tool registry, storage or scheduler is opened by this module.
import { spawn } from 'node:child_process';
import { isAbsolute, dirname } from 'node:path';
import { realpathSync, statSync } from 'node:fs';
import { Authorization } from './authorization.ts';
import { canonical, Denied, requireCondition, type Launch } from './contract.ts';

/** Copied from Forge's persisted Assignment/Check binding by the trusted
 * launcher, never from model output. The complete immutable bound object (including attempt input,
 * continuation and recovery context) is compared, not just the Assignment ID.
 * Rust remains the owner of eligibility, evidence and lifecycle policy.
 */
export interface CheckBinding {
  assignmentId: string;
  recordId: number;
  profileRevision: number;
  profileRecordType: string;
  provider: string;
  agentKind: string;
  model: string;
  attemptNumber: number;
  checkAttempt: { id: string; record_id: number; generation: number; [key: string]: unknown };
  executionDirectory: string;
  providerWorkspace: string | null;
}

export interface CapabilityEnvironment {
  FORGE_ASSIGNMENT_ID: string;
  FORGE_ASSIGNMENT_DAEMON_URL: string;
  FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE: string;
  FORGE_ASSIGNMENT_FORGE_CLI: string;
}

export interface BootstrappedCheck {
  // Behavior-bearing data: never log this object or use it as an Outcome.
  context: Record<string, unknown>;
  behavior: string;
}

const MAX_OUTPUT = 1024 * 1024;
const TIMEOUT_MILLIS = 10_000;
const failed = () => new Denied('assignment-bootstrap-failed');

/** Invoke only the delivered CLI bootstrap, with explicit cwd/argv/environment.
 * A failed/uncertain invocation fences this process incarnation; no retries,
 * model fallback or inference from partial output are permitted. Bootstrap can
 * have acknowledged/started work before a reply is lost: failure is NOT cleanup
 * evidence or permission to release capacity. The host must retain uncertainty.
 */
export async function bootstrapCheck(
  launch: Launch,
  generation: number,
  bound: CheckBinding,
  capability: CapabilityEnvironment,
  authorization: Authorization,
): Promise<BootstrappedCheck> {
  try {
    // Snapshot before awaiting: caller mutation cannot rebind the result.
    const identity = structuredClone(launch);
    const expected = structuredClone(bound);
    const env = structuredClone(capability);
    requireCondition(expected.assignmentId === identity.assignment_id && env.FORGE_ASSIGNMENT_ID === identity.assignment_id && expected.profileRevision === identity.profile_revision, 'assignment-bootstrap-failed');
    requireCondition(expected.profileRecordType === 'task' && expected.provider === 'pi-durable' && expected.model === 'openai/gpt-4.1', 'assignment-bootstrap-failed');
    requireCondition(expected.checkAttempt?.id === identity.attempt_id && expected.checkAttempt.record_id === expected.recordId && expected.checkAttempt.generation === generation && Number.isSafeInteger(generation) && generation > 0, 'assignment-bootstrap-failed');
    requireCondition([expected.recordId, expected.profileRevision, expected.attemptNumber, expected.checkAttempt.check_id].every(value => Number.isSafeInteger(value) && (value as number) > 0), 'assignment-bootstrap-failed');
    const cwd = expected.executionDirectory;
    requireCondition(isAbsolute(cwd) && realpathSync(cwd) === cwd && statSync(cwd).isDirectory(), 'assignment-bootstrap-failed');
    const cli = env.FORGE_ASSIGNMENT_FORGE_CLI;
    requireCondition(typeof cli === 'string' && isAbsolute(cli) && realpathSync(cli) === cli && statSync(cli).isFile(), 'assignment-bootstrap-failed');
    requireCondition(typeof env.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE === 'string' && isAbsolute(env.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE) && typeof env.FORGE_ASSIGNMENT_DAEMON_URL === 'string' && env.FORGE_ASSIGNMENT_DAEMON_URL.length > 0, 'assignment-bootstrap-failed');
    authorization.admitFor(identity, generation);
    const output = await new Promise<Buffer>((resolve, reject) => {
      const child = spawn(cli, ['assignment', 'bootstrap', '--json'], {
        cwd,
        // Do not inherit ambient Pi configuration, auth values or caller extras.
        env: {
          PATH: `${dirname(cli)}:/usr/bin:/bin`, LANG: 'C.UTF-8',
          FORGE_ASSIGNMENT_ID: env.FORGE_ASSIGNMENT_ID,
          FORGE_ASSIGNMENT_DAEMON_URL: env.FORGE_ASSIGNMENT_DAEMON_URL,
          FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE: env.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE,
          FORGE_ASSIGNMENT_FORGE_CLI: cli,
        },
        stdio: ['ignore', 'pipe', 'ignore'],
        shell: false,
      });
      const chunks: Buffer[] = [];
      let length = 0;
      let uncertain = false;
      const stop = () => {
        uncertain = true;
        // Only the directly spawned CLI, never a PID from retained metadata.
        // This is cancellation, not proof that all Assignment effects stopped.
        child.kill('SIGKILL');
      };
      const timer = setTimeout(stop, TIMEOUT_MILLIS);
      authorization.signal.addEventListener('abort', stop, { once: true });
      if (authorization.signal.aborted) stop();
      child.stdout.on('data', (chunk: Buffer) => {
        length += chunk.length;
        if (length > MAX_OUTPUT) { stop(); return; }
        chunks.push(chunk);
      });
      const cleanup = () => {
        clearTimeout(timer);
        authorization.signal.removeEventListener('abort', stop);
      };
      child.once('error', () => { cleanup(); reject(failed()); });
      child.once('close', (code, signal) => {
        cleanup();
        if (uncertain || code !== 0 || signal !== null) reject(failed());
        else resolve(Buffer.concat(chunks));
      });
    });
    authorization.admitFor(identity, generation);
    const response = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(output));
    requireCondition(response?.schemaVersion === 1 && response.context?.schemaVersion === 1 && canonical(response.context.bound) === canonical(expected), 'assignment-bootstrap-failed');
    requireCondition(response.context.record?.id === expected.recordId && response.context.record?.record_type === 'task' && response.context.record?.status === 'in_progress' && typeof response.behavior === 'string' && response.behavior.trim().length > 0, 'assignment-bootstrap-failed');
    return { context: response.context, behavior: response.behavior };
  } catch {
    // Do not expose CLI output, upstream diagnostics, capability or paths.
    authorization.close();
    throw failed();
  }
}
