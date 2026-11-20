// Committed recovery evidence only. Never open a Harness or start its scheduler.
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import type { ConversationId } from '@earendil-works/pi-durable';
import { canonical, requireCondition, type Launch } from './contract.ts';
import { requireNoInterruptedEffects } from './interrupted-effects.ts';
import { journal, readStorage, validateStorageDirectory } from './storage.ts';

/** This is a necessary runtime precondition, NOT execution permission. The
 * trusted caller must separately validate current Forge authority, freshness,
 * worktree/profile bindings and exclusive host ownership before any writable
 * open, then obtain a newly committed lease. Retained leases are not reusable.
 *
 * A completed or failed input requires intervention, not a new input/continue
 * submission manufactured by recovery. Only the original pending admission is
 * a candidate. An empty/malformed store is never initialized or repaired here.
 * Call under exclusive ownership so evidence cannot change between inspection
 * and a later authorized open; observation alone cannot prove ownership/death.
 */
export async function inspectRecoveryCandidate(
  directory: string,
  launch: Launch,
  admissionId: string,
): Promise<{ root: number; admission_id: string; status: 'queued' | 'placed' }> {
  // Snapshot before the first await; caller mutation must not rebind inspection.
  const expected = structuredClone(launch);
  requireCondition(typeof admissionId === 'string' && admissionId.length > 0, 'runner-admission-invalid');
  validateStorageDirectory(directory);
  const identity = journal(directory);
  requireCondition(identity !== undefined, 'runner-recovery-required');
  requireCondition(canonical(identity.launch) === canonical(expected), 'runner-identity-conflict');
  // This validates the committed root (including no parent/owner), and scans all
  // conversations for unsettled effects BEFORE any potential writable repair.
  await requireNoInterruptedEffects(directory, expected);
  const storage = await readStorage(directory);
  try {
    const submission = await storage.submissionByRequest(identity.root as ConversationId, expected.operation_id, context);
    requireCondition(submission !== undefined, 'runner-recovery-required');
    requireCondition(String(submission.id) === admissionId && submission.conversationId === identity.root && submission.type === 'input', 'runner-admission-conflict');
    requireCondition(submission.status === 'queued' || submission.status === 'placed', 'runner-recovery-intervention-required');
    return { root: identity.root as number, admission_id: String(submission.id), status: submission.status };
  } finally { await storage.close(context); }
}
