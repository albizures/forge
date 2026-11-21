// Cross-language committed-storage fixture only. No Harness/model/tool execution.
import { readFileSync, writeSync } from 'node:fs';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { Harness, createRegistry } from '@earendil-works/pi-durable';
import { createModels } from '@earendil-works/pi-ai';
import { databasePath, persistIntent, persistRoot } from '../src/storage.ts';

process.umask(0o077);
const { directory, launch, mode, worktree } = JSON.parse(readFileSync(0, 'utf8'));
persistIntent(directory, launch);
const storage = await openNodeSqliteStorage(databasePath(directory));
if (mode === 'queued-agent') {
  let root;
  const controlled = new Proxy(storage, {
    get(target, key) {
      if (key === 'commit') return async (batches, ...args) => {
        if (batches.flat().some(row => row.type === 'task' && row.value.kind === 'pi.generation' && row.value.state.status === 'running')) {
          const submission = await storage.submissionByRequest(root.id, launch.operation_id, context);
          writeSync(1, JSON.stringify({ root: root.id, admission_id: String(submission.id) }));
          process.exit(0); // committed safe boundary, BEFORE generation admission
        }
        return target.commit(batches, ...args);
      };
      const value = Reflect.get(target, key, target);
      return typeof value === 'function' ? value.bind(target) : value;
    },
  });
  const harness = await Harness.open(controlled, { models: createModels(), registry: createRegistry(), env: () => undefined }, context);
  root = await harness.root(context, { agent: { cwd: worktree, model: { provider: 'faux', modelId: 'faux-1' } } });
  persistRoot(directory, root.id);
  await root.submit({ type: 'input', requestId: launch.operation_id, content: 'Original authorized bound input.' }, context);
  await new Promise(() => {});
}
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
