# ADR-006: Record-update SSE delivery is in-process for V1

- **Status:** Accepted
- **Date:** 2026-10-25
- **Decision scope:** Daemon ownership and record-update subscription delivery

## Context

Record Update Subscriptions replay durable SQLite updates, then use SSE as a
low-latency wakeup mechanism. The active-connection queue, replacement,
revocation, and lease-expiry state is deliberately in process. Multiple daemon
processes against one store would not share that state and could produce
inconsistent live delivery.

## Decision

V1 permits one Forge daemon per SQLite store. Before binding its HTTP listener,
the daemon acquires and retains a non-blocking exclusive advisory lock on a
sidecar file beside the configured store. A competing daemon fails startup
clearly. Closing the owning daemon releases the advisory lock so a later daemon
can start.

The lock establishes daemon ownership only. It is not a SQLite writer lock and
does not prohibit intentional direct SQLite access. Durable SQLite record
updates remain the source of truth; in-process SSE is only a wake transport and
replay/reconciliation closes its publish/replay races.

## Consequences

- A second daemon cannot appear healthy on another listener while sharing an
  active store.
- SSE queue and fanout behavior can remain simple and lifecycle-safe without a
  distributed coordination layer.
- Operators must use a distinct store for each concurrently running daemon.
- Direct SQLite writers remain possible, but do not gain live SSE fanout merely
  by writing the database.

## Replacement condition

Replace this decision before supporting multiple daemons for one store or any
cross-process subscription delivery. Its replacement must provide shared
publication/fanout coordination, preserve durable cursor replay and ownership
revocation semantics, and define failure recovery across daemon processes.

## References

- [Record Update Subscriptions](../record-update-subscriptions.md)
- Spec #510, “Make record-update SSE delivery lifecycle-safe”
