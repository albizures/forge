// Recovery of the SHIPPED production Check runner from its real protected
// check-runtime.json, with ONLY the model provider substituted. The original
// admission stopped before its first model generation committed; recovery must
// re-bootstrap, rebuild the guarded tools and resume the same admission.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fauxProvider, fauxAssistantMessage, fauxToolCall } from '@earendil-works/pi-ai';
import { runCheckRunner } from '../src/check-runner.ts';

const [directory, worktree] = process.argv.slice(2);
const test = JSON.parse(readFileSync(join(directory, 'controlled-launch.json'), 'utf8'));
const quote = value => "'" + value.replaceAll("'", "'\\''") + "'";
function substitute() {
  const provider = fauxProvider();
  provider.setResponses([
    transcript => {
      const text = JSON.stringify(transcript);
      assert.ok(text.includes('T07-SUPERVISED-BEHAVIOR'));
      assert.ok(text.includes(test.launch.attempt_id));
      return fauxAssistantMessage(fauxToolCall('bash', { command: [process.execPath, test.effect_script, join(directory, 'controlled-launch.json')].map(quote).join(' ') }), { stopReason: 'toolUse' });
    },
    transcript => {
      const results = transcript.messages.filter(m => m.role === 'toolResult');
      assert.equal(results.length, 1); assert.ok(!results[0].isError);
      return fauxAssistantMessage('Recovered original input; accepted Outcome remains Forge authority.');
    },
  ]);
  return { provider: provider.provider, model: { provider: 'faux', modelId: 'faux-1' } };
}
process.exitCode = await runCheckRunner(directory, worktree, substitute);
