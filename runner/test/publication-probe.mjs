// Throwaway research probe for albizures/forge#37 ("can a durable agent publish?").
// Runs commands through the REAL durable `bash` tool built by createProfileRuntime
// (same path as the runner), with a fixture authorization and a fake model key.
// No model provider is contacted. Nothing is pushed: GitHub checks are read-only
// (`ssh -T`, `git push --dry-run`, `gh auth status`, `gh api user`).
//
// Usage: node runner/test/publication-probe.mjs <repo-with-github-remote>
import { mkdtemp, mkdir, writeFile, readFile, realpath, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { Authorization } from '../src/authorization.ts';
import { createProfileRuntime } from '../src/profiles.ts';

const repo = process.argv[2];
if (!repo) throw new Error('usage: publication-probe.mjs <repo>');
const wire = JSON.parse(await readFile(new URL('./profile-v1.json', import.meta.url), 'utf8'));
const grant = JSON.parse(await readFile(new URL('./authorization-lease.json', import.meta.url), 'utf8'));

const directory = await realpath(await mkdtemp(join(tmpdir(), 'forge-publication-probe-')));
const cwd = join(directory, 'worktree');
const home = join(directory, 'keys');
await mkdir(cwd); await mkdir(home, { mode: 0o700 });
const key = join(home, 'openai-key');
await writeFile(key, 'sk-PROBE-NOT-A-REAL-KEY\n', { mode: 0o600 });

// Local bare remote + a clone in the durable cwd: push mechanics without credentials.
const bare = join(directory, 'remote.git');
execFileSync('git', ['init', '-q', '--bare', bare]);
execFileSync('git', ['-C', cwd, 'init', '-q', '-b', 'main']);
execFileSync('git', ['-C', cwd, '-c', 'user.name=probe', '-c', 'user.email=probe@example.invalid', 'commit', '-q', '--allow-empty', '-m', 'probe']);
execFileSync('git', ['-C', cwd, 'remote', 'add', 'local', bare]);

let now = 1000;
const authorization = new Authorization(grant.launch, grant.generation, grant.owner, grant.daemon_epoch, () => now);
authorization.accept({ ...grant, sequence: 1, issued_boottime_millis: 1000, expires_boottime_millis: 31000 });

// Capability env as the Rust host delivers it (names from profiles.ts capabilityNames).
const capability = {
  FORGE_ASSIGNMENT_ID: 'assignment-probe',
  FORGE_ASSIGNMENT_DAEMON_URL: 'http://127.0.0.1:9/rpc',
  FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE: join(home, 'capability'),
  FORGE_ASSIGNMENT_FORGE_CLI: '/opt/forge-probe/bin/forge',
};
const runtime = createProfileRuntime({ ...wire, cwd }, key, authorization, capability);
const env = runtime.options.env({ cwd }, context);
const tools = runtime.options.registry.snapshot().extension('forge-coding').tools;
const bash = tools.find(tool => tool.name === 'bash');

async function durable(command) {
  let output = '';
  const api = { env, agent: runtime.agent, models: runtime.options.models, output: text => { output += text; }, diagnostic: () => {}, retainedOutput: () => ({ text: output, truncated: false }) };
  let status = 'ok';
  try {
    const result = await bash.execute({ command, timeout: 30 }, api, context);
    if (result?.isError) status = 'nonzero';
  } catch (error) { status = `threw:${error?.code ?? error?.message}`; }
  return { status, output: output.trim() };
}
function ambient(command) {
  try { return { status: 'ok', output: execFileSync('/bin/sh', ['-c', command], { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 30000 }).trim() }; }
  catch (error) { return { status: `nonzero:${error.status}`, output: `${error.stdout ?? ''}${error.stderr ?? ''}`.trim() }; }
}

const GH = '/home/linuxbrew/.linuxbrew/bin/gh';
const probes = [
  ['env (names=values, durable has no secrets)', 'env | sort'],
  ['id / passwd home', 'id -un; getent passwd "$(id -u)" | cut -d: -f6'],
  ['tool lookup', 'for t in git gh ssh forge; do printf "%s=" "$t"; command -v "$t" || echo MISSING; done'],
  ['git global config visible', 'git config --global --list >/dev/null 2>&1 && echo readable || echo "unreadable (exit $?)"; git config --show-origin --get-regexp "^(credential|user)\\." || echo "no credential/user config"'],
  ['git commit with no identity config', 'echo x > probe.txt && git add probe.txt && git commit -q -m probe 2>&1; echo "exit=$?"'],
  ['push to local bare remote', 'git push local HEAD:refs/heads/probe 2>&1 && echo PUSHED'],
  ['ssh auth to github.com', 'ssh -o BatchMode=yes -o ConnectTimeout=10 -T git@github.com 2>&1; echo "exit=$?"'],
  ['git push --dry-run to github (ssh)', `git -C ${JSON.stringify(repo)} push --dry-run origin HEAD:refs/heads/research/durable-publication 2>&1; echo "exit=$?"`],
  ['git push --dry-run to github (https)', `GIT_TERMINAL_PROMPT=0 git -C ${JSON.stringify(repo)} push --dry-run https://github.com/albizures/forge.git HEAD:refs/heads/research/durable-publication 2>&1; echo "exit=$?"`],
  ['gh on PATH: auth status', 'gh auth status 2>&1; echo "exit=$?"'],
  ['gh by absolute path: auth status', `${GH} auth status 2>&1; echo "exit=$?"`],
  ['gh by absolute path: api user', `${GH} api user --jq .login 2>&1; echo "exit=$?"`],
];

const results = [];
for (const [name, command] of probes) {
  const d = await durable(command);
  const a = name.startsWith('env') ? { status: 'ok', output: ambient('env | cut -d= -f1 | sort').output } : ambient(command);
  results.push({ name, command, durable: d, ambient: a });
}
authorization.close();
await rm(directory, { recursive: true, force: true });

for (const r of results) {
  console.log(`\n=== ${r.name}\n$ ${r.command}`);
  console.log(`--- durable bash [${r.durable.status}]\n${r.durable.output}`);
  console.log(`--- ambient shell [${r.ambient.status}]${r.name.startsWith('env') ? ' (names only)' : ''}\n${r.ambient.output}`);
}
