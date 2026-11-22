// Read-only recovery admission fence for the pinned pi.tool v3 contract.
// Run under exclusive host ownership BEFORE Harness.open: opening the Harness
// can settle unsafe calls as interrupted and immediately schedule another model.
import { statSync } from 'node:fs';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import type { ConversationId, Cursor, ToolRegistration } from '@earendil-works/pi-durable';
import { Authorization, guardTool } from './authorization.ts';
import { canonical, Denied, requireCondition, type Launch } from './contract.ts';
import { journal, readStorage } from './storage.ts';

const MAX_FAILURE_TEXT = 2000;

export interface InterruptedEffect {
  task_id: string;
  conversation_id: string;
  call_id: string;
}

/** Does not repair tasks, create input, inspect Git, or infer external outcomes.
 * Intent/arguments remain in protected runtime storage. Only bounded identities
 * cross this boundary, never command text, output or credentials.
 */
export async function inspectInterruptedEffects(directory: string, launch: Launch): Promise<InterruptedEffect[]> {
  const identity = journal(directory);
  if (!identity) return [];
  requireCondition(canonical(identity.launch) === canonical(launch), 'runner-identity-conflict');
  // Interrupted initialization is not permission to initialize another store.
  requireCondition(typeof identity.root === 'number' && Number.isSafeInteger(identity.root) && identity.root > 0, 'runner-recovery-required');
  const storage = await readStorage(directory);
  try {
    // A journal pointer alone is not a committed conversation. In particular,
    // an empty tool scan must not certify interrupted/corrupt initialization or
    // a child transcript substituted for the Assignment root as recoverable.
    const root = await storage.conversation(identity.root as ConversationId, context);
    requireCondition(root !== undefined && root.id === identity.root && root.parent === undefined && root.owner === undefined, 'runner-recovery-required');
    const effects: InterruptedEffect[] = [];
    let cursor: Cursor | undefined;
    let pages = 0;
    do {
      requireCondition(++pages <= 1000, 'runner-effect-inspection-limit');
      // Scan the whole Assignment store, including child conversations. A root-
      // only scan could miss effects owned by nested execution machinery.
      const page = await storage.scanTasks({ kind: 'pi.tool' }, 100, cursor, context);
      for (const task of page.items) {
        if (task.kind !== 'pi.tool') continue;
        requireCondition(task.version === 3, 'runner-tool-contract-incompatible');
        const state = task.state;
        if (state.status === 'terminal' || state.status === 'completing') {
          if (state.outcome.status === 'completed') continue;
          // Cancellation, faulting and interruption receipts do not establish
          // absence of an external effect. Keep these conservative blockers.
        } else {
          const checkpoint = state.checkpoint;
          requireCondition(checkpoint !== null && typeof checkpoint === 'object' && !Array.isArray(checkpoint), 'runner-tool-contract-incompatible');
          if (checkpoint.phase === 'call') continue; // durable execution intent not yet committed
          requireCondition(checkpoint.phase === 'execute', 'runner-tool-contract-incompatible');
          // No automatic replay-safe exception: all currently approved tools
          // are unsafe, including reads (shell/ambient filesystem semantics).
        }
        const input = task.input;
        requireCondition(input !== null && typeof input === 'object' && !Array.isArray(input), 'runner-tool-contract-incompatible');
        const call = input.kind === 'nested' ? input.call : undefined;
        const callId = input.kind === 'model' ? input.callId
          : call !== null && typeof call === 'object' && !Array.isArray(call) ? call.id : undefined;
        requireCondition(typeof callId === 'string' && callId.length > 0 && callId.length <= 1024, 'runner-tool-contract-incompatible');
        effects.push({ task_id: String(task.id), conversation_id: String(task.conversationId), call_id: callId as string });
        requireCondition(effects.length <= 1000, 'runner-effect-inspection-limit');
      }
      cursor = page.next;
    } while (cursor !== undefined);
    return effects;
  } finally { await storage.close(context); }
}

/** Counts started ExecutionEnv operations that may have an external effect.
 * Fail-closed: only the named read-only methods are uncounted, so any other
 * method (writes, removal, directories, temp files, `exec`, watchers, cleanup,
 * and any method a future upstream adds) counts as a possible effect once it is
 * invoked, whether or not it then succeeds.
 */
export class EffectMeter {
  #started = 0;
  get started(): number { return this.#started; }
  note(): void { this.#started++; }
}

const READ_ONLY_ENV = new Set([
  'absolutePath', 'joinPath', 'canonicalPath', 'exists', 'fileInfo', 'listDir', 'openDirReader',
  'readTextFile', 'readTextLines', 'readBinaryFile', 'openTextLineReader', 'openBinaryReader',
]);
/** File-content reads open their (symlink-followed) target. Opening a FIFO,
 * device or socket can consume data or block a peer, so such a read is
 * uncounted only when its target is a regular file or does not exist.
 */
const CONTENT_READS = new Set(['readTextFile', 'readTextLines', 'readBinaryFile', 'openTextLineReader', 'openBinaryReader']);

function readOnlyCall(property: string | symbol, args: unknown[]): boolean {
  if (typeof property !== 'string' || !READ_ONLY_ENV.has(property)) return false;
  if (!CONTENT_READS.has(property)) return true;
  const path = args[0];
  if (typeof path !== 'string') return false;
  try {
    return statSync(path).isFile(); // stat follows links and never opens the target
  } catch (error) {
    return (error as NodeJS.ErrnoException)?.code === 'ENOENT' || (error as NodeJS.ErrnoException)?.code === 'ENOTDIR';
  }
}

/** The coding tools reach the filesystem and shell only through this env (the
 * pinned pi-durable tools import no `fs` or `child_process`). Methods run bound
 * to the real env, which keeps its private state.
 */
export function meteredEnv<T extends object>(env: T, meter: EffectMeter): T {
  return new Proxy(env, {
    get(target, property) {
      const value: unknown = Reflect.get(target, property, target);
      if (typeof value !== 'function') return value;
      return (...args: unknown[]) => {
        if (!readOnlyCall(property, args)) meter.note();
        return Reflect.apply(value, target, args);
      };
    },
  });
}

/** Bounded model-facing text for an effect-free failure; undefined if the error
 * cannot be rendered safely (the caller then fails closed). */
function failureText(error: unknown): string | undefined {
  try {
    const reason = error instanceof Error && typeof error.message === 'string' ? error.message : 'tool failed';
    return `Tool failed without any effect: ${[...reason.replaceAll('\0', '')].slice(0, MAX_FAILURE_TEXT).join('')}`;
  } catch {
    return undefined;
  }
}

/** A failed/cancelled invocation cannot be fed back into the model as permission
 * to immediately repeat an unknown effect. Close this process's authorization;
 * later committed read-only inspection and Forge retention still own recovery.
 * A returned tool result (including a known shell exit code) is not a thrown,
 * interrupted invocation. Persistence failure remains the Harness's boundary.
 *
 * Exception (with a meter): an invocation that threw before it started any
 * counted env operation, while neither the lease nor the call was aborted, had
 * no external effect through the env (a missing file, `edit` text not found,
 * invalid input): uncounted operations are path resolution, metadata, listing
 * and content reads of regular files only. It returns an ordinary error result, which the Harness
 * commits as a completed call, so the model can correct itself and recovery is
 * not blocked. Any concurrently started operation also counts, so overlap only
 * ever makes classification more conservative. Without a meter every thrown
 * invocation stays an interrupted effect.
 */
export function guardEffectTool(execute: ToolRegistration['execute'], authorization: Authorization, meter?: EffectMeter): ToolRegistration['execute'] {
  const guarded = guardTool(execute, authorization);
  return (args, api, context) => {
    // Keep the established synchronous admission-denial contract; a denied
    // invocation has not started an effect and is not an interrupted effect.
    authorization.admit();
    const before = meter?.started;
    return (async () => {
      try {
        return await guarded(args, api, context);
      } catch (error) {
        if (meter !== undefined && meter.started === before && !authorization.signal.aborted
          && !context.abortSignal?.aborted && !(error instanceof Error && error.name === 'AbortError')) {
          // The tool's own local failure text for a model-chosen path or input,
          // bounded. Like successful tool output it is stored in the transcript.
          const text = failureText(error);
          if (text !== undefined) return { output: [{ type: 'text' as const, text }], isError: true };
        }
        authorization.close();
        // No arbitrary upstream error, command, output or credentials escape here.
        throw new Denied('runner-interrupted-effect');
      }
    })();
  };
}

export async function requireNoInterruptedEffects(directory: string, launch: Launch): Promise<void> {
  requireCondition((await inspectInterruptedEffects(directory, launch)).length === 0, 'runner-interrupted-effect');
}
