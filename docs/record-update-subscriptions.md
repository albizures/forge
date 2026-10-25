# Record Update Subscriptions

Record Update Subscriptions let an Agent Assignment receive durable wakeups when
specified Records change. They are intended to prompt reconciliation, not to
replace Forge's authorized Record and Record Update reads.

The normative HTTP/SSE framing and recovery rules are in the
[Record Update Subscription SSE replay and recovery contract](research/record-update-subscription-sse-contract.md).
That contract is the wire-level authority; this document describes the
integration boundary and V1 lifecycle.

## Ownership and authorization

A subscription belongs to one open Agent Assignment and is authorized by that
assignment's opaque capability. The capability is validated by Forge for every
subscription-management request. Delivery is restricted to the daemon's
loopback listener and validates the named open assignment and subscription
ownership. Closing or retiring the owner revokes access and removes or makes
its subscriptions unreadable.

An assignment may create subscriptions only for its explicit direct prerequisites.
Targets are explicit normalized Record IDs; they do not follow later relationship
changes and are never derived transitively. A subscription has a
target set, optional normalized Record Update-kind filter, a captured starting
cursor, and a renewable 30-minute lease.

## Lifecycle and management

The management API supports creating, renewing, and deleting subscriptions.
Its RPC shapes are an implementation seam.

Creation is idempotent for the same owner, normalized targets, and filter. An
unexpired matching subscription is returned with its original starting cursor.
Creation captures that cursor atomically with persistence, so cursorless
subscription delivery starts after updates committed before creation. Renewing
extends the lease; opening delivery does not. Deletion, expiry, and assignment
retirement promptly cancel active delivery.

## Delivery and reconciliation

The loopback delivery endpoint is:

```text
GET /v1/agent-assignments/{assignment_id}/record-update-subscriptions/{subscription_id}/events

The endpoint is available only through the daemon's loopback listener; V1 has
no HTTP bearer capability requirement.
```

It delivers committed matching updates through SSE. Each notification is a
compact immutable Record Update envelope carrying the subscription ID, Record
ID, global Record Update ID, kind, timestamp, and only allowed type-specific
references. It never carries Record state, comment bodies, or an assignment
capability.

The global Record Update ID is the subscription cursor. Clients may resume with
`Last-Event-ID` (or the contract's `after` query parameter rules). Forge replays
matching later updates in ascending order and then continues live delivery.
Delivery is at-least-once: duplicates can occur across a disconnect or retry.
Clients must reconcile through authorized Forge reads after connecting,
reconnecting, observing a gap or duplicate, or receiving recovery guidance.

A malformed, pre-subscription, unavailable, or retained-away cursor requires
recovery rather than silent skipping. The endpoint responds before starting a
stream with the contract's `409 resync-required` JSON response, which supplies
the cursor facts needed to reconcile and resume. Authentication, ownership,
missing-subscription, and expired-lease failures are likewise ordinary
pre-stream HTTP responses as specified by the wire contract.

## Connection behavior and limits

A subscription has one active delivery connection. The latest connection
replaces and cancels the prior connection. Each connection has a bounded
256-envelope in-memory queue; a slow consumer that fills it is cancelled
without blocking Record writes. A subsequent connection replays and
reconciles from its last dispatched cursor.

Forge publishes only after the Record mutation commits. The in-process delivery
adapter is a wake mechanism; durable cursor replay closes publish/replay races.
V1 supports one daemon process against a store. Daemon startup holds a
non-blocking exclusive advisory sidecar lock for that store, rejecting another
daemon before it binds its listener; the lock does not restrict direct SQLite
writers. Queue state and fanout are not shared between processes.

## V1 boundaries

V1 does not implement a Pi extension or prescribe Pi/client reactions. It does
not provide WebSockets, dynamic or transitive targets, exactly-once delivery,
or cross-process fanout. Subscription delivery is deliberately a replaceable
transport boundary; V1 uses loopback-only SSE.
