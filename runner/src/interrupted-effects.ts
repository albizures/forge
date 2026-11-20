// Read-only recovery admission fence for the pinned pi.tool v3 contract.
// Run under exclusive host ownership BEFORE Harness.open: opening the Harness
// can settle unsafe calls as interrupted and immediately schedule another model.
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import type { Cursor } from '@earendil-works/pi-durable';
import { canonical, requireCondition, type Launch } from './contract.ts';
import { journal, readStorage } from './storage.ts';

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
  requireCondition(identity.root !== null, 'runner-recovery-required');
  const storage = await readStorage(directory);
  try {
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

export async function requireNoInterruptedEffects(directory: string, launch: Launch): Promise<void> {
  requireCondition((await inspectInterruptedEffects(directory, launch)).length === 0, 'runner-interrupted-effect');
}
