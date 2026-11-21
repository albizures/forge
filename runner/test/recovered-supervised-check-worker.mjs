// Actual authenticated bootstrap/coding-tool/Outcome path, fake immediate model.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { createModels, fauxProvider, fauxAssistantMessage, fauxToolCall } from '@earendil-works/pi-ai';
import { createCheckRuntime } from '../src/check-runtime.ts';
import { guardProvider } from '../src/authorization.ts';
import { serveRecovery } from '../src/recovery-consumer.ts';
import { canonical } from '../src/contract.ts';
const [directory, worktree] = process.argv.slice(2);
const input = JSON.parse(readFileSync(join(directory, 'controlled-launch.json'), 'utf8'));
try {
  await serveRecovery(directory, worktree, async (candidate, authorization) => {
    assert.equal(canonical(candidate.attempt), canonical(input.bound.checkAttempt));
    const runtime = await createCheckRuntime(candidate.launch, candidate.attempt.generation, input.bound,
      input.capability, input.configuration, input.credential_file, authorization, directory);
    console.log(JSON.stringify({ event: 'bootstrapped' }));
    const provider = fauxProvider(), models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
    const quote = value => "'" + value.replaceAll("'", "'\\''") + "'";
    provider.setResponses([
      transcript => {
        const text = JSON.stringify(transcript);
        assert.ok(text.includes('T07-SUPERVISED-BEHAVIOR'));
        assert.ok(text.includes(candidate.launch.attempt_id));
        return fauxAssistantMessage(fauxToolCall('bash', { command: [process.execPath, input.effect_script, join(directory, 'controlled-launch.json')].map(quote).join(' ') }), { stopReason: 'toolUse' });
      },
      transcript => {
        const results = transcript.messages.filter(m => m.role === 'toolResult');
        assert.equal(results.length, 1); assert.ok(!results[0].isError);
        return fauxAssistantMessage('Recovered original input; accepted Outcome remains Forge authority.');
      },
    ]);
    models.setProvider(guardProvider(provider.provider, authorization));
    return { ...runtime.options, models };
  });
} catch { console.error('controlled-recovery-failed'); process.exitCode = 1; }
