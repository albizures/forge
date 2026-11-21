// Controlled fake model/configuration port; actual private receiver and reopen.
import { existsSync, unlinkSync, appendFileSync } from 'node:fs';
import { join } from 'node:path';
import { serveRecovery } from '../src/recovery-consumer.ts';
import { createRegistry } from '@earendil-works/pi-durable';
import { createModels, fauxProvider, fauxAssistantMessage } from '@earendil-works/pi-ai';
import { guardProvider } from '../src/authorization.ts';
const [directory, worktree] = process.argv.slice(2);
try {
  await serveRecovery(directory, worktree, async (_, authorization) => {
    appendFileSync(join(directory, 'controlled-recovery-preparations'), 'prepare\n');
    const marker = join(directory, 'controlled-crash-once');
    if (existsSync(marker)) {
      unlinkSync(marker);
      process.kill(process.pid, 'SIGKILL'); // test-owned safe pre-open boundary
      await new Promise(() => {});
    }
    const provider = fauxProvider();
    const model = () => {
      appendFileSync(join(directory, 'controlled-recovery-models'), 'model\n');
      // Keep the first answer in flight so the daemon can send recorded input.
      if (existsSync(join(directory, 'controlled-slow-model'))) Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 1500);
      console.log(JSON.stringify({ event: 'recovered-model' }));
      return fauxAssistantMessage('Recovered the original bound input; not an accepted Forge Outcome.');
    };
    provider.setResponses([model, model]);
    const models = createModels({ authContext: { env: async () => undefined, fileExists: async () => false } });
    models.setProvider(guardProvider(provider.provider, authorization));
    return { models, registry: createRegistry(), settings: { retry: { enabled: false }, compaction: { enabled: false } }, env: () => undefined };
  });
} catch { console.error('controlled-recovery-failed'); process.exitCode = 1; }
