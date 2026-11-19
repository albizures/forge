// Test-only Rust server -> real runner configuration bridge. No execution/store.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Authorization } from '../src/authorization.ts';
import { createProfileRuntime } from '../src/profiles.ts';
const input = JSON.parse(readFileSync(0, 'utf8'));
const grant = JSON.parse(readFileSync(new URL('./authorization-lease.json', import.meta.url), 'utf8'));
const authorization = new Authorization(grant.launch, grant.generation, grant.owner, grant.daemon_epoch, () => 1000);
try {
  const runtime = createProfileRuntime(input.configuration, input.credential_file, authorization);
  const auth = await runtime.options.models.getAuth('openai');
  assert.ok(auth?.auth.apiKey);
  // Access to model auth does not establish Forge execution authority.
  assert.throws(() => runtime.options.models.getProvider('openai').streamSimple(runtime.options.models.getModel('openai', 'gpt-4.1'), {}), { code: 'assignment-lease-denied' });
  console.log(JSON.stringify({ model: runtime.agent.model, tools: runtime.agent.tools.map(tool => tool.name), configured: true, permission: false }));
} catch {
  console.error('profile-fixture-failed');
  process.exitCode = 1;
} finally { authorization.close(); }
