// Matched Check recovery entrypoint. No model fallback or ambient discovery.
import { readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { createCheckRuntime } from './check-runtime.ts';
import { canonical, exactKeys, requireCondition } from './contract.ts';
import { serveRecovery } from './recovery-consumer.ts';
import { protectedPath } from './storage.ts';

process.umask(0o077);
const [directory, worktree] = process.argv.slice(2);
try {
  // Trusted host writes this owner-only original-binding snapshot, not a model,
  // follow-up, operator RPC payload or environment value containing credentials.
  const path = join(directory, 'check-runtime.json');
  protectedPath(path);
  requireCondition(statSync(path).size <= 1024 * 1024, 'assignment-runtime-failed');
  const input = JSON.parse(readFileSync(path, 'utf8'));
  exactKeys(input, ['bound', 'capability', 'configuration', 'credential_file']);
  await serveRecovery(directory, worktree, async (candidate, authorization) => {
    requireCondition(canonical(input.bound.checkAttempt) === canonical(candidate.attempt), 'assignment-runtime-failed');
    const runtime = await createCheckRuntime(candidate.launch, candidate.attempt.generation, input.bound,
      input.capability, input.configuration, input.credential_file, authorization, directory);
    return runtime.options;
  });
} catch {
  // Runtime/private-file details and credentials never enter ordinary output.
  console.error('runner-recovery-failed');
  process.exitCode = 1;
}
