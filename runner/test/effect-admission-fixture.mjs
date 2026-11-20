// Committed-storage fixture for the actual Rust supervisor/grant-controller test.
// No Harness, provider, tool execution or external effect is started here.
import { readFileSync } from 'node:fs';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';

process.umask(0o077);
const { directory, launch, mode } = JSON.parse(readFileSync(0, 'utf8'));
persistIntent(directory, launch);
const storage = await openNodeSqliteStorage(databasePath(directory));
try {
  const root = await storage.mintId();
  persistRoot(directory, root);
  if (mode !== 'missing-root') {
    const id = await storage.mintId();
    await storage.commit([[
      { type: 'conversation', value: { id: root } },
      { type: 'task', value: { id, conversationId: root, kind: 'pi.tool', version: 3,
        background: false, abortRequested: false,
        input: { kind: 'model', callId: 'controller-effect', assistant: 3 },
        state: mode === 'completed'
          ? { status: 'terminal', outcome: { status: 'completed', result: {} } }
          : { status: 'running', checkpoint: { phase: 'execute', replay: 'unsafe', arguments: {} } },
      } },
    ]], () => {}, context);
  }
} finally { await storage.close(context); }
