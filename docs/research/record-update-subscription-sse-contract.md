# Record Update Subscription SSE replay and recovery contract

**Question:** What exact V1 SSE/recovery behavior should durable, assignment-owned Record Update Subscriptions expose?

**Decision:** deliver one compact, immutable `record-update` SSE event for each matching committed Record Update. The update's global integer ID is both event ID and replay cursor. SSE delivery is a lossy wake signal: clients must reconcile through authorized Forge reads after connecting, reconnecting, receiving `resync-required`, or noticing a gap/duplicate.

## Evidence and implementation constraints

- The [HTML Server-Sent Events standard](https://html.spec.whatwg.org/multipage/server-sent-events.html) requires UTF-8 `text/event-stream`; events are delimited by a blank line; `event`, `data`, `id`, and `retry` are recognized fields; and comment lines are ignored. It specifies that after dispatching an event with a non-empty `id`, browser EventSource reconnects with that value in `Last-Event-ID`. Therefore Forge uses the decimal global Record Update ID as `id`, never a per-subscription sequence.
- The same [HTML standard](https://html.spec.whatwg.org/multipage/server-sent-events.html) says reconnect behavior is client-controlled apart from the `retry` hint, and that a failed EventSource stops reconnecting. Thus a recovery response must be an explicit non-2xx `resync-required`, rather than an un-IDed SSE control event that a browser may immediately reconnect from in a loop.
- Forge currently pins Axum 0.8.8. [Axum's SSE source](https://docs.rs/axum/latest/src/axum/response/sse.rs.html) produces `Content-Type: text/event-stream` and `Cache-Control: no-cache`, supports `Event::id`, named events, JSON data, and comment keep-alives. Its [`Event::id` documentation](https://docs.rs/axum/latest/axum/response/sse/struct.Event.html) rejects newline, CR, and NUL, all absent from a decimal update ID.
- The authoritative store finding in **Map Record Update Subscription integration seams (#468)** is that SQLite `record_updates.id` is global ascending order. It is therefore the only ordering/cursor key; timestamps are metadata, not cursors.

Sources were retrieved 2026-10-24. No external source was dropped. The sources describe SSE mechanics; the authorization, retention, and application semantics below are Forge V1 decisions.

## Terms and durable state

A subscription has a random opaque `subscription_id`, an owning open Agent Assignment, an immutable normalized target-ID set, an optional normalized Record Update-kind filter, `start_after_update_id`, and a 30-minute lease expiry. Creation captures `start_after_update_id` as the greatest committed global Record Update ID in the same transaction that persists the subscription. It is idempotent by owner, normalized targets, and filter, returning the same unexpired subscription and preserving its original start cursor.

`head` is the greatest committed global update ID at connection setup. `retention_floor` is the greatest update ID known to have been discarded. V1 retains Record Updates indefinitely, so `retention_floor` is `0`. This field exists in the port/contract now so a future pruning policy can make recovery explicit instead of silently skipping history.

A supplied cursor is valid only when it is an unsigned base-10 integer, is at least `start_after_update_id`, and is not below `retention_floor`. `Last-Event-ID` takes precedence over an optional `after` query parameter; supplying both with unequal valid values is a `400 invalid-cursor`. With neither, the effective cursor is `start_after_update_id`: a newly created subscription does not replay pre-subscription history.

## HTTP and authorization

The management API creates, renews, changes targets, and deletes subscriptions; its exact RPC shapes remain an implementation seam. The event endpoint is:

```text
GET /v1/agent-assignments/{assignment_id}/record-update-subscriptions/{subscription_id}/events
Authorization: Bearer <assignment-capability>
Last-Event-ID: <decimal-update-id>  # optional
```

The endpoint validates that the opaque capability authorizes the named, still-open assignment; that assignment owns the subscription; and its lease has not expired. It must not infer authority from the subscription ID. Failures before a stream starts are ordinary HTTP responses: `401` missing/invalid capability, `403` valid capability for another assignment, `404` absent subscription, and `410 subscription-expired` for an expired lease. The existing unauthenticated daemon listener is therefore not sufficient: adding this endpoint requires an authenticated HTTP capability seam.

A malformed, too-old, or pre-subscription cursor returns `409 Conflict` with `application/json`:

```json
{"code":"resync-required","subscription_id":"sub_…","retention_floor":0,"start_after_update_id":381,"head":412}
```

The response is intentionally not an SSE stream and carries no Record state. The client stops/recreates its EventSource (or otherwise reconnects only after reconciliation), performs authorized authoritative reads, then resumes from the returned `head` when appropriate. This applies equally when future retention makes a previously accepted cursor unavailable.

On success the response is `200`, `Content-Type: text/event-stream; charset=utf-8`, `Cache-Control: no-cache, no-transform`, and `X-Accel-Buffering: no`. It sends `retry: 1000` once and a `: keepalive` comment every 15 seconds of otherwise idle stream time. Deployment must also configure any reverse proxy not to buffer this route.

## Event wire format

For each matching update with ID `412`, Forge writes exactly one complete SSE event:

```text
id: 412
event: record-update
data: {"subscription_id":"sub_…","record_id":467,"record_update_id":412,"kind":"comment-added","timestamp":"2026-10-24T01:33:00.482Z","comment_id":2}

```

`record_update_id` equals the SSE `id`. `kind` is Forge's stable hyphenated Record Update kind. The only optional type-specific references are `from_status`/`to_status`, `related_record_id`, and `comment_id`; absent fields are omitted. The envelope never embeds a Record, comment body, assignment capability, or mutable state.

The delivery adapter selects committed updates whose IDs are strictly greater than the effective cursor, strictly greater than `start_after_update_id`, belong to the subscription's explicit targets, and pass its filter, in ascending ID order. It establishes the live listener before (or atomically with) the final replay query and deduplicates by ID, so an update committed during handoff is either replayed or observed live. Duplicates remain permitted across any disconnect/retry boundary; consumers deduplicate only as an optimization and always reconcile.

## Connection and overload rules

A subscription has one active connection slot. Claiming it atomically installs a new generation and cancels the prior generation. The prior stream ends without a terminal update; its client reconnecting later may itself supersede the then-current connection. This is deliberate last-connected-wins behavior.

Each active connection has a bounded, in-memory queue of **256 envelopes**. Publication occurs only after the Record mutation transaction commits and uses non-blocking enqueue. On a full queue, Forge marks that generation overflowed, cancels its stream, and drops no database writes or publisher work. The next connection replays from its last dispatched `Last-Event-ID`; because the last dispatched point may be behind the overflow, reconciliation is mandatory. Connection state and queue contents are not durable and are not shared across processes. V1 therefore supports only one daemon process against a store; cross-process fan-out is a prerequisite for multi-process deployment.

An SSE connection validates capability/lease at open only. It does not renew the lease. Lease renewal, explicit deletion, assignment retirement, stream cancellation, and overflow all remove/cancel the active generation promptly. Assignment retirement atomically removes its subscriptions or makes them unreadable in the same close transaction.

## Required tests

1. **Framing:** successful response is UTF-8 `text/event-stream`, includes cache/no-buffer headers, initial retry, periodic comment, named event, decimal `id`, JSON data, and a blank-line terminator.
2. **Cursor replay:** create at update 10; updates 11, 12, 13 matching targets replay ascending after `Last-Event-ID: 10`; reconnect at 12 yields only 13. A duplicate delivery after disconnect is accepted by the contract.
3. **Boundary:** no cursor begins after the captured creation cursor; a cursor before that cursor, malformed decimal cursor, or conflicting header/query cursor yields the stated recovery/error response. Future simulated `retention_floor` advancement yields `409 resync-required`.
4. **Filtering and envelope:** unrelated target/filter updates never emit; each supported kind serializes only its allowed references and no Record state or capability.
5. **Race:** inject an update between live-listener registration and replay query; it arrives exactly once in that uninterrupted connection. Inject an update immediately after the query; it arrives live.
6. **Authorization/lifecycle:** another assignment's capability cannot subscribe; expiry does not renew from streaming; explicit deletion and assignment retirement end a stream and block reconnect; direct-child target authorization is enforced at subscription mutation.
7. **Connection replacement:** opening a second stream cancels the first and only the current generation receives subsequent live deliveries.
8. **Slow consumer:** fill 256 pending envelopes without polling the writer; the stream is cancelled, publication and Record writes complete without waiting, and a new stream can replay/reconcile from the last dispatched ID.
9. **Process boundary:** document/test that in-process publication reaches same-process connections only; reject or defer multi-process configuration until a cross-process wake mechanism exists.

## Implementation handoff

Turn this contract plus **Map Record Update Subscription integration seams (#468)** into an implementation Spec. That Spec should introduce the subscription domain/repository port, global ascending Record Update envelope query, transaction-safe creation/retirement behavior, post-commit in-process adapter, authenticated Axum endpoint, and the tests above. It must not implement Pi behavior, WebSockets, dynamic targets, transitive traversal, or exactly-once delivery.
