import { appendFileSync } from 'node:fs';
import { open, context, fauxAssistantMessage, fauxToolCall } from './fixture.mjs';
const [path, effects] = process.argv.slice(2);
const { root } = await open(path, {
  responses: [fauxAssistantMessage([fauxToolCall('effect', {}, { id: 'effect-1' })], { stopReason: 'toolUse' })],
  tool: async () => {
    appendFileSync(effects, 'mutation\n');
    // Intent is already committed, external effect happened, result is not committed.
    process.send('effect');
    await new Promise(() => {});
  },
});
await root.submit({ type: 'input', content: 'go', requestId: 'crash-input' }, context);
