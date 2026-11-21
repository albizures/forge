// Trusted same-attempt reopen only, after protected host verification and commit.
// Never submit, continue, create a root, rewrite an agent or repair a worktree.
import { BACKGROUND_CONTEXT as context, withAbortSignal } from '@earendil-works/chord/context';
import { Harness, AgentDoc, createSession, type ConversationId, type HarnessOptions, type SubmissionId } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { Authorization } from './authorization.ts';
import { Denied, canonical, requireCondition, type Launch } from './contract.ts';
import { inspectRecoveryCandidate } from './recovery.ts';
import { databasePath, readStorage, validateDirectory } from './storage.ts';

/** The callback must perform authenticated bootstrap and construct the exact
 * pinned profile's guarded model/tools. No retained API key or capability is
 * accepted as renewed authority. Host ownership must span this entire operation.
 */
export async function resumeAuthorizedRuntime(
  directory: string,
  worktree: string,
  launch: Launch,
  generation: number,
  admissionId: string,
  authorization: Authorization,
  prepare: () => Promise<HarnessOptions>,
): Promise<{ harness: Harness; root: number; admission_id: string }> {
  const expected = structuredClone(launch);
  let harness: Harness | undefined;
  let storage: Awaited<ReturnType<typeof openNodeSqliteStorage>> | undefined;
  try {
    validateDirectory(directory, worktree);
    authorization.beginRecovery(expected, generation);
    const before = await inspectRecoveryCandidate(directory, expected, admissionId);
    const reader = createSession(await readStorage(directory));
    try {
      const agent = await reader.snapshot(AgentDoc, before.root as ConversationId, context);
      requireCondition(agent?.cwd === worktree, 'runner-recovery-binding-invalid');
    } finally { await reader.close(context); }
    const options = await prepare();
    authorization.admitFor(expected, generation);
    const after = await inspectRecoveryCandidate(directory, expected, admissionId);
    requireCondition(canonical(before) === canonical(after), 'runner-recovery-intervention-required');
    authorization.admitFor(expected, generation);
    storage = await openNodeSqliteStorage(databasePath(directory));
    authorization.admitFor(expected, generation);
    const executionContext = withAbortSignal(authorization.signal, context);
    harness = await Harness.open(storage, options, executionContext);
    // Harness.open may reconcile execution tasks, but no root/input is created.
    // Guarded providers/tools remain the actual effect-admission boundaries.
    authorization.admitFor(expected, generation);
    const submission = await harness.submission(Number(admissionId) as SubmissionId, executionContext);
    requireCondition(submission && String(submission.id) === admissionId, 'runner-admission-conflict');
    harness.resume();
    return { harness, root: before.root, admission_id: admissionId };
  } catch {
    authorization.close();
    // Always close storage on failed open without returning upstream/private data.
    try { if (harness) await harness.close(context); else if (storage) await storage.close(context); }
    catch { throw new Denied('runner-recovery-close-failed'); }
    throw new Denied('runner-recovery-failed');
  }
}
