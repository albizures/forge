// Real process-crash fixture, not a supervisor or a Forge authorization proof.
import { readFileSync } from 'node:fs';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { Harness, createRegistry } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { createModels, fauxProvider } from '@earendil-works/pi-ai';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';
import { componentHash } from '../src/contract.ts';

process.umask(0o077);
const directory = process.argv[2];
const launch = { ...JSON.parse(readFileSync(new URL('./wire-v1.json', import.meta.url), 'utf8')).operation.launch, component_hash: componentHash() };
const provider = fauxProvider();
provider.setResponses([async () => {
  // The request has started only after its input and model intent committed.
  process.stdout.write('model-entered\n');
  await new Promise(() => {});
}]);
const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
models.setProvider(provider.provider);
persistIntent(directory, launch);
const storage = await openNodeSqliteStorage(databasePath(directory));
let rootId;
const safe = process.argv[4] === 'safe';
const controlled = new Proxy(storage, {
  get(target, key) {
    if (key === 'commit' && safe) return async (batches, ...args) => {
      if (batches.flat().some(row => row.type === 'task' && row.value.kind === 'pi.generation' && row.value.state.status === 'running')) {
        const submission = await storage.submissionByRequest(rootId, launch.operation_id, context);
        process.stdout.write(`${JSON.stringify({ root: rootId, admission_id: String(submission.id) })}\nmodel-entered\n`);
        await new Promise(() => {}); // fault barrier BEFORE any model admission
      }
      return target.commit(batches, ...args);
    };
    const value = Reflect.get(target, key, target);
    return typeof value === 'function' ? value.bind(target) : value;
  },
});
const harness = await Harness.open(controlled, { models, registry: createRegistry(), env: () => undefined }, context);
const root = await harness.root(context, { agent: { model: { provider: 'faux', modelId: 'faux-1' }, ...(process.argv[3] ? { cwd: process.argv[3] } : {}) } });
rootId = root.id;
persistRoot(directory, root.id);
const submission = await root.submit({ type: 'input', requestId: launch.operation_id, content: 'Controlled pending input' }, context);
if (!safe) process.stdout.write(`${JSON.stringify({ root: root.id, admission_id: String(submission.id) })}\n`);
// Keep the fixture alive until the parent explicitly kills it, not process-idle.
setInterval(() => {}, 60_000);
await submission.wait(context);
