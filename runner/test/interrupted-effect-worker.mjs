// Component-only worker: fixture lease, real pinned Harness and Bash tool.
import { readFileSync } from 'node:fs';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { Harness } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { createModels, fauxProvider, fauxAssistantMessage, fauxToolCall } from '@earendil-works/pi-ai';
import { Authorization, guardProvider } from '../src/authorization.ts';
import { createProfileRuntime } from '../src/profiles.ts';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';

process.umask(0o077);
const input = JSON.parse(readFileSync(0, 'utf8'));
const grant = JSON.parse(readFileSync(new URL('./authorization-lease.json', import.meta.url)));
const authorization = new Authorization(input.launch, grant.generation, grant.owner, grant.daemon_epoch, () => 1000);
authorization.accept({ ...grant, launch: input.launch, sequence: 1, issued_boottime_millis: 1000, expires_boottime_millis: 31000 });
const runtime = createProfileRuntime({ version: 1, model: 'openai/gpt-4.1', thinkingLevel: 'off', tools: ['bash'], context: { instructions: 'Controlled mutation only.', resources: [] }, cwd: input.worktree }, input.key, authorization);
const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
const model = fauxProvider();
models.setProvider(guardProvider(model.provider, authorization));
const quote = value => "'" + value.replaceAll("'", "'\\''") + "'";
const command = `${quote(process.execPath)} -e ${quote(`fetch(${JSON.stringify(input.endpoint)}, {method:'POST'}).then(r => r.text()).then(t => console.log(t))`)}`;
model.setResponses([
  fauxAssistantMessage(fauxToolCall('bash', { command }), { stopReason: 'toolUse' }),
  fauxAssistantMessage('Done is not a Forge Outcome.'),
]);
persistIntent(input.directory, input.launch);
const harness = await Harness.open(await openNodeSqliteStorage(databasePath(input.directory)), { ...runtime.options, models }, context);
try {
  const root = await harness.root(context, { agent: { ...runtime.agent, model: { provider: 'faux', modelId: 'faux-1' } } });
  persistRoot(input.directory, root.id);
  const submission = await root.submit({ type: 'input', requestId: input.launch.operation_id, content: 'Execute one controlled external mutation.' }, context);
  await submission.wait(context);
} finally {
  await harness.close(context);
  authorization.close();
}
console.log('settled');
