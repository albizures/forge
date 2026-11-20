// Real process-crash fixture, not a supervisor or a Forge authorization proof.
import { readFileSync } from 'node:fs';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { Harness, createRegistry } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { createModels, fauxProvider } from '@earendil-works/pi-ai';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';

process.umask(0o077);
const directory = process.argv[2];
const launch = JSON.parse(readFileSync(new URL('./wire-v1.json', import.meta.url), 'utf8')).operation.launch;
const provider = fauxProvider();
provider.setResponses([async () => {
  // The request has started only after its input and model intent committed.
  process.stdout.write('model-entered\n');
  await new Promise(() => {});
}]);
const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
models.setProvider(provider.provider);
persistIntent(directory, launch);
const harness = await Harness.open(await openNodeSqliteStorage(databasePath(directory)), { models, registry: createRegistry(), env: () => undefined }, context);
const root = await harness.root(context, { agent: { model: { provider: 'faux', modelId: 'faux-1' } } });
persistRoot(directory, root.id);
const submission = await root.submit({ type: 'input', requestId: launch.operation_id, content: 'Controlled pending input' }, context);
process.stdout.write(`${JSON.stringify({ root: root.id, admission_id: String(submission.id) })}\n`);
// Keep the fixture alive until the parent explicitly kills it, not process-idle.
setInterval(() => {}, 60_000);
await submission.wait(context);
