// Test-only composed daemon/store/host/consumer fixture. Not a runner profile,
// production entrypoint, live provider or authority issuer.
import { createInterface } from 'node:readline';
import { readFileSync, readlinkSync } from 'node:fs';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { createModels, fauxProvider, fauxAssistantMessage, fauxToolCall } from '@earendil-works/pi-ai';
import { Harness, createRegistry, defineExtension, defineTool } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { Authorization, guardProvider, guardTool } from '../src/authorization.ts';

process.umask(0o077);
const [directory] = process.argv.slice(2);
const mode = readFileSync(join(directory, 'controlled-mode'), 'utf8');
const emit = value => console.log(JSON.stringify(value));
const lines = createInterface({ input: process.stdin });
let authorization, harness, child, started = false, requests = 0, tools = 0, cancellations = 0, childExit;
process.on('SIGTERM', () => authorization?.close());

async function execute() {
  const provider = fauxProvider();
  provider.setResponses([mode === 'model' ? (_input, options) => new Promise((_resolve, reject) => {
    requests++;
    options.signal.addEventListener('abort', () => { cancellations++; emit({ event: 'cancelled' }); reject(options.signal.reason); }, { once: true });
    emit({ event: 'started', mode, requests });
  }) : () => { requests++; return fauxAssistantMessage([fauxToolCall('effect', {}, { id: 'controlled' })], { stopReason: 'toolUse' }); }]);
  const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
  models.setProvider(guardProvider(provider.provider, authorization));
  const registry = createRegistry();
  if (mode === 'child') registry.install(defineExtension({ name: 'controlled', tools: [defineTool({
    name: 'effect', description: 'Controlled cancellable test child', parameters: { type: 'object', properties: {} }, replay: 'unsafe',
    execute: guardTool(async (_args, _api, ctx) => {
      tools++;
      child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { env: {}, stdio: 'ignore' });
      const closed = once(child, 'close');
      ctx.abortSignal.addEventListener('abort', () => { cancellations++; emit({ event: 'cancelled' }); child.kill('SIGTERM'); }, { once: true });
      await once(child, 'spawn');
      emit({ event: 'started', mode, requests, tools, child: child.pid });
      const [, signal] = await closed; childExit = signal;
      return {};
    }, authorization),
  })] }));
  harness = await Harness.open(await openNodeSqliteStorage(join(directory, 'controlled.sqlite')), { models, registry, settings: { retry: { enabled: false }, compaction: { enabled: false } }, env: () => undefined }, context);
  const root = await harness.root(context, { agent: { model: { provider: 'faux', modelId: 'faux-1' } } });
  await (await root.submit({ type: 'input', content: 'controlled effect' }, context)).wait(context);
  // Actual pinned scheduling is asked again after cancellation; the boundary
  // must reject it without another provider request or tool effect.
  await (await root.submit({ type: 'input', content: 'must not execute' }, context)).wait(context);
  emit({ event: 'post-expiry', requests, tools, cancellations, child_exit: childExit ?? null });
}

try {
  let evidence;
  for await (const line of lines) {
    if (!evidence) {
      if (line !== '{"host_ready":true}') throw new Error('handshake');
      evidence = JSON.parse(readFileSync(join(directory, 'owner.json'), 'utf8'));
      emit({ event: 'owned', identity: evidence });
      continue;
    }
    const control = JSON.parse(line);
    const lease = control.lease;
    if (!authorization) authorization = new Authorization(lease.launch, lease.generation, {
      boot_id: evidence.owner.boot_id, time_namespace: readlinkSync('/proc/self/ns/time'),
      host_pid: evidence.owner.host_pid, host_start: evidence.owner.host_start,
      worker_pid: process.pid, worker_start: evidence.worker_start,
      lock_device: evidence.owner.lock_device, lock_inode: evidence.owner.lock_inode,
    }, lease.daemon_epoch);
    authorization.accept(lease);
    emit({ event: 'authorized', sequence: lease.sequence });
    if (!started) { started = true; void execute().catch(() => emit({ event: 'fixture-failed' })); }
  }
} finally {
  authorization?.close();
  child?.kill('SIGKILL');
  if (harness) await harness.close(context);
  lines.close();
}
