// Controlled process fixture for the production-composed supervised tracer.
// Runs the SHIPPED production Check runner (src/check-runner.ts, protected
// check-runtime.json written by the daemon's production launcher, real owner,
// lease, bootstrap, profile, model-key loading, guarded coding tools and
// storage) with ONLY the model provider substituted by a deterministic faux
// model. The daemon composes this file as its runner entry, so the same fixture
// serves the first admission and a same-attempt recovery incarnation.
//
// Scripted test data: `<Workspace root>/controlled-tracer.json` (mode, Git and
// effect script) is fixture-wide; this worker combines it with the protected
// input into `controlled-launch.json` for the controlled effect script. The
// shipped runner never reads either file.
import assert from 'node:assert/strict';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { createServer } from 'node:net';
import { once } from 'node:events';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { SqliteStorage } from '@earendil-works/pi-durable/storage/sqlite';
import { fauxProvider, fauxAssistantMessage, fauxToolCall } from '@earendil-works/pi-ai';
import { runCheckRunner } from '../src/check-runner.ts';
import { journal } from '../src/storage.ts';

const [directory, worktree] = process.argv.slice(2);
// <root>/.forge/assignment-runtimes/<digest>
const root = dirname(dirname(dirname(directory)));
const fixture = JSON.parse(readFileSync(join(root, 'controlled-tracer.json'), 'utf8'));
const input = JSON.parse(readFileSync(join(directory, 'check-runtime.json'), 'utf8'));
const launchFile = join(directory, 'controlled-launch.json');
const boundaryFile = join(directory, 'controlled-recovery-boundary.json');
const recovering = existsSync(boundaryFile);
const test = {
  launch: input.launch, bound: input.bound, generation: input.generation,
  capability: input.capability, mode: fixture.mode, git: fixture.git, effect_script: fixture.effect_script,
};
if (!existsSync(launchFile)) writeFileSync(launchFile, JSON.stringify(test), { mode: 0o600 });
const quote = value => "'" + value.replaceAll("'", "'\\''") + "'";
const effect = () => fauxAssistantMessage(fauxToolCall('bash', {
  command: [process.execPath, test.effect_script, launchFile].map(quote).join(' '),
}), { stopReason: 'toolUse' });

// Test synchronization only: the first model response waits until the Rust
// driver has inspected the bootstrapped state. It cannot create authority.
// Linux abstract socket: canonical runtime paths may exceed sockaddr_un limits.
const barrier = createServer();
const released = new Promise(resolve => barrier.once('connection', socket => {
  socket.once('data', data => { assert.equal(String(data), 'start\n'); socket.end(); resolve(); });
}));
barrier.listen(`\0forge-t07-start-${process.pid}`);
await once(barrier, 'listening');

// `packaged` (T24) crashes at the same boundary, then recovers after a Revision switch.
if ((test.mode === 'recovered' || test.mode === 'packaged') && !recovering) {
  // Fault injection at the pinned upstream storage class, in this test process
  // only: stop forever BEFORE the first model generation is committed as running.
  // The boundary is reported out of band; stdout stays strictly protocol 1.
  const commit = SqliteStorage.prototype.commit;
  SqliteStorage.prototype.commit = async function (batches, ...rest) {
    if (batches.flat().some(row => row.type === 'task' && row.value.kind === 'pi.generation' && row.value.state.status === 'running')) {
      const root = journal(directory).root;
      const submission = await this.submissionByRequest(root, test.launch.operation_id, context);
      writeFileSync(boundaryFile, JSON.stringify({ root, admission_id: String(submission.id) }), { mode: 0o600 });
      await new Promise(() => {});
    }
    return commit.call(this, batches, ...rest);
  };
}

function recovered() {
  const model = fauxProvider();
  model.setResponses([
    transcript => {
      const text = JSON.stringify(transcript);
      assert.ok(text.includes('T07-SUPERVISED-BEHAVIOR'));
      assert.ok(text.includes(test.launch.attempt_id));
      return effect();
    },
    transcript => {
      const results = transcript.messages.filter(m => m.role === 'toolResult');
      assert.equal(results.length, 1); assert.ok(!results[0].isError);
      return fauxAssistantMessage('Recovered original input; accepted Outcome remains Forge authority.');
    },
  ]);
  return { provider: model.provider, model: { provider: 'faux', modelId: 'faux-1' } };
}

function substitute() {
  if (recovering) return recovered();
  const model = fauxProvider();
  const answer = () => fauxAssistantMessage('Plausible completion prose is not an accepted Forge Outcome.');
  // A recorded operator follow-up joins the same conversation; prose only.
  const followUp = transcript => {
    assert.ok(JSON.stringify(transcript).includes('T23-FOLLOWUP'));
    return fauxAssistantMessage('Follow-up acknowledged; no Outcome change.');
  };
  model.setResponses(test.mode === 'missing' ? [async () => { await released; return answer(); }, followUp] : [
    async transcript => {
      await released;
      const text = JSON.stringify(transcript);
      assert.ok(text.includes('T07-SUPERVISED-BEHAVIOR'));
      assert.ok(text.includes(test.launch.attempt_id));
      if (test.mode === 'chain-complete') {
        assert.ok(test.bound.checkAttempt.continuation);
        assert.ok(text.includes('accepted-continuation-handoff'));
      }
      if (test.mode === 'review') assert.equal(test.bound.checkAttempt.continuation, null);
      assert.ok(!text.includes(test.capability.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE));
      return effect();
    },
    transcript => {
      const results = transcript.messages.filter(message => message.role === 'toolResult');
      assert.equal(results.length, 1);
      assert.equal(Boolean(results[0].isError), test.mode === 'rejected');
      assert.ok(JSON.stringify(results[0]).includes(test.mode === 'rejected' ? 'stale-generation' : test.launch.assignment_id));
      return answer();
    },
  ]);
  return { provider: model.provider, model: { provider: 'faux', modelId: 'faux-1' } };
}

try {
  process.exitCode = await runCheckRunner(directory, worktree, substitute);
} finally { barrier.close(); }
