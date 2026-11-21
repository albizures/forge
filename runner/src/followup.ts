// Operator follow-up delivery into the already-authorized Assignment conversation.
// Never creates a root, Continuation or attempt; never rebinds Check/profile/cwd.
import { createHash } from 'node:crypto';
import { BACKGROUND_CONTEXT as context, withAbortSignal } from '@earendil-works/chord/context';
import type { ConversationId, Harness } from '@earendil-works/pi-durable';
import { Authorization } from './authorization.ts';
import { canonical, exactKeys, requireCondition, type Launch } from './contract.ts';
import { requireNoInterruptedEffects } from './interrupted-effects.ts';
import { journal, readStorage } from './storage.ts';

export const FOLLOWUP_CONTENT_BYTES = 16384;
export interface FollowUp { operation_id: string; content: string; content_sha256: string }
export type FollowUpState = { state: 'unknown' } | { state: 'accepted'; admission_id: string; status: string };

/** The runtime request ID is namespaced so it can never equal an Assignment
 * admission operation ID, and is exact for retry deduplication. */
export const followUpRequestId = (operationId: string) => `followup:${operationId}`;

export function validateFollowUp(value: unknown): asserts value is FollowUp {
  exactKeys(value, ['operation_id', 'content', 'content_sha256']);
  requireCondition(typeof value.operation_id === 'string' && /^[A-Za-z0-9._:-]{1,128}$/.test(value.operation_id), 'runner-followup-invalid');
  requireCondition(typeof value.content === 'string' && value.content.trim() !== '' && Buffer.byteLength(value.content) <= FOLLOWUP_CONTENT_BYTES, 'runner-followup-invalid');
  requireCondition(createHash('sha256').update(value.content).digest('hex') === value.content_sha256, 'runner-followup-invalid');
}

/** Committed, read-only state; a missing admission is unknown, not failure. */
export async function observeFollowUp(directory: string, launch: Launch, operationId: string): Promise<FollowUpState> {
  const identity = journal(directory);
  if (!identity || identity.root === null) return { state: 'unknown' };
  requireCondition(canonical(identity.launch) === canonical(launch), 'runner-identity-conflict');
  const storage = await readStorage(directory);
  try {
    const submission = await storage.submissionByRequest(identity.root as ConversationId, followUpRequestId(operationId), context);
    if (!submission) return { state: 'unknown' };
    requireCondition(submission.type === 'input', 'runner-admission-conflict');
    return { state: 'accepted', admission_id: String(submission.id), status: submission.status };
  } finally { await storage.close(context); }
}

/** Submit exactly one input per operation ID. Requires current live authority
 * and a store with no unsettled interrupted effect. Retries return the existing
 * admission; Forge's store owns content-identity conflict rejection before delivery. */
export async function deliverFollowUp(harness: Harness, directory: string, launch: Launch, generation: number,
  authorization: Authorization, followUp: FollowUp): Promise<{ admission_id: string; replayed: boolean }> {
  const expected = structuredClone(launch);
  const input = structuredClone(followUp);
  validateFollowUp(input);
  authorization.admitFor(expected, generation);
  const identity = journal(directory);
  requireCondition(identity?.root !== undefined && identity.root !== null && canonical(identity.launch) === canonical(expected), 'runner-identity-conflict');
  const executionContext = withAbortSignal(authorization.signal, context);
  const root = await harness.conversation(identity.root as ConversationId, executionContext);
  requireCondition(root !== undefined, 'runner-recovery-required');
  const prior = await observeFollowUp(directory, expected, input.operation_id);
  if (prior.state === 'accepted') {
    return { admission_id: prior.admission_id, replayed: true };
  }
  await requireNoInterruptedEffects(directory, expected);
  authorization.admitFor(expected, generation);
  const submission = await root.submit({ type: 'input', requestId: followUpRequestId(input.operation_id), content: input.content }, executionContext);
  return { admission_id: String(submission.id), replayed: false };
}
