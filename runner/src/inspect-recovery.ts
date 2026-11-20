// Trusted read-only host entrypoint. This reports a runtime precondition, never
// execution permission, process absence or a substitute for Forge authorization.
import { exactKeys, requireCondition, validateLaunch } from './contract.ts';
import { inspectRecoveryCandidate } from './recovery.ts';

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
  exactKeys(request, ['protocol', 'launch', 'admission_id']);
  requireCondition(request.protocol === 1, 'runner-protocol-incompatible');
  validateLaunch(request.launch);
  requireCondition(typeof request.admission_id === 'string' && request.admission_id.length > 0, 'runner-admission-invalid');
  const candidate = await inspectRecoveryCandidate(directory, request.launch, request.admission_id);
  console.log(JSON.stringify({ protocol: 1, launch: request.launch, candidate }));
} catch {
  // Parser, storage, upstream and caller contents never enter diagnostics.
  console.error('runner-recovery-inspection-failed');
  process.exitCode = 1;
}
