// Bounded committed answer observation, never a Harness or execution admission.
import { exactKeys, requireCondition, validateLaunch } from './contract.ts';
import { observeAnswer, validateStorageDirectory } from './storage.ts';
try {
  const [directory, ...extra] = process.argv.slice(2);
  requireCondition(extra.length === 0, 'runner-request-invalid');
  const chunks: Buffer[] = []; let size = 0;
  for await (const chunk of process.stdin) {
    size += chunk.length; requireCondition(size <= 16384, 'runner-request-invalid'); chunks.push(chunk);
  }
  const request = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  exactKeys(request, ['protocol', 'launch', 'admission_id']);
  requireCondition(request.protocol === 1, 'runner-request-invalid'); validateLaunch(request.launch);
  validateStorageDirectory(directory);
  console.log(JSON.stringify({ protocol: 1, launch: request.launch,
    answer: await observeAnswer(directory, request.launch, request.admission_id) }));
} catch { console.error('runner-answer-inspection-failed'); process.exitCode = 1; }
