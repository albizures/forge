// T25: the runner's defense-in-depth fence check. The host's Rust verification
// is authoritative; this only refuses to run beside an unexplained fence.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, rename, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { unblocked } from '../src/recovery-consumer.ts';

const marker = JSON.stringify({ version: 1, code: 'runner-lease-expired', owner: { host_pid: 7 }, last_lease: { sequence: 3 }, cleanup: 'unverified' }) + '\n';
const receipt = (name, contents, sequence = 3) => JSON.stringify({
  version: 1, marker: name, marker_contents: contents, lease: { sequence }, children_absent: true,
});

test('fence markers block unless each is settled by its own exact receipt', async t => {
  const directory = await mkdtemp(join(tmpdir(), 'forge-fence-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const denied = () => assert.throws(() => unblocked(directory), { code: 'assignment-lease-denied' });
  unblocked(directory);
  await writeFile(join(directory, 'watchdog.json'), marker);
  denied();
  await writeFile(join(directory, 'watchdog-receipt-3.json'), receipt('watchdog.json', marker));
  unblocked(directory);
  // Tampered marker bytes, a misnamed receipt or absent children are not proof.
  await writeFile(join(directory, 'watchdog.json'), marker + ' ');
  denied();
  await writeFile(join(directory, 'watchdog.json'), marker);
  await rename(join(directory, 'watchdog-receipt-3.json'), join(directory, 'watchdog-receipt-4.json'));
  denied();
  await rename(join(directory, 'watchdog-receipt-4.json'), join(directory, 'watchdog-receipt-3.json'));
  await writeFile(join(directory, 'watchdog-receipt-3.json'), JSON.stringify({ ...JSON.parse(receipt('watchdog.json', marker)), children_absent: false }));
  denied();
  await writeFile(join(directory, 'watchdog-receipt-3.json'), receipt('watchdog.json', marker));
  unblocked(directory);
  // A later episode's own marker needs its own receipt.
  const episode = 'watchdog-9-1234-fence.json';
  await writeFile(join(directory, episode), marker);
  denied();
  await writeFile(join(directory, 'watchdog-receipt-5.json'), receipt(episode, marker, 5));
  unblocked(directory);
  await writeFile(join(directory, 'watchdog-receipt-5.json'), 'not json');
  denied();
});

test('an ungranted fence is settled only by its owner-bound receipt with no last lease', async t => {
  const directory = await mkdtemp(join(tmpdir(), 'forge-fence-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const denied = () => assert.throws(() => unblocked(directory), { code: 'assignment-lease-denied' });
  const ungranted = JSON.stringify({ version: 1, code: 'runner-issuer-lost', owner: { host_pid: 7, host_start: '99' }, last_lease: null, cleanup: 'unverified' }) + '\n';
  const proof = (contents, owner = { host_pid: 7, host_start: '99' }) => JSON.stringify({
    version: 1, marker: 'watchdog.json', marker_contents: contents, owner, children_absent: true,
  });
  await writeFile(join(directory, 'watchdog.json'), ungranted);
  denied();
  await writeFile(join(directory, 'ungranted-receipt-7-99.json'), proof(ungranted));
  unblocked(directory);
  // A misnamed receipt, or one bound to a marker that names a lease, is not proof.
  await writeFile(join(directory, 'ungranted-receipt-7-99.json'), proof(ungranted, { host_pid: 8, host_start: '99' }));
  denied();
  await writeFile(join(directory, 'watchdog.json'), marker);
  await writeFile(join(directory, 'ungranted-receipt-7-99.json'), proof(marker));
  denied();
});
