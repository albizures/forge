// Builds a matched durable-runner bundle for `forgeup register --runner-bundle`.
//   node package-bundle.mjs --out DIR --forge-commit FULL_SHA [--node PATH_TO_NODE_BINARY]
// The bundle contains only the pinned Node executable, runner sources and the
// lock-installed production dependency closure (no PATH/npm discovery at runtime).
import { cp, mkdir, readFile, writeFile, readdir, lstat, rm, copyFile, chmod } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = new Map();
for (let i = 2; i < process.argv.length; i += 2) args.set(process.argv[i], process.argv[i + 1]);
const out = args.get('--out');
const commit = args.get('--forge-commit');
const nodePath = args.get('--node') ?? process.execPath;
if (!out || !/^[0-9a-f]{40}$/.test(commit ?? '')) {
  throw new Error('usage: --out DIR --forge-commit <40-hex> [--node PATH]');
}
const source = fileURLToPath(new URL('.', import.meta.url));
const pkg = JSON.parse(await readFile(join(source, 'package.json'), 'utf8'));
const pinned = pkg.engines.node;
const reported = execFileSync(nodePath, ['--version'], { env: {} }).toString().trim();
if (reported !== `v${pinned}`) throw new Error(`node-version-mismatch: ${reported} is not pinned v${pinned}`);

await rm(out, { recursive: true, force: true });
await mkdir(join(out, 'node'), { recursive: true });
await mkdir(join(out, 'runner/src'), { recursive: true });
await copyFile(nodePath, join(out, 'node/node'));
await chmod(join(out, 'node/node'), 0o755);
for (const file of ['package.json', 'package-lock.json', 'UPSTREAM-LICENSE', 'prepare-reader.mjs']) {
  await copyFile(join(source, file), join(out, 'runner', file));
}
await cp(join(source, 'src'), join(out, 'runner/src'), { recursive: true });
// Locked, production-only, script-free, no bin symlinks. Offline cache is preferred.
execFileSync('npm', ['ci', '--omit=dev', '--ignore-scripts', '--no-bin-links', '--no-audit', '--no-fund', '--prefer-offline'], {
  cwd: join(out, 'runner'), stdio: 'inherit',
});
// The maintained derived reader is generated inside the bundle's own dependency copy.
execFileSync(join(out, 'node/node'), [join(out, 'runner/prepare-reader.mjs')], { cwd: join(out, 'runner'), stdio: 'inherit', env: {} });
await rm(join(out, 'runner/node_modules/.package-lock.json'), { force: true });

async function files(dir) {
  const found = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    const stat = await lstat(path);
    if (stat.isDirectory()) found.push(...await files(path));
    else if (stat.isFile()) found.push(path);
    else throw new Error(`non-regular file in bundle: ${relative(out, path)}`);
  }
  return found;
}
const sha = async (path) => createHash('sha256').update(await readFile(path)).digest('hex');
const entries = (await files(out)).map((p) => relative(out, p)).filter((p) => p !== 'forge-runner-bundle.json').sort();
const tree = createHash('sha256');
for (const entry of entries) {
  const stat = await lstat(join(out, entry));
  tree.update(Buffer.concat([Buffer.from(entry), Buffer.from([0, (stat.mode & 0o111) ? 1 : 0, 0]), Buffer.from(await sha(join(out, entry))), Buffer.from('\n')]));
}
const manifest = {
  schema: 1,
  runner_protocol: 1,
  node_version: pinned,
  forge_commit: commit,
  package_lock_sha256: await sha(join(out, 'runner/package-lock.json')),
  tree_sha256: tree.digest('hex'),
};
await writeFile(join(out, 'forge-runner-bundle.json'), JSON.stringify(manifest, null, 2) + '\n');
console.log(JSON.stringify(manifest));
