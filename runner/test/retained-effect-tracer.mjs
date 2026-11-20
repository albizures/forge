// Test-only real Bash/process/effect driver. The Rust caller supplies its exact
// retained Launch; the lease is still a fixture, not supervised authorization.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { inspectInterruptedEffects, requireNoInterruptedEffects } from '../src/interrupted-effects.ts';
const input = JSON.parse(readFileSync(0, 'utf8'));
let mutations = 0, accepted;
const effect = new Promise(resolve => { accepted = resolve; });
const server = createServer((request, response) => {
  assert.equal(request.method, 'POST');
  mutations++;
  accepted();
  if (input.beforeResultCommit) response.end('publication accepted');
});
server.listen(0, '127.0.0.1');
await once(server, 'listening');
const child = spawn(process.execPath, [new URL('./interrupted-effect-worker.mjs', import.meta.url).pathname], { detached: true, stdio: ['pipe', 'pipe', 'pipe'] });
let output = '', reached;
const boundary = new Promise(resolve => { reached = resolve; });
child.stdout.on('data', data => { output += data; if (output.includes('before-result-commit')) reached(); });
child.stderr.on('data', data => { output += data; });
const exited = once(child, 'exit');
let timer;
try {
  child.stdin.end(JSON.stringify({ ...input, endpoint: `http://127.0.0.1:${server.address().port}/publication` }));
  await Promise.race([
    input.beforeResultCommit ? boundary : effect,
    exited.then(() => { throw Error('worker exited before controlled boundary'); }),
    new Promise((_, reject) => { timer = setTimeout(() => reject(Error('controlled boundary timed out')), 15000); }),
  ]);
  assert.equal(mutations, 1);
  process.kill(-child.pid, 'SIGKILL');
  assert.equal((await exited)[1], 'SIGKILL');
  const before = await readFile(`${input.directory}/runtime.sqlite`);
  const effects = await inspectInterruptedEffects(input.directory, input.launch);
  assert.equal(effects.length, 1);
  assert.deepEqual(await inspectInterruptedEffects(input.directory, input.launch), effects);
  await assert.rejects(requireNoInterruptedEffects(input.directory, input.launch), { code: 'runner-interrupted-effect' });
  assert.deepEqual(await readFile(`${input.directory}/runtime.sqlite`), before);
  assert.equal(mutations, 1);
  assert.ok(!output.includes('T07-MODEL-KEY-CANARY'));
  process.stdout.write(JSON.stringify({ effects, mutations, boundary: input.beforeResultCommit ? 'before-result-commit' : 'after-external-effect' }));
} finally {
  clearTimeout(timer);
  try { process.kill(-child.pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
}
