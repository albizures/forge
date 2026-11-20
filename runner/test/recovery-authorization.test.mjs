import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';
import { Authorization, guardProvider, guardTool } from '../src/authorization.ts';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';

const original = JSON.parse(await readFile(new URL('./authorization-lease.json', import.meta.url), 'utf8'));
const denied = { code: 'assignment-lease-denied' };
function envelope() {
  const lease = structuredClone(original);
  lease.sequence = 7;
  lease.daemon_epoch = 'recovery-epoch';
  lease.owner.host_pid += 100;
  lease.owner.worker_pid += 100;
  return {
    claim: { launch: structuredClone(lease.launch), operation_id: 'recover-original', daemon_epoch: lease.daemon_epoch, candidate_sha256: 'a'.repeat(64) },
    lease,
    evidence_sha256: 'b'.repeat(64),
  };
}
function waiting(t, expected = envelope()) {
  const binding = envelope().lease;
  const clock = { now: 1000 };
  const a = Authorization.forRecovery(binding.launch, binding.generation, binding.owner, binding.daemon_epoch, expected, () => clock.now);
  t.after(() => a.close());
  return { a, clock };
}
function renewal(sequence = 8, issued = 11000) {
  return { ...envelope().lease, sequence, issued_boottime_millis: issued, expires_boottime_millis: issued + 30000 };
}

test('recovery waits for exact committed envelope, retries and renews without sequence reset', t => {
  const expected = envelope();
  const { a, clock } = waiting(t, expected);
  assert.throws(() => a.admit(), denied);
  a.acceptRecovery(structuredClone(expected));
  a.admitFor(original.launch, original.generation);
  clock.now = 5000;
  a.acceptRecovery(structuredClone(expected));
  // Both expectation and accepted delivery are snapshotted, not caller-owned authority.
  expected.lease.expires_boottime_millis = 999999;
  clock.now = 11000;
  const next = renewal();
  a.accept(next); a.accept(structuredClone(next));
  next.expires_boottime_millis = 999999;
  clock.now = 40999; a.admit();
  clock.now = 41000;
  assert.throws(() => a.admit(), denied);
  assert.equal(a.signal.aborted, true);
  assert.throws(() => a.accept(renewal(9, clock.now)), denied);
});

test('ordinary first delivery and ordinary restarted consumers cannot adopt recovery leases', t => {
  for (const lease of [envelope().lease, renewal(1, 1000)]) {
    const { a } = waiting(t);
    assert.throws(() => a.accept(lease), denied);
    assert.throws(() => a.acceptRecovery(envelope()), denied);
  }
  const lease = envelope().lease;
  const ordinary = new Authorization(lease.launch, lease.generation, lease.owner, lease.daemon_epoch, () => 1000);
  t.after(() => ordinary.close());
  assert.throws(() => ordinary.accept(lease), denied);
  assert.throws(() => ordinary.acceptRecovery(envelope()), denied);
});

test('all recovery envelope identities are exact and mismatch terminally fences', t => {
  const changes = [
    e => { e.claim.operation_id += '-foreign'; },
    e => { e.claim.daemon_epoch += '-foreign'; },
    e => { e.claim.candidate_sha256 = 'c'.repeat(64); },
    e => { e.claim.launch.attempt_id += '-foreign'; },
    e => { e.evidence_sha256 = 'c'.repeat(64); },
    e => { e.lease.sequence++; },
    e => { e.lease.owner.worker_start += '-foreign'; },
    e => { e.lease.generation++; },
    e => { e.lease.launch.runtime_id += '-foreign'; },
    e => { e.lease.daemon_epoch += '-foreign'; },
    e => { e.extra = true; },
    e => { e.claim.extra = true; },
    e => { e.lease.expires_boottime_millis++; },
  ];
  for (const change of changes) {
    const { a } = waiting(t);
    const wrong = envelope(); change(wrong);
    assert.throws(() => a.acceptRecovery(wrong), denied);
    assert.equal(a.signal.aborted, true);
    assert.throws(() => a.acceptRecovery(envelope()), denied);
  }
});

test('invalid trusted expectations reject; matched payloads still undergo defensive lease checks', t => {
  const invalid = [
    e => { e.claim.candidate_sha256 = 'A'.repeat(64); },
    e => { e.evidence_sha256 = ''; },
    e => { e.claim.operation_id = ' '; },
    e => { e.claim.operation_id = 'x\n'; },
    e => { e.claim.daemon_epoch = 'foreign'; },
    e => { e.claim.launch.attempt_id = 'foreign'; },
    e => { e.lease.owner.worker_pid++; },
    e => { e.lease.sequence = 1; },
    e => { e.lease.sequence = 4294967296; },
    e => { e.unknown = true; },
  ];
  for (const change of invalid) {
    const expected = envelope(); change(expected);
    assert.throws(() => waiting(t, expected), denied);
  }
  for (const change of [
    e => { e.lease.owner.unknown = true; },
    e => { e.lease.issued_boottime_millis = 1001; },
    e => { e.lease.expires_boottime_millis++; },
    e => { e.lease.expires_boottime_millis = Number.MAX_SAFE_INTEGER + 1; },
  ]) {
    const expected = envelope(); change(expected);
    // Preparation may reject before delivery; neither phase may grant permission.
    try {
      const { a } = waiting(t, expected);
      assert.throws(() => a.admit(), denied);
      assert.throws(() => a.acceptRecovery(expected), denied);
      assert.equal(a.signal.aborted, true);
    } catch (error) { assert.equal(error.code, denied.code); }
  }
});

test('expiry, clock rollback, close and superseded recovery retries cannot revive', t => {
  for (const fault of ['late-first', 'expiry', 'rollback', 'closed', 'superseded']) {
    const { a, clock } = waiting(t);
    if (fault !== 'late-first') a.acceptRecovery(envelope());
    if (fault === 'late-first' || fault === 'expiry') clock.now = 31000;
    if (fault === 'rollback') clock.now = 999;
    if (fault === 'closed') a.close();
    if (fault === 'superseded') { clock.now = 11000; a.accept(renewal()); }
    assert.throws(() => a.acceptRecovery(envelope()), denied);
    clock.now = 11000;
    assert.throws(() => a.accept(renewal()), denied);
    assert.equal(a.signal.aborted, true);
  }
});

test('recovery permission gates real provider/tool wrapper boundaries and cancels their signals', t => {
  const { a, clock } = waiting(t);
  let effects = 0;
  const signals = [];
  const provider = guardProvider({ stream(_model, _input, options) { effects++; signals.push(options.signal); } }, a);
  const tool = guardTool((_args, _api, ctx) => { effects++; signals.push(ctx.abortSignal); }, a);
  assert.throws(() => provider.stream({}, {}), denied);
  assert.throws(() => tool({}, {}, context), denied);
  assert.equal(effects, 0);
  a.acceptRecovery(envelope());
  provider.stream({}, {}); tool({}, {}, context);
  assert.equal(effects, 2);
  clock.now = 31000;
  assert.throws(() => provider.stream({}, {}), denied);
  assert.throws(() => tool({}, {}, context), denied);
  assert.equal(effects, 2);
  assert.ok(signals.every(s => s.aborted));
});
