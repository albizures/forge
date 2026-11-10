# Authenticated Assignment context and actions

Implemented application/RPC boundary for Spec #614, Task #618. This is **not yet
an integrated launch feature**: the Assignment CLI is #620, subscription/event
security is #619, and protected Task/Grilling launch integration is #621.
Registered-policy dispatch remains disabled. The flags-only runtime limitations
in [Agent Profile policy](agent-profile-policy.md) are unchanged.

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
arguments or instructions to paste secrets into tools**. #620 must derive them
automatically. Requests reject unknown fields, so own mutations have no target
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
routes are explicitly deferred to #619; they are not certified by this Task.

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
full launch/recovery and subscription verification remain separate Tasks.
