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
Both Tasks and Grillings use this sequence. The Implementation Check's Assignment then
receives the minimal form described under
[Implementation bootstrap](#implementation-bootstrap); Review, Publication and Grilling
Assignments receive the full form described here. Each step can fail; failure does not
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
  `checkAttempt.continuation` is null or the latest accepted Implementation handoff
  captured at attempt creation: accepted attempt id, checkpoint evidence and the
  stopping reason, completed work, actual verification/unrun gates and remaining work.
  `checkAttempt.recovery` separately captures explicit operator salvage authorization.
  Its optional `retained_state.sha256` binds dirty/staged/untracked starting state,
  not a clean accepted checkpoint. Inspect and salvage that retained checkout;
  ordinary output submission still requires clean committed evidence.
  These are persisted configuration/ownership facts, **not runtime
  attestation**. The execution directory is retained once available by dispatch
  context construction (or the first authenticated context read for an older
  attempt), independently of later Worktree Claim cleanup state.
- `record`: live identity/type, title (first Body line), Body, status, resolution,
  scope and incident relationships. Prerequisite `kind` is explicitly `waits-for`
  or `builds-on`, matching operator Record inspection; the former `ordering_only`
  response flag is removed. Related Record reads use the same canonical contract.
  Historical Update Log relationship kinds remain historical.
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

## Implementation bootstrap

The Assignment bound to the Implementation Check authenticates, acknowledges, starts and
reads context exactly as above, but is returned (human-readable and `--json`) only:

- the execution directory (Managed Worktree path);
- the pinned Task body (`taskBody`);
- every open Finding in full (`findings`: id, severity, blocking, message);
- the Forge-owned command contract (`commandContract`);
- `retainedWorkRecovery`, only when the operator authorized retained-work recovery.

Trusted constraints remain enforced server-side but are not rendered. There is no Check
id, attempt id, generation, input evidence, related-Record list, Continuation checkpoint
or Agent Profile outcome language. The JSON has no `context` or `behavior` field.

The command contract is fixed Forge text in `forge_app::assignment`, identical for every
pinned profile revision: work only in the worktree; commit as you go; run
`forge assignment request-review` only when the Task and all open Findings are met; after
requesting review, end your turn and wait; if blocked, explain with
`forge assignment comment`. It names no other Forge command.

The pinned Implementation profile body is **omitted**, not rendered as guidance: existing
revisions embed outcome, Continuation and bootstrap instructions that would otherwise
surface as authority. The profile still selects launch configuration. Review and
Publication bootstraps are unchanged.

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

## Assignment sessions

`forge assignment bootstrap` reads optional `PI_SESSION_ID` and sends it as
`sessionId` in the authenticated bootstrap request. Missing identity preserves
existing bootstrap behavior. A supplied identity must be nonblank, no more than
256 bytes and contain no control characters; invalid identity fails bootstrap.

Successful bootstrap records an Assignment Session before returning context and
behavior. Persistence failure fails the request; retry is safe. Associations are
deduplicated by Assignment + session ID, retaining first and most recent successful
bootstrap timestamps (Unix milliseconds). These are observation times, not session
start/end times. A different session adds an association, and the same session may
appear on multiple Assignments.

`forge dispatcher assignment-info <assignment-id>` exposes all associations in
human output and JSON `sessions`, including `session_id`,
`first_bootstrap_at_millis` and `last_bootstrap_at_millis`. They remain with
Assignment history after closure; there is no independent removal operation.
Registration captures only sessions that bootstrap, not forks or other sessions
that never bootstrap. It is provenance, not ownership, authorization, heartbeat,
or proof of agent exit. Transcript lookup, navigation and session control are not
provided.

Wire compatibility: `sessionId` is optional, so existing bootstrap requests still
work. Inspection adds a default-empty `sessions` collection. New clients supplying
`sessionId` require a server supporting this field; older servers may reject it.

## Implementation profile delivery

Implementation Agents receive Forge-owned instructions at bootstrap, not profile outcome
guidance, and cannot submit outcomes (see below). The tracked
[Continuation profile source](profiles/implementation-continuations.md) and its
[handoff](profiles/implementation-continuations-handoff.md) are historical, not live
registered revisions, and do not alter existing immutable Check pins.

## Agent CLI examples

In a delivered Assignment environment, first run `forge assignment bootstrap`.
For an Implementation Assignment the output is the minimal form above. For other
Assignments its default output is agent-facing Markdown limited to facts the agent acts on:
execution directory and Projects, trusted constraints for the bound Record type,
the outcome identity and input evidence of the bound Check attempt (plus any
accepted Continuation checkpoint or authorized recovery), worker behavior, and
clearly labelled Record content. Diagnostic identity (Assignment ID, profile
revision, model, attempt number, freshness) is available only through `--json`.
The pinned Task body is shown once; if the live body differs, both are labelled
explicitly. Related Records are listed by title; read one with
`forge assignment record <id> --json` only when needed. Bootstrap and context inspection are preparation,
not completion of the bound work. Stop if bootstrap fails.

Use `--json` for the unchanged versioned machine-readable response, including the
full context and behavior. This presentation-only change does not change RPC,
authentication, lifecycle, profile revisions, or Check pins. Scripts that parsed
the former default pretty-printed JSON must explicitly pass `--json`.
Record IDs below are examples; use IDs from the authorized context.

```sh
forge assignment bootstrap
forge assignment context --json
forge assignment record 42 --json
forge assignment comments 42 --limit 20 --json
forge assignment history 42 --limit 20 --json
forge assignment comment --message 'Progress update' --json
forge assignment start --json
```

Review Check workers submit their bound Check outcome rather than calling Done:

```sh
forge assignment outcome --file outcome.json --json
```

Implementation workers never do. From an Implementation Assignment the server rejects
`assignment outcome` (every disposition), `check assess`, `check invalidate`,
`finding raise` and `finding mutate` with `implementation-outcome-forbidden`, changing
nothing. They may use only `request-review`, `comment`, `context`, `finding list` and
`record`.

An Implementation worker whose committed work is ready finishes with a Review Request,
which needs no outcome file and states no verdict:

```sh
forge assignment request-review --json
```

Commit all work first: a dirty or invalid Managed Worktree is rejected with
`review-request-worktree-invalid` and nothing is recorded. Forge records the clean HEAD it
observes as the Implementation Check's pass and hands the work to the review Checks. Only
the Implementation Check's Assignment may use it (`review-request-not-implementation`
otherwise). After requesting review, end your turn and wait. See
[Review Requests](dispatch.md#check-attempts-and-controlled-acceptance).

Review outcomes use the actual `bound.checkAttempt` id/generation and repeat the bound
input evidence. An `interrupted` outcome likewise repeats its bound input evidence;
changed evidence is rejected with `interrupted-evidence-changed`, without acceptance or
retry-budget effects, and Forge preserves the checkout. Implementation work interrupted
by safe retirement without an outcome requires the operator's explicit
`dispatcher recover-retained-work` authorization after verified retirement/resource
reconciliation. Dirty salvage is content-bound and transfers inspection authority, not
correctness. Preserve the work and ask the operator; do not reset or discard changes.
Exact replay of already accepted historical review outcomes remains supported and confers no
new authority; an Implementation Assignment cannot replay an outcome through `outcome`.
See [outcome shape and handback rules](dispatch.md#check-attempts-and-controlled-acceptance).
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

## Task stack reads and publication

`forge assignment stack --json` reads only the bound Task's relationship-derived
Stack, including its inherited destination, Base Branch, pinned Base Commit and
verified publication or unchanged result. Resolving transitive repository lineage
does not grant transitive Record access. It does not configure membership or expand
related-Record visibility. Authenticated start rechecks required predecessor hosting
evidence even when the Task is already In Progress; stale or deleted evidence does
not fall back to the Project checkout.

`forge assignment stack --url <actual-pr-url> --json` is restricted to the live,
bootstrapped final Publication Check Assignment. Forge independently verifies the
open non-draft PR and remote branch, then transactionally rechecks bound ownership,
attempt freshness and output evidence before recording publication.
`forge assignment stack --unchanged --json` uses the same final-Check authority
but independently verifies a clean Managed Worktree at the exact prepared input
commit instead of observing a PR. `--url` and `--unchanged` are mutually exclusive.
Both paths still require the ordinary bound Publication outcome; verification does
not itself pass a Check. Operator publication requires Manual Execution Mode and
safe ownership. No capability
secret belongs in command arguments or diagnostics. See [Task stacks](stacks.md)
for setup and retry behavior.

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
