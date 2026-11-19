// Test-only component driver. The lease is a protocol fixture, NOT a grant from
// the supervisor. The optional execution branch composes a real Harness and
// actual coding tool with a deterministic fake provider, NOT supervised dispatch.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { BACKGROUND_CONTEXT as context, withAbortSignal } from '@earendil-works/chord/context';
import { Harness } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { createModels, fauxProvider, fauxAssistantMessage, fauxToolCall } from '@earendil-works/pi-ai';
import { Authorization, guardProvider } from '../src/authorization.ts';
import { createCheckRuntime } from '../src/check-runtime.ts';
import { bootstrapCheck } from '../src/bootstrap.ts';
import { componentHash } from '../src/contract.ts';
import { databasePath, observeAnswer, persistIntent, persistRoot, validateDirectory } from '../src/storage.ts';

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
    const runtime = await createCheckRuntime(launch, generation, input.bound, input.capability, config, input.execution.key, authorization);
    // Only this test composition replaces the selected model. The real guarded
    // coding tool, explicit environment/cwd and authenticated CLI are unchanged.
    const model = fauxProvider();
    const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
    models.setProvider(guardProvider(model.provider, authorization));
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
    const executionContext = withAbortSignal(authorization.signal, context);
    authorization.admitFor(launch, generation);
    persistIntent(directory, launch);
    const harness = await Harness.open(await openNodeSqliteStorage(databasePath(directory)), { ...runtime.options, models }, executionContext);
    try {
      authorization.admitFor(launch, generation);
      const root = await harness.root(executionContext, { agent: { ...runtime.agent, model: { provider: 'faux', modelId: 'faux-1' } } });
      persistRoot(directory, root.id);
      authorization.admitFor(launch, generation);
      const submission = await root.submit({ type: 'input', requestId: launch.operation_id, content: 'Execute only the bound Check.' }, executionContext);
      assert.equal((await submission.wait(context)).status, 'done');
      executionReport = { admission: { state: 'accepted', admission_id: String(submission.id) } };
      assert.equal(verifiedToolResult, submits);
      assert.equal(model.state.callCount, submits ? 2 : 1);
      assert.equal(model.getPendingResponseCount(), 0);
    } finally { await harness.close(context); }
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
