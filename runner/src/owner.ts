// Defensive worker-side validation of the host's shared kernel ownership.
// This evidence is not a grant, process-adoption permission or proof of cleanup.
import { readFileSync, fstatSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { canonical, requireCondition } from './contract.ts';

function processStart(pid: number): string {
  return readFileSync(`/proc/${pid}/stat`, 'utf8').split(/\) (?=[RSDZTtXxKWPI] )/).at(-1)!.split(' ')[19];
}

export function ownerIdentity(directory: string) {
  const owner = JSON.parse(process.env.FORGE_RUNNER_OWNER ?? 'null');
  requireCondition(owner && owner.host_pid === process.ppid, 'runner-owner-parent-invalid');
  requireCondition(owner.host_start === processStart(process.ppid), 'runner-owner-start-invalid');
  requireCondition(owner.boot_id === readFileSync('/proc/sys/kernel/random/boot_id', 'utf8').trim(), 'runner-owner-boot-invalid');
  requireCondition(Number.isInteger(owner.lock_fd) && owner.lock_fd >= 3, 'runner-owner-descriptor-invalid');
  const inherited = fstatSync(owner.lock_fd, { bigint: true });
  const lock = statSync(join(directory, 'owner.lock'), { bigint: true });
  requireCondition(inherited.dev === lock.dev && inherited.ino === lock.ino, 'runner-owner-descriptor-invalid');
  requireCondition(owner.lock_device === String(lock.dev) && owner.lock_inode === String(lock.ino), 'runner-owner-invalid');
  const evidence = JSON.parse(readFileSync(join(directory, 'owner.json'), 'utf8'));
  requireCondition(canonical(evidence.owner) === canonical(owner), 'runner-owner-evidence-invalid');
  requireCondition(evidence.worker_pid === process.pid && evidence.worker_start === processStart(process.pid) && evidence.process_group === process.pid, 'runner-owner-worker-invalid');
  return { ...evidence, worker_start: processStart(process.pid) };
}
