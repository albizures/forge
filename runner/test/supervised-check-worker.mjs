// Controlled T07 process fixture: real owner/lease/bootstrap/Harness/coding tools,
// deterministic model only. Never selected by production adapter wiring.
import assert from 'node:assert/strict';
import { createInterface } from 'node:readline';
import { readFileSync, existsSync } from 'node:fs';
import { readlinkSync } from 'node:fs';
import { join } from 'node:path';
import { createServer } from 'node:net';
import { once } from 'node:events';
import { BACKGROUND_CONTEXT as context, withAbortSignal } from '@earendil-works/chord/context';
import { Harness } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { createModels, fauxProvider, fauxAssistantMessage, fauxToolCall } from '@earendil-works/pi-ai';
import { Authorization, guardProvider } from '../src/authorization.ts';
import { createCheckRuntime } from '../src/check-runtime.ts';
import { compatibility, canonical, exactKeys, requireCondition, validateRequest } from '../src/contract.ts';
import { ownerIdentity } from '../src/owner.ts';
import { databasePath, journal, persistIntent, persistRoot, validateDirectory } from '../src/storage.ts';

process.umask(0o077);
const [directory, worktree] = process.argv.slice(2);
validateDirectory(directory, worktree);
const input = JSON.parse(readFileSync(join(directory, 'controlled-launch.json'), 'utf8'));
validateRequest(input.request);
const launch = input.request.operation.launch;
requireCondition(input.request.operation.kind === 'admit', 'runner-request-invalid');
requireCondition(!journal(directory), 'runner-recovery-unavailable');
const emit = value => console.log(JSON.stringify(value));
const lines = createInterface({ input: process.stdin });
let authorization, evidence, harness, executing = false, calls = 0, tools = 0;
const barrier = createServer();
const released = new Promise(resolve => barrier.once('connection', socket => {
  socket.once('data', data => { assert.equal(String(data), 'start\n'); socket.end(); resolve(); });
}));
// Linux abstract socket: canonical runtime paths may exceed sockaddr_un limits.
// This is only a test barrier, not an execution credential or production control.
barrier.listen(`\0forge-t07-start-${process.pid}`);
await once(barrier, 'listening');

async function execute() {
  try {
    const runtime = await createCheckRuntime(launch, input.generation, input.bound, input.capability,
      input.configuration, input.credential_file, authorization);
    emit({ event: 'bootstrapped', calls, tools });
    // Test synchronization only; this cannot create/extend execution authority.
    await released;
    barrier.close();
    authorization.admitFor(launch, input.generation);
    const model = fauxProvider();
    const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
    models.setProvider(guardProvider(model.provider, authorization));
    const response = () => {
      calls++;
      return fauxAssistantMessage('Plausible completion prose is not an accepted Forge Outcome.');
    };
    const quote = value => "'" + value.replaceAll("'", "'\\''") + "'";
    model.setResponses(input.mode === 'missing' ? [response] : [
      transcript => {
        calls++;
        const text = JSON.stringify(transcript);
        assert.ok(text.includes('T07-SUPERVISED-BEHAVIOR'));
        assert.ok(text.includes(launch.attempt_id));
        assert.ok(!text.includes(input.capability.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE));
        return fauxAssistantMessage(fauxToolCall('bash', {
          command: [process.execPath, input.effect_script, join(directory, 'controlled-launch.json')].map(quote).join(' '),
        }), { stopReason: 'toolUse' });
      },
      transcript => {
        const results = transcript.messages.filter(message => message.role === 'toolResult');
        assert.equal(results.length, 1);
        assert.equal(Boolean(results[0].isError), input.mode === 'rejected');
        const text = JSON.stringify(results[0]);
        assert.ok(text.includes(input.mode === 'rejected' ? 'stale-generation' : launch.assignment_id));
        tools++;
        return response();
      },
    ]);
    const executionContext = withAbortSignal(authorization.signal, context);
    persistIntent(directory, launch);
    harness = await Harness.open(await openNodeSqliteStorage(databasePath(directory)), { ...runtime.options, models }, executionContext);
    authorization.admitFor(launch, input.generation);
    const root = await harness.root(executionContext, { agent: { ...runtime.agent, model: { provider: 'faux', modelId: 'faux-1' } } });
    persistRoot(directory, root.id);
    authorization.admitFor(launch, input.generation);
    const submission = await root.submit({ type: 'input', requestId: launch.operation_id, content: 'Execute the authenticated bound Check only.' }, executionContext);
    emit({ event: 'admission', response: { compatibility: compatibility(), assignment_id: launch.assignment_id,
      runtime_id: launch.runtime_id, operation_id: launch.operation_id,
      admission: { state: 'accepted', admission_id: String(submission.id) } } });
    assert.equal((await submission.wait(context)).status, 'done');
    await harness.close(context);
    harness = undefined;
    assert.equal(model.state.callCount, calls);
    assert.equal(model.getPendingResponseCount(), 0);
    emit({ event: 'settled', calls, tools });
  } catch {
    authorization.close();
    emit({ event: 'runtime-failed', calls, tools });
  }
}

try {
  for await (const line of lines) {
    if (!evidence) {
      assert.equal(line, '{"host_ready":true}');
      evidence = ownerIdentity(directory);
      emit({ event: 'owned', identity: evidence, compatibility: compatibility() });
      continue;
    }
    const control = JSON.parse(line);
    exactKeys(control, ['host_control', 'lease']);
    requireCondition(control.host_control === 1 && !existsSync(join(directory, 'watchdog.json')), 'assignment-lease-denied');
    const lease = control.lease;
    requireCondition(canonical(lease.launch) === canonical(launch) && lease.generation === input.generation, 'assignment-lease-denied');
    if (!authorization) authorization = new Authorization(launch, input.generation, {
      boot_id: evidence.owner.boot_id, time_namespace: readlinkSync('/proc/self/ns/time'),
      host_pid: evidence.owner.host_pid, host_start: evidence.owner.host_start,
      worker_pid: process.pid, worker_start: evidence.worker_start,
      lock_device: evidence.owner.lock_device, lock_inode: evidence.owner.lock_inode,
    }, lease.daemon_epoch);
    authorization.accept(lease);
    emit({ event: 'authorized', sequence: lease.sequence });
    if (!executing) { executing = true; void execute(); }
  }
} catch {
  authorization?.close();
  emit({ event: 'runtime-failed', calls, tools });
} finally {
  authorization?.close();
  barrier.close();
  if (harness) await harness.close(context);
  lines.close();
}
