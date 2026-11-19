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
export function validateDirectory(directory: string, worktree: string): void {
  requireCondition(process.platform === 'linux' && process.version === 'v24.18.0', 'runner-host-incompatible');
  protectedPath(directory, true);
  requireCondition(isAbsolute(worktree) && realpathSync(worktree) === worktree, 'runner-worktree-invalid');
  const nested = (a: string, b: string) => { const rel = relative(a, b); return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel)); };
  requireCondition(!nested(worktree, directory) && !nested(directory, worktree), 'runner-storage-in-worktree');
  for (const name of ['runtime.sqlite', 'runtime.sqlite-wal', 'runtime.sqlite-shm']) {
    if (exists(join(directory, name))) protectedPath(join(directory, name));
  }
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
export async function snapshot(directory: string) {
  const identity = journal(directory);
  if (!identity || identity.root === null) return { identity, entries: [] };
  const storage = await readStorage(directory);
  try {
    const page = await storage.scanEntries({ conversationId: identity.root as ConversationId }, 100, undefined, context);
    return { identity, entries: page.items, truncated: page.next !== undefined };
  } finally { await storage.close(context); }
}
