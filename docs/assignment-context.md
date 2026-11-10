# Authenticated Assignment context and actions

The application/RPC boundary and `forge assignment` CLI implement authenticated
Assignment operations. Task and Grilling launches, including legacy revisions, use
protected bootstrap; the flags-only runtime limitations in
[Agent Profile policy](agent-profile-policy.md) remain.

## Authority and delivery

`Application::assignment` takes the daemon-configured protected capability-file
root and delivered Assignment identity/secret. The root is trusted daemon
configuration, never an agent request field. A session is deliberately neither
Debug nor serializable. Every operation revalidates the actual secret file and
persisted open Assignment; neither an Assignment ID nor a caller-supplied
`capability_id` is a bearer credential. Delivery remains the owner-only file
contract in ADR-007. This is not OS/process/same-user isolation.

The new `AssignmentApi` is registered on the production `/rpc` router. Its actual
JSON-RPC method names use the jsonrpsee namespace separator **underscore**:

| Method | Extra request fields beyond `assignmentId`, `secret` |
| --- | --- |
| `assignment_bootstrap` | none |
| `assignment_context` | none |
| `assignment_record` | `target` |
| `assignment_comments` | `target`, `limit`, optional `cursor` |
| `assignment_history` | `target`, `limit`, optional `cursor` |
| `assignment_comment` | `body` |
| `assignment_start` | none |
| `assignment_done` | optional `resolution`, optional `human` |

Credentials are transport inputs from protected delivery, **not model-facing
arguments or instructions to paste secrets into tools**. The CLI derives them
automatically from protected delivery. Requests reject unknown fields, so own mutations have no target
selector and cannot be redirected by adding one. Responses carry
`schemaVersion: 1`; existing nested Record fields retain their established wire
names. Credential-bearing requests and behavior-bearing responses have no Debug
implementation. Operator APIs remain the existing trusted local interface, not
an independently authenticated operator security boundary.

`agentDispatch_taskStart` now always rejects with `assignment-route-removed`.
Retained `agentDispatch_assignmentAcknowledge`, `assignmentTaskStart`,
`assignmentInstructions` and `assignmentBootstrapFailure` use the same actual
application capability authentication. Caller correlation/capability strings are
not persisted as audit authority. Bootstrap-failure reporting records a fixed
redacted stop/review message rather than caller-supplied diagnostics. Subscription
routes now use the same application session; their contract is described below.

## Bootstrap and context

Bootstrap authenticates, acknowledges, idempotently starts the assigned Record,
fetches trusted context, then returns separately labelled persisted `behavior`
(registered Markdown, or the frozen prompt template for a legacy revision).
Record bodies remain in live context rather than being mixed into behavior.
Both Tasks and Grillings use this sequence. Each step can fail; failure does not
fall back to behavioral instructions, clear a Dispatch Hold or authorize another
launch. Context bootstrap evidence is durable and only marked after an
acknowledged In Progress Record's context is assembled. Instructions cannot be
retrieved after acknowledgement/start alone. Repeating bootstrap is safe while
the Record remains In Progress; normal lifecycle rules still apply after Done.
Launch/acknowledgement race integration remains #621.

Context separates:

- `bound`: Assignment/Record identity, immutable per-Record-Type profile revision,
  provider/kind/model, attempt number, execution directory and provider Workspace
  identity. These are persisted configuration/ownership facts, **not runtime
  attestation**. The execution directory is retained once available by dispatch
  context construction (or the first authenticated context read for an older
  attempt), independently of later Worktree Claim cleanup state.
- `record`: live identity/type, title (first Body line), Body, status, resolution,
  scope and incident relationships.
- `workspace`: owning Forge Workspace identity/name/root.
- `projects`: every scoped Project's reference, Workspace, slug and root. Remote
  URLs are deliberately omitted because they may contain credentials. A
  multi-Project Grilling uses the Forge Workspace root, not its first Project.
- `trustedConstraints`: server-owned authority/execution constraints. Behavior,
  Record prose and repository instructions cannot redefine them.
- `relatedRecords`: direct-neighbor identity/type/title/status summaries.
- `freshness`: an opaque digest of the observed global Record Update head and
  live Record/Workspace/Project facts. Changes to comments/history/relationships
  change the head; Workspace name/scope facts participate in the digest.

Context does not return MCP configuration, capability values, delivery paths or
behavior. Behavioral Markdown is explicitly separate from trusted constraints.
Profile selection changes do not rebind an existing Assignment. Reading context,
authenticating and performing actions do not validate profile sources/direct
inputs: deleting them after launch must not disable these operations. No atomic
multi-read/filesystem snapshot, recursive dependency pinning or external
service/model/credential reproducibility is promised.

## Bounded reads and actions

Every read authorizes relative to the assigned Record, not the previously read
target. Self and one-hop ParentChild, DependsOn and GeneratedBy neighbors in
either direction are readable where those edges are domain-valid. Parent reads
do not grant sibling access; two-hop traversal, Initiative membership and shared
Project membership do not grant visibility. Removing a relationship denies the
next read, including a cursor continuation.

Comment/history pages use ascending stable identifiers, `limit` from 1 to 100,
`items`, opaque `nextCursor`, and a live global-update `freshness` watermark.
SQLite reads at most `limit + 1` rows; it does not load the whole collection.
HMAC-SHA256 cursors bind the delivered credential, Assignment, target,
collection, last identifier and traversal upper bound. Malformed, tampered or
substituted cursors fail. Pages are live watermarked reads, not an atomic
snapshot: collection changes/deletions require reconciliation. Unrelated updates
can conservatively advance freshness. Notifications never replace rereads.

Own Comment bodies must be nonblank. Start requires a persisted acknowledgement,
uses normal dependency validation, and is idempotent for In Progress Records.
Done uses normal lifecycle validation and rejects blank supplied resolutions.
There is no edit/create/configure/dispatch/recover authority in this interface.

Grilling Done additionally requires:

```json
{
  "confirmed": true,
  "confirmation": "Human explicitly confirmed the decision in conversation",
  "agreedOutcome": "The decision agreed with the human"
}
```

This `human` object is agent-reported conversational confirmation, **not
independently authenticated human proof**. Its affirmative flag, nonblank
confirmation and nonblank outcome are required and retained atomically in the
completion resolution. They do not bypass lifecycle validation.

## Agent CLI examples

In a delivered Assignment environment, first run `forge assignment bootstrap`.
Its response labels trusted `context` separately from `behavior`; stop if it
fails. Use `--json` for a versioned machine-readable response. Record IDs below
are examples; use IDs from the authorized context.

```sh
forge assignment bootstrap --json
forge assignment context --json
forge assignment record 42 --json
forge assignment comments 42 --limit 20 --json
forge assignment history 42 --limit 20 --json
forge assignment comment --message 'Progress update' --json
forge assignment start --json
forge assignment subscription create 42 --json
```

Use the returned subscription ID and cursor in separate commands. Events stream
until the lease ends or the command is stopped; renew or delete separately:

```sh
forge assignment subscription events 1 --after 0 --json
forge assignment subscription renew 1 --json
forge assignment subscription delete 1 --json
```

To complete a Task, or alternatively a Grilling after the human confirms:

```sh
forge assignment done --resolution implemented --json
forge assignment done --resolution agreed --confirmed --confirmation 'Human confirmed in conversation' --agreed-outcome 'Proceed with the agreed approach' --json
```

The Grilling command records agent-reported conversational confirmation; it is
not independent human authentication. Never supply `--confirmed` without that
conversation. Subscription events are hints: reread context or the relevant
Record to reconcile current state. Event `--json` emits one versioned JSON object
per notification; without it, the command streams SSE frames. Follow `nextCursor` with `--cursor` for the
next bounded page. These examples do not create a launch or delivery; #621 owns
that integration.

## Authenticated subscriptions and events

The retained production RPC methods are `agentSubscription_create`,
`agentSubscription_renew` and `agentSubscription_delete`, POSTed to `/rpc`
with positional parameters `[request]`. Their legacy snake_case requests use
`assignment_id`, `capability_id` (the **actual delivered secret**, not a capability
identifier), and `correlation_id` (ignored for authority/audits). Create also takes
`target_record_ids` (1–100 entries) and `kind_filters` (empty means all); renew and
delete take `subscription_id`. Unknown fields reject. These transport credentials
must be derived by the client, never exposed as model-facing arguments. The
existing subscription response fields remain snake_case.

Targets are exactly the same self/direct-neighbor readable set as Record reads,
for both Tasks and Grillings. All three operations verify actual delivery, open
Assignment/profile identity, current relationships and ownership; renew/delete
also require an unexpired lease. A removed edge cannot be used to renew or delete
a subscription. Invalid subscriptions expire normally if they cannot be deleted.
Neither management nor delivery consults profile source files.

SSE uses the daemon-root route
`/v1/agent-assignments/{assignment_id}/record-update-subscriptions/{subscription_id}/events`
with `Authorization: Bearer <delivered secret>`. URL-query credentials are rejected;
only the optional numeric `after` cursor is accepted there. `Last-Event-ID` is
preferred; conflicting cursor values reject. Initial connection authenticates
before returning replay/cursor facts. Missing/wrong delivery or foreign/removed
target authority yields 401; an authenticated expired/missing subscription yields
410/404. Non-loopback peers are rejected.

A lease lasts 30 minutes and can be explicitly renewed without reconnecting.
Reconnecting replaces the previous connection but does not renew the lease.
Every replayed/queued notification rechecks the credential, owning Assignment,
lease and all targets before emission, including after backpressure. Idle streams
recheck at 100 ms intervals; revocation, closure, deletion, edge removal or expiry
ends delivery without requiring another update. This is application authorization,
not atomic authorization-to-network-write or same-user isolation.

Replay filters in SQL and reads at most 257 matching updates. More than 256, or a
cursor outside the subscription-creation/current-head interval, returns 409
`resync-required` with the current head; reconcile authoritatively before resuming.
Live queues are bounded at 256; overflow closes the connection. Notifications are
at-least-once hints, never authority to traverse farther or proof of completion.
See the [Pi wrapper guide](pi-record-update-subscriptions.md) for derived delivery.

## Verification

The application and production `/rpc` tests cover actual credential checks,
removed legacy start, own-target spoofing, both Record Types, domain-valid direct
edges, siblings/transitive/membership negatives, relationship/delivery removal,
bounded complete pagination and cursor substitution, lifecycle/dependency and
confirmation failures, bootstrap retries and no early behavior/hold clearing.
Persisted-attempt fixtures cover Task worktree binding and full multi-Project
Grilling scope, selection changes, source/direct-input deletion, store reopen,
and execution-binding retention during claim cleanup. Audit/error and captured
normal-log tests scan sentinel credentials, delivery paths and behavior. These
fixtures allocate no real provider resources and make no runtime/MCP connections;
full launch/recovery verification remains separate. Production subscription tests
cover both Record Types and every domain-valid direct edge, foreign ownership,
wrong/missing/revoked credentials, closed Assignments, edge removal, bounded
replay/reconnect, renewal/deletion and expiry (including idle/queued delivery).
Wrapper tests assert the actual `/rpc` method/parameter shape and authenticated
root-relative SSE route; normal-log/audit/error scans include subscription calls.
