// An actual Bash tool invokes this controlled external effect. No mocked CLI,
// shell replay, capability bytes in argv, or provider credentials are used here.
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, appendFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { spawnSync } from 'node:child_process';

const file = process.argv[2];
const input = JSON.parse(readFileSync(file, 'utf8'));
assert.equal(process.cwd(), input.bound.executionDirectory);
const directory = dirname(file);
const git = args => {
  const result = spawnSync(input.git, args, { cwd: process.cwd(), env: {}, encoding: 'utf8' });
  assert.equal(result.status, 0, 'controlled Git operation failed');
  return result.stdout.trim();
};
if (input.mode !== 'review') {
  writeFileSync('deliverable.txt', `Supervised Implementation output ${input.bound.assignmentId}\n`);
  git(['add', 'deliverable.txt']);
  git(['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'Controlled output']);
}
appendFileSync(join(directory, 'controlled-effects'), 'effect\n');
const outcome = {
  attempt_id: input.bound.checkAttempt.id,
  generation: input.generation + Number(input.mode === 'rejected'),
  evidence: { ...input.bound.checkAttempt.evidence, commit: git(['rev-parse', 'HEAD']) },
  verdict: input.mode === 'continuation-chain' ? 'fail' : 'pass', actor: 'spoofed-actor', message: 'Supervised output',
  disposition: input.mode === 'continuation-chain' ? { continuation: {
    stopping_reason: 'Controlled finite execution-context boundary', completed_work: 'Committed the first controlled checkpoint',
    verification: 'Controlled checkpoint commit only; successor checks remain unrun', remaining_work: 'accepted-continuation-handoff: finish implementation and independent review',
  } } : 'complete', findings: [], verifications: [],
};
const outcomeFile = join(directory, 'controlled-outcome.json');
writeFileSync(outcomeFile, JSON.stringify(outcome), { mode: 0o600 });
const result = spawnSync(process.env.FORGE_ASSIGNMENT_FORGE_CLI,
  ['assignment', 'outcome', '--file', outcomeFile, '--json'],
  { cwd: process.cwd(), env: { ...input.capability }, encoding: 'utf8' });
if (input.mode === 'rejected') {
  assert.notEqual(result.status, 0);
  assert.ok(result.stderr.includes('stale-generation'));
  process.stderr.write('stale-generation\n');
  process.exitCode = 1;
} else {
  assert.equal(result.status, 0, 'authenticated Outcome failed');
  const accepted = JSON.parse(result.stdout);
  assert.equal(accepted.attempt.id, input.bound.checkAttempt.id);
  process.stdout.write(JSON.stringify(accepted));
}
