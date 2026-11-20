// Cross-language committed-storage fixture only. No Harness/model/tool execution.
import { readFileSync } from 'node:fs';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';

process.umask(0o077);
const { directory, launch, mode } = JSON.parse(readFileSync(0, 'utf8'));
persistIntent(directory, launch);
const storage = await openNodeSqliteStorage(databasePath(directory));
try {
  const root = await storage.mintId(), admission = await storage.mintId();
  const status = mode === 'placed' ? 'placed' : mode === 'done' ? 'done' : 'queued';
  const submission = { id: admission, conversationId: root, requestId: launch.operation_id, type: 'input', status };
  if (status !== 'queued') submission.entry = 99;
  if (status === 'done') submission.answer = 100;
  const rows = [
    { type: 'conversation', value: { id: root } },
    { type: 'submission', value: submission },
  ];
  if (mode === 'interrupted') rows.push({ type: 'task', value: {
    id: await storage.mintId(), conversationId: root, kind: 'pi.tool', version: 3, background: false, abortRequested: false,
    input: { kind: 'model', callId: 'private-call-canary', assistant: 3 },
    state: { status: 'running', checkpoint: { phase: 'execute', replay: 'unsafe', arguments: { command: 'private-command-canary' } } },
  } });
  await storage.commit([rows], () => {}, context);
  persistRoot(directory, root);
  console.log(JSON.stringify({ root, admission_id: String(admission) }));
} finally { await storage.close(context); }
