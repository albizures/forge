import { DatabaseSync } from 'node:sqlite';
import { lstatSync, readFileSync, realpathSync, openSync, closeSync } from 'node:fs';
import { join, relative, isAbsolute } from 'node:path';
import { createHash } from 'node:crypto';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { openNodeSqliteDatabase } from '@earendil-works/pi-durable/storage/sqlite/node';
import type { Storage, ConversationId } from '@earendil-works/pi-durable';
import { canonical, requireCondition, type Launch } from './contract.ts';

export const databasePath = (directory: string) => join(directory, 'runtime.sqlite');
function exists(path: string): boolean {
  try { lstatSync(path); return true; }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false; throw error; }
}
export function protectedPath(path: string, directory = false): void {
  const stat = lstatSync(path);
  requireCondition((directory ? stat.isDirectory() : stat.isFile()) && stat.uid === process.getuid!() && (stat.mode & 0o777) === (directory ? 0o700 : 0o600) && (directory || stat.nlink === 1), 'runner-storage-ownership-invalid');
  requireCondition(realpathSync(path) === path, 'runner-storage-path-invalid');
}
export function validateStorageDirectory(directory: string): void {
  requireCondition(process.platform === 'linux' && process.version === 'v24.18.0', 'runner-host-incompatible');
  protectedPath(directory, true);
  for (const name of ['runtime.sqlite', 'runtime.sqlite-wal', 'runtime.sqlite-shm']) {
    if (exists(join(directory, name))) protectedPath(join(directory, name));
  }
}
export function validateDirectory(directory: string, worktree: string): void {
  validateStorageDirectory(directory);
  requireCondition(isAbsolute(worktree) && realpathSync(worktree) === worktree, 'runner-worktree-invalid');
  const nested = (a: string, b: string) => { const rel = relative(a, b); return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel)); };
  requireCondition(!nested(worktree, directory) && !nested(directory, worktree), 'runner-storage-in-worktree');
}
export function journal(directory: string): { launch: Launch; root: number | null } | undefined {
  const path = databasePath(directory);
  if (!exists(path)) return undefined;
  protectedPath(path);
  const db = new DatabaseSync(path, { readOnly: true });
  try {
    const row = db.prepare('SELECT launch, root FROM forge_runner_identity WHERE singleton=1 AND version=1').get();
    requireCondition(row && typeof row.launch === 'string', 'runner-journal-incompatible');
    return { launch: JSON.parse(row.launch), root: row.root as number | null };
  } finally { db.close(); }
}
export function persistIntent(directory: string, launch: Launch): void {
  requireCondition(!exists(databasePath(directory)), 'runner-recovery-required');
  // Atomic exclusive creation also refuses dangling links and unexpected files.
  // Any interrupted initialization remains retained, never reset/repaired here.
  closeSync(openSync(databasePath(directory), 'wx', 0o600));
  const db = new DatabaseSync(databasePath(directory));
  try {
    db.exec('PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; BEGIN IMMEDIATE; CREATE TABLE forge_runner_identity(singleton INTEGER PRIMARY KEY CHECK(singleton=1), version INTEGER NOT NULL, launch TEXT NOT NULL, root INTEGER);');
    db.prepare('INSERT INTO forge_runner_identity VALUES(1,1,?,NULL)').run(canonical(launch));
    db.exec('COMMIT');
  } finally { db.close(); }
}
export function persistRoot(directory: string, root: number): void {
  const db = new DatabaseSync(databasePath(directory));
  try { db.prepare('UPDATE forge_runner_identity SET root=? WHERE singleton=1 AND root IS NULL').run(root); }
  finally { db.close(); }
}
export async function readStorage(directory: string): Promise<Storage> {
  const derived = new URL('./forge-readonly-storage.js', import.meta.resolve('@earendil-works/pi-durable/storage/sqlite/node'));
  requireCondition(createHash('sha256').update(readFileSync(derived)).digest('hex') === 'b7801ce1290d4fc33d95848fbab5e8981fb2b47e9497650f5049c00c474c62aa', 'runner-reader-incompatible');
  const { SqliteStorage } = await import(derived.href);
  const db = await openNodeSqliteDatabase(databasePath(directory), { mode: 'read-only' });
  // Physical read-only SQLite is the enforcement boundary, in addition to the
  // derived adapter refusing initialization, commit and ID allocation.
  return SqliteStorage.open(db);
}
export async function observe(directory: string, assignment: string, runtime: string, operation: string) {
  const identity = journal(directory);
  if (!identity) return { state: 'unknown' };
  requireCondition(identity.launch.assignment_id === assignment && identity.launch.runtime_id === runtime && identity.launch.operation_id === operation, 'runner-identity-conflict');
  if (identity.root === null) return { state: 'unknown' };
  const storage = await readStorage(directory);
  try {
    const submission = await storage.submissionByRequest(identity.root as ConversationId, operation, context);
    return submission ? { state: 'accepted', admission_id: String(submission.id) } : { state: 'unknown' };
  } finally { await storage.close(context); }
}
/** Committed answer observation only: never create a Harness or resume work.
 * A settled passive write, missing input, failed run or idle scheduler is not an
 * answer. Caller supplies the exact retained launch and accepted admission ID.
 * This result is not a Forge Outcome, execution permission or cleanup evidence.
 */
export async function observeAnswer(directory: string, launch: Launch, admissionId: string): Promise<'answered' | 'unanswered' | 'unknown'> {
  requireCondition(typeof admissionId === 'string' && admissionId.length > 0, 'runner-admission-invalid');
  const expected = structuredClone(launch);
  const identity = journal(directory);
  if (!identity) return 'unknown';
  requireCondition(canonical(identity.launch) === canonical(expected), 'runner-identity-conflict');
  if (identity.root === null) return 'unknown';
  const storage = await readStorage(directory);
  try {
    const submission = await storage.submissionByRequest(identity.root as ConversationId, expected.operation_id, context);
    if (!submission) return 'unknown';
    requireCondition(String(submission.id) === admissionId && submission.type === 'input', 'runner-admission-conflict');
    if (submission.status === 'unanswered') return 'unanswered';
    return submission.status === 'done' && Number.isSafeInteger(submission.answer) ? 'answered' : 'unknown';
  } finally { await storage.close(context); }
}

export async function snapshot(directory: string) {
  const identity = journal(directory);
  if (!identity || identity.root === null) return { identity, entries: [] };
  const storage = await readStorage(directory);
  try {
    const page = await storage.scanEntries({ conversationId: identity.root as ConversationId }, 100, undefined, context);
    return { identity, entries: page.items, truncated: page.next !== undefined };
  } finally { await storage.close(context); }
}

export interface TranscriptEntry { id: string; kind: string; by_task_id: string | null; content: string; truncated: boolean }
export interface TranscriptPage { history: 'available' | 'not-started'; entries: TranscriptEntry[]; next_after: string | null; more: boolean }
export const TRANSCRIPT_ENTRY_BYTES = 8192;
/** Committed history of the Assignment root only, ascending by immutable entry ID.
 * `after` is the exclusive last entry ID the caller already holds; later appends
 * always have larger IDs, so a live writer cannot reorder or duplicate a page.
 * Never creates a Harness, repairs storage or opens it writable. A missing journal
 * or root is not-started, never an empty successful transcript of an existing run.
 */
export async function observeTranscript(directory: string, launch: Launch, after: string | null, limit: number): Promise<TranscriptPage> {
  requireCondition(Number.isInteger(limit) && limit >= 1 && limit <= 100, 'runner-request-invalid');
  requireCondition(after === null || (typeof after === 'string' && /^[1-9][0-9]{0,15}$/.test(after)), 'runner-request-invalid');
  const identity = journal(directory);
  if (!identity || identity.root === null) return { history: 'not-started', entries: [], next_after: after, more: false };
  requireCondition(canonical(identity.launch) === canonical(launch), 'runner-identity-conflict');
  const storage = await readStorage(directory);
  try {
    const root = await storage.conversation(identity.root as ConversationId, context);
    requireCondition(root !== undefined && root.id === identity.root && root.parent === undefined && root.owner === undefined, 'runner-recovery-required');
    const query: { conversationId: ConversationId; order: 'ascending'; minEntryId?: any } = { conversationId: identity.root as ConversationId, order: 'ascending' };
    if (after !== null) query.minEntryId = (Number(after) + 1) as any;
    const page = await storage.scanEntries(query, limit, undefined, context);
    const entries = page.items.map((entry: any): TranscriptEntry => {
      const full = JSON.stringify({ model: entry.model ?? null, data: entry.data ?? null });
      const truncated = Buffer.byteLength(full) > TRANSCRIPT_ENTRY_BYTES;
      const content = truncated ? Buffer.from(full).subarray(0, TRANSCRIPT_ENTRY_BYTES).toString('utf8').replace(/\uFFFD+$/, '') : full;
      return { id: String(entry.id), kind: String(entry.kind), by_task_id: entry.byTaskId === undefined ? null : String(entry.byTaskId), content, truncated };
    });
    for (let i = 1; i < entries.length; i++) requireCondition(Number(entries[i].id) > Number(entries[i - 1].id), 'runner-transcript-order-invalid');
    if (after !== null && entries.length > 0) requireCondition(Number(entries[0].id) > Number(after), 'runner-transcript-order-invalid');
    return { history: 'available', entries, next_after: entries.length ? entries[entries.length - 1].id : after, more: page.next !== undefined };
  } finally { await storage.close(context); }
}
