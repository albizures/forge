// Trusted read-only host entrypoint. Never import/open a Harness or discover
// profiles/credentials. Missing storage is unknown, not an empty successful scan.
import { exactKeys, requireCondition, validateLaunch } from './contract.ts';
import { inspectInterruptedEffects } from './interrupted-effects.ts';
import { journal, validateStorageDirectory } from './storage.ts';

try {
  const [directory, ...extra] = process.argv.slice(2);
  requireCondition(typeof directory === 'string' && extra.length === 0, 'runner-request-invalid');
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of process.stdin) {
    size += chunk.length;
    requireCondition(size <= 16384, 'runner-request-invalid');
    chunks.push(chunk);
  }
  const request: unknown = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  exactKeys(request, ['protocol', 'launch']);
  requireCondition(request.protocol === 1, 'runner-protocol-incompatible');
  validateLaunch(request.launch);
  validateStorageDirectory(directory);
  requireCondition(journal(directory) !== undefined, 'runner-recovery-required');
  const effects = await inspectInterruptedEffects(directory, request.launch);
  console.log(JSON.stringify({ protocol: 1, launch: request.launch, effects }));
} catch {
  // No parser/storage errors or upstream output enter ordinary diagnostics.
  console.error('runner-effect-inspection-failed');
  process.exitCode = 1;
}
