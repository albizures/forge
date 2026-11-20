// Synthetic transport fixture only: the expectation is fixed test data, not
// predecessor proof. No runtime, Harness, model, tool or writable store is opened.
import { readFileSync } from 'node:fs';
import { createInterface } from 'node:readline';
import { Authorization } from '../src/authorization.ts';
import { exactKeys } from '../src/contract.ts';

const { grant } = JSON.parse(readFileSync(new URL('./recovery-control.json', import.meta.url), 'utf8'));
const lease = grant.lease;
let now = 1000;
const ordinary = process.argv[2] === 'ordinary';
const args = [lease.launch, lease.generation, lease.owner, lease.daemon_epoch];
const authorization = ordinary
  ? new Authorization(...args, () => now)
  : Authorization.forRecovery(...args, grant, () => now);
const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
const event = value => process.stdout.write(`${JSON.stringify(value)}\n`);
try { authorization.admit(); throw Error('unexpected initial permission'); }
catch (error) { if (error.code !== 'assignment-lease-denied') throw error; }
event({ event: 'waiting' });
lines.on('line', line => {
  try {
    const value = JSON.parse(line);
    if ('host_recovery' in value) {
      exactKeys(value, ['host_recovery', 'grant']);
      if (value.host_recovery !== 1) throw Error('version');
      authorization.acceptRecovery(value.grant);
      authorization.admit();
      event({ event: 'accepted', sequence: value.grant.lease.sequence });
    } else {
      exactKeys(value, ['host_control', 'lease']);
      if (value.host_control !== 1) throw Error('version');
      now = 11000;
      authorization.accept(value.lease);
      authorization.admit();
      event({ event: 'accepted', sequence: value.lease.sequence });
    }
  } catch {
    authorization.close();
    event({ event: 'denied' });
  }
});
lines.on('close', () => authorization.close());
