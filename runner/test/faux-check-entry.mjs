// Positive control for check-entry.test.mjs: the shipped runner with ONLY a faux
// provider substituted (one final text answer, no tool calls).
import { fauxProvider, fauxAssistantMessage } from '@earendil-works/pi-ai';
import { runCheckRunner } from '../src/check-runner.ts';

const [directory, worktree] = process.argv.slice(2);
process.exitCode = await runCheckRunner(directory, worktree, () => {
  const model = fauxProvider();
  model.setResponses([fauxAssistantMessage('Prose is not a Forge Outcome.')]);
  return { provider: model.provider, model: { provider: 'faux', modelId: 'faux-1' } };
});
