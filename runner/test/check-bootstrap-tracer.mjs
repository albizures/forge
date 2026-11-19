// Test-only component driver. The lease is a protocol fixture, NOT a grant from
// the supervisor. This opens no conversation/storage and makes no model request.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Authorization } from '../src/authorization.ts';
import { bootstrapCheck } from '../src/bootstrap.ts';

const input = JSON.parse(readFileSync(0, 'utf8'));
const fixture = JSON.parse(readFileSync(new URL('./authorization-lease.json', import.meta.url), 'utf8'));
const launch = { ...fixture.launch, assignment_id: input.bound.assignmentId, attempt_id: input.bound.checkAttempt.id, profile_revision: input.bound.profileRevision };
const generation = input.bound.checkAttempt.generation;
const authorization = new Authorization(launch, generation, fixture.owner, fixture.daemon_epoch, () => 1000);
authorization.accept({ ...fixture, launch, generation, sequence: 1, issued_boottime_millis: 1000, expires_boottime_millis: 31000 });
try {
  const result = await bootstrapCheck(launch, generation, input.bound, input.capability, authorization);
  assert.equal(input.expected, 'accepted');
  assert.equal(result.behavior, 'T07-BOUND-BEHAVIOR');
  assert.equal(result.context.record.id, input.bound.recordId);
  authorization.admit();
  // Behavior-bearing context is deliberately not printed.
  process.stdout.write(JSON.stringify({ result: 'accepted' }));
} catch (error) {
  assert.equal(input.expected, 'denied');
  assert.equal(error.code, 'assignment-bootstrap-failed');
  assert.throws(() => authorization.admit(), { code: 'assignment-lease-denied' });
  process.stdout.write(JSON.stringify({ result: 'denied' }));
} finally {
  authorization.close();
}
