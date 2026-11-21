// Research fixture only: synthetic provider, no network or ambient configuration.
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { createModels, fauxProvider, fauxAssistantMessage, fauxText, fauxToolCall } from '@earendil-works/pi-ai';
import { Harness, createRegistry, defineExtension, defineTool } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { obj } from '@earendil-works/pi-schema';
import { openReadOnlyStorage } from './readonly-storage.mjs';
import { guardProvider, guardTool } from './authorization.mjs';
import { createCodingTools } from '@earendil-works/pi-durable/tools';
export { context, fauxAssistantMessage, fauxText, fauxToolCall };
export async function open(path, { readOnly = false, physicalReadOnly = readOnly, responses = [], tool, hooks = [], reader = false, audit, lease, settings, coding = false, agent = {}, env } = {}) {
  const faux = fauxProvider();
  faux.setResponses(responses);
  const models = createModels();
  models.setProvider(lease ? guardProvider(faux.provider, lease) : faux.provider);
  const registry = createRegistry();
  if (hooks.length) registry.install(defineExtension({ name: 'hooks', hooks }));
  if (coding) registry.install(defineExtension({ name: 'coding-probe', tools: createCodingTools().tools.map(tool => ({
    ...tool, replay: 'unsafe', execute: lease ? guardTool(tool.execute, lease) : tool.execute,
  })) }));
  if (tool) registry.install(defineExtension({ name: 'probe', tools: [defineTool({
    name: 'effect', description: 'Synthetic external mutation', parameters: obj({}),
    replay: 'unsafe', execute: lease ? guardTool(tool, lease) : tool,
  })] }));
  const storage = reader ? await openReadOnlyStorage(path, audit) :
    await openNodeSqliteStorage(path, physicalReadOnly ? { mode: 'read-only' } : {});
  const harness = await Harness.open(storage, { models, registry, readOnly, settings, env: env ? () => env : undefined }, context);
  const root = readOnly ? await harness.conversation(1, context) :
    await harness.root(context, { agent: { model: { provider: 'faux', modelId: 'faux-1' }, ...agent } });
  return { harness, root, storage };
}
