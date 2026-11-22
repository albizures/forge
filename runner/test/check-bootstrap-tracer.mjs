// Test-only component driver. The lease is a protocol fixture, NOT a grant from
// the supervisor. The optional execution branch runs the shipped first-admission
// composition (src/check-runner.ts::admitCheck) with only a deterministic fake
// provider substituted. It is not supervised dispatch.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { fauxProvider, fauxAssistantMessage, fauxToolCall } from '@earendil-works/pi-ai';
import { Authorization } from '../src/authorization.ts';
import { admitCheck, validateCheckRuntime } from '../src/check-runner.ts';
import { bootstrapCheck } from '../src/bootstrap.ts';
import { componentHash } from '../src/contract.ts';
import { observeAnswer, validateDirectory } from '../src/storage.ts';

const input = JSON.parse(readFileSync(0, 'utf8'));
const fixture = JSON.parse(readFileSync(new URL('./authorization-lease.json', import.meta.url), 'utf8'));
const launch = input.execution?.launch ?? { ...fixture.launch, assignment_id: input.bound.assignmentId, attempt_id: input.bound.checkAttempt.id, profile_revision: input.bound.profileRevision };
const generation = input.bound.checkAttempt.generation;
const authorization = new Authorization(launch, generation, fixture.owner, fixture.daemon_epoch, () => 1000);
authorization.accept({ ...fixture, launch, generation, sequence: 1, issued_boottime_millis: 1000, expires_boottime_millis: 31000 });
let executionReport;
try {
  if (input.execution) {
    assert.equal(launch.component_hash, componentHash());
    const directory = dirname(input.execution.storage);
    validateDirectory(directory, input.bound.executionDirectory);
    const config = input.execution.configuration;
    assert.equal(config.cwd, input.bound.executionDirectory);
    assert.deepEqual(config.tools, ['read', 'write', 'edit', 'bash']);
    // The SHIPPED first-admission composition (bootstrap, profile, model-key
    // loading, guarded coding tools, storage); only the model provider is swapped.
    const check = validateCheckRuntime({ version: 1, launch, generation, bound: input.bound, configuration: config,
      credential_file: input.execution.key, capability: input.capability }, input.bound.executionDirectory);
    const model = fauxProvider();
    const quote = value => "'" + value.replaceAll("'", "'\\''") + "'";
    const submits = input.execution.mode !== 'missing';
    let verifiedToolResult = false;
    model.setResponses(submits ? [
      transcript => {
        assert.ok(JSON.stringify(transcript).includes('T07-BOUND-BEHAVIOR'));
        assert.ok(JSON.stringify(transcript).includes(input.bound.checkAttempt.id));
        assert.ok(!JSON.stringify(transcript).includes('T07-CAPABILITY-CANARY'));
        return fauxAssistantMessage(fauxToolCall('bash', { command: `"$FORGE_ASSIGNMENT_FORGE_CLI" assignment outcome --file ${quote(input.execution.outcome)} --json` }), { stopReason: 'toolUse' });
      },
      transcript => {
        const results = transcript.messages.filter(message => message.role === 'toolResult');
        assert.equal(results.length, 1);
        if (input.execution.mode === 'rejected') {
          assert.ok(results[0].isError);
          assert.ok(JSON.stringify(results[0]).includes('stale-generation'));
        } else {
          assert.ok(!results[0].isError);
          assert.ok(JSON.stringify(results[0]).includes(input.bound.assignmentId));
        }
        verifiedToolResult = true;
        return fauxAssistantMessage('Runtime prose is not a Forge Outcome.');
      },
    ] : [fauxAssistantMessage('Implementation complete. All work looks good. No Outcome was submitted.')]);
    const admitted = await admitCheck(check, authorization, directory, () => {},
      () => ({ provider: model.provider, model: { provider: 'faux', modelId: 'faux-1' } }));
    try {
      assert.equal(await admitted.settled, 'done');
      executionReport = { admission: { state: 'accepted', admission_id: admitted.admission_id } };
      assert.equal(verifiedToolResult, submits);
      assert.equal(model.state.callCount, submits ? 2 : 1);
      assert.equal(model.getPendingResponseCount(), 0);
    } finally { await admitted.harness.close(context); }
    // Physical read-only inspection after the writer stops, not a settled-event
    // assertion or a model-supplied verdict. No scheduling is started here.
    executionReport.answer = await observeAnswer(directory, launch, executionReport.admission.admission_id);
    assert.equal(executionReport.answer, 'answered');
  } else {
    const result = await bootstrapCheck(launch, generation, input.bound, input.capability, authorization);
    assert.equal(result.behavior, 'T07-BOUND-BEHAVIOR');
    assert.equal(result.context.record.id, input.bound.recordId);
  }
  assert.equal(input.expected, 'accepted');
  authorization.admit();
  // Behavior-bearing context is deliberately not printed.
  process.stdout.write(JSON.stringify({ result: 'accepted', execution: executionReport }));
} catch (error) {
  assert.equal(input.expected, 'denied');
  assert.equal(error.code, 'assignment-bootstrap-failed');
  assert.throws(() => authorization.admit(), { code: 'assignment-lease-denied' });
  process.stdout.write(JSON.stringify({ result: 'denied' }));
} finally {
  authorization.close();
}
