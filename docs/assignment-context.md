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
redacted stop/review message rather than caller-supplied diagnostics.

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
Launch and acknowledgement races retain one-shot launch and recovery safeguards.

Context separates:

- `bound`: Assignment/Record identity, immutable per-Record-Type profile revision,
  provider/kind/model, attempt number, execution directory and provider Workspace
  identity. Task `bound.checkAttempt` binds the immutable attempt/Check identity,
  starting freshness generation, input evidence and Task body; the Assignment
  revision is that Check's pin, never a live Workspace Task selection.
  These are persisted configuration/ownership facts, **not runtime
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
can conservatively advance freshness.

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
```

Task workers submit their bound Check outcome rather than calling Done:

```sh
forge assignment outcome --file outcome.json --json
```

Use the actual `bound.checkAttempt` id/generation/input and the clean authorized
output evidence. See [outcome shape and handback rules](dispatch.md#check-attempts-and-controlled-acceptance).
Implementation cannot self-verify review Findings. Reviews cannot modify the
checkout. Final review acceptance leaves In Progress until Forge separately
reconciles retirement/resources and every completion gate. Exact accepted replay
is replay-only after closure; closed Assignment reads/new mutations remain revoked.

To complete a Grilling after the human confirms:

```sh
forge assignment done --resolution agreed --confirmed --confirmation 'Human confirmed in conversation' --agreed-outcome 'Proceed with the agreed approach' --json
```

The Grilling command records agent-reported conversational confirmation; it is
not independent human authentication. Never supply `--confirmed` without that
conversation. Reread context or the relevant Record to reconcile current state.
Follow `nextCursor` with `--cursor` for the next bounded page. These examples
require an existing launch and delivery.

## Subscription removal

Record Update Subscriptions, their management RPCs, SSE endpoint, CLI commands,
and Pi extension have been removed without compatibility shims or a replacement
notification mechanism. Database initialization drops subscription tables and
subscription-specific authorization audits. Records, Update Logs, Assignments,
and unrelated authorization audits remain intact. Back up existing databases
before upgrading; removed subscription state cannot be recovered by downgrading.

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
full launch/recovery verification remains separate. Removal tests verify absent
subscription RPC/HTTP/CLI interfaces and database cleanup while preserving
Records, Update Logs, Assignments, and unrelated audit entries.
