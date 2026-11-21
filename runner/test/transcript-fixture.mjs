// Committed-storage fixture for the Rust transcript/status integration test.
// No Harness, provider, tool execution or external effect is started here.
import { readFileSync } from 'node:fs';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { databasePath, journal, persistIntent, persistRoot } from '../src/storage.ts';

process.umask(0o077);
const { directory, launch, mode, count } = JSON.parse(readFileSync(0, 'utf8'));
if (mode === 'seed') persistIntent(directory, launch);
const storage = await openNodeSqliteStorage(databasePath(directory));
try {
  let root;
  if (mode === 'seed') {
    root = await storage.mintId();
    await storage.commit([[{ type: 'conversation', value: { id: root } }]], () => {}, context);
    persistRoot(directory, root);
  } else root = journal(directory).root;
  for (let i = 0; i < count; i++) {
    const id = await storage.mintId();
    await storage.commit([[{ type: 'entry', value: { id, conversationId: root, kind: 'fixture.message', data: { text: `${mode}-${i}` } } }]], () => {}, context);
  }
} finally { await storage.close(context); }
