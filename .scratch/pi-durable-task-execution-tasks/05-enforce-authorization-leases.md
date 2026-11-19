# T05 — Fence execution with Forge authorization and bounded leases

## Execution status

In progress: [application/store foundation, verification and resume checklist](../../docs/research/pi-durable-t05/README.md).
Schema 21 retains grants; bounded renewal policy, terminally fenced issuance,
a trusted Linux process/flock/boottime adapter and a fresh-epoch server service
seam are delivered. All four repository gates pass (600 Rust tests passed,
1 ignored); eight host-evidence tests and ten TypeScript runner/process tests pass.
Daemon launch/renewal delivery wiring, runner admission enforcement, independent
watchdog and required lease-outage evidence are still pending. This Task is not complete and does not unblock T06.

## Delivered outcome

A runner can execute only within current Forge authority, and daemon outages stop new admissions and initiate cancellation when a bounded lease expires.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC7, AC8, AC12. Build on T04. Implement app-owned authorization and server-side validation, not a runner-defined policy or a heartbeat-as-permission shortcut.

Validate Assignment identity, current binding/freshness, revocation, exclusive ownership and applicable blockers before initial scheduling and recovery. Gate all model/tool admissions identified in T01, including compaction and internal/resumed execution. Leases are renewable only through Forge, with documented timing and restart semantics; choose concrete duration/renewal values from the contract/probes. Expiry blocks new work, cancels active execution and retains uncertainty where exit is not established.

Apply the Spec's proposed Assignment Authorization Lease glossary block to the actual runtime context. Record the authorization portion of proposed ADR-0013, completing it with retirement/recovery in later Tasks.

## Acceptance checks

- Revoked, stale, mismatched or unreachable authorization cannot start or resume work.
- No new model/tool admissions occur after expiry, including nested and compaction paths.
- Cancellation failure retains diagnostics/capacity, not inferred absence.
- Heartbeats, capability acknowledgement and leases remain distinct facts.

## Verification and evidence

Use a fake clock for renewal races, boundary expiry and daemon restart. Exercise a real long-running child and a controlled model request during connectivity loss. Assert admission counts, cancellation requests and retained blockers across restart. Record selected timing and limits without claiming universal process-death guarantees.

## Dependency

Depends on T04. Code Stack predecessor: T04.
