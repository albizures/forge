// Trusted read-only host entrypoint for operator transcript pages. Never opens a
// Harness, discovers profiles/credentials or creates runtime files. Unavailable
// history is reported as a bounded code, not as an empty or repaired transcript.
import { Denied, exactKeys, requireCondition, validateLaunch } from './contract.ts';
import { observeTranscript, validateStorageDirectory } from './storage.ts';

const reportable = new Set(['runner-recovery-required', 'runner-identity-conflict', 'runner-storage-ownership-invalid', 'runner-storage-path-invalid', 'runner-reader-incompatible', 'runner-journal-incompatible', 'runner-transcript-order-invalid']);
try {
  const [directory, ...extra] = process.argv.slice(2);
  requireCondition(typeof directory === 'string' && extra.length === 0, 'runner-request-invalid');
  const chunks: Buffer[] = []; let size = 0;
  for await (const chunk of process.stdin) {
    size += chunk.length; requireCondition(size <= 16384, 'runner-request-invalid'); chunks.push(chunk);
  }
  const request: unknown = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  exactKeys(request, ['protocol', 'launch', 'after', 'limit']);
  requireCondition(request.protocol === 1, 'runner-protocol-incompatible');
  validateLaunch(request.launch);
  requireCondition(Number.isInteger(request.limit) && request.limit >= 1 && request.limit <= 100, 'runner-request-invalid');
  requireCondition(request.after === null || (typeof request.after === 'string' && /^[1-9][0-9]{0,15}$/.test(request.after)), 'runner-request-invalid');
  let outcome;
  try {
    validateStorageDirectory(directory);
    outcome = { protocol: 1, launch: request.launch, ...(await observeTranscript(directory, request.launch, request.after, request.limit)) };
  } catch (error) {
    // Only fixed, non-secret codes cross the boundary. Corrupt SQLite and upstream
    // errors collapse to one generic unavailable code.
    const code = error instanceof Denied && reportable.has(error.code) ? error.code : 'runner-history-unavailable';
    outcome = { protocol: 1, launch: request.launch, history: 'unavailable', code, entries: [], next_after: request.after, more: false };
  }
  console.log(JSON.stringify(outcome));
} catch { console.error('runner-transcript-inspection-failed'); process.exitCode = 1; }
