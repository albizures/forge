# Forge

Forge is a daemon-first Rust application foundation. One running `forge-server`
daemon exposes status and supported Project and Record operations through a Rust
CLI and JSON-RPC; its browser page currently exposes read-only status.

For Forgeup release and operational guidance, see
[`crates/docs/forgeup.md`](crates/docs/forgeup.md). The Forge-side Management
Protocol is specified in
[`crates/docs/management-protocol.md`](crates/docs/management-protocol.md).

## Prerequisites

- Rust 1.98.0 or newer (the repository selects its toolchain with
  `rust-toolchain.toml`)
- A supported development platform: macOS, Windows, or Linux

## Start the daemon

From the repository root, start the daemon:

```sh
cargo run -p forge-server --bin forge-server
```

It listens on `127.0.0.1:45003` by default. The daemon serves its browser
routes and JSON-RPC endpoint from that one listener. Stop it with Ctrl-C;
SIGTERM also triggers graceful shutdown where the platform supports it. This is
a manual daemon: pass `--server-url http://127.0.0.1:45003/rpc` (or export
`FORGE_SERVER_URL` with that value) for the direct `cargo run ... forge`
examples below, unless you have selected a managed Revision.

To select a bind address, use `--bind-addr` or `FORGE_BIND_ADDR`. The flag
wins when both are supplied:

```sh
cargo run -p forge-server --bin forge-server -- --bind-addr 127.0.0.1:45004
FORGE_BIND_ADDR=127.0.0.1:45004 cargo run -p forge-server --bin forge-server
```

A non-loopback bind is unauthenticated. Use it only for an explicitly trusted
private network such as Tailscale; startup logging warns about this boundary.

## CLI endpoint modes

Ordinary `forge` commands use the selected **Active Forge Revision** by
default. The CLI reads the active Forgeup installation, attests that its own
hash and Revision Identity match it, ensures that Revision's managed daemon,
and uses its registered endpoint. Select a Revision first with `forgeup use
<selector>`; see [`crates/docs/forgeup.md`](crates/docs/forgeup.md) for
registration and recovery.

An explicit endpoint is **Manual Daemon** operation. `--server-url` takes
precedence over `FORGE_SERVER_URL`; either bypasses Forgeup lookup, attestation,
and daemon lifecycle work completely:

```sh
cargo run -p forge-cli --bin forge -- status --server-url http://127.0.0.1:45004/rpc
FORGE_SERVER_URL=http://127.0.0.1:45004/rpc cargo run -p forge-cli --bin forge -- status
```

Use this mode for a daemon you started yourself. If neither override is set,
there is no active Revision, the CLI is stale, the registry is invalid, or
Forgeup cannot ensure the daemon, `forge` fails closed with recovery guidance;
it never falls back to `http://127.0.0.1:45003/rpc`. `--build-info --json` is
the sole offline command and remains available without Forgeup or a daemon.

With a reachable endpoint, status calls `system.status` through daemon
JSON-RPC/HTTP and reports failures on stderr after at most the 10-second
request timeout.

## Register and confirm a Project

A Forge Project is a local Git repository's **main worktree**, identified by an
immutable slug. It must have an `origin` remote. With the daemon running,
register it through the CLI:

```sh
cargo run -p forge-cli --bin forge -- add project /repos/alpha alpha
```

Forge confirms the registration and prompts you to run `forge here`. From the
registered folder or any subfolder, confirm the Project that contains the
current directory:

```sh
cd /repos/alpha/src/module
cargo run -p forge-cli --bin forge -- here
# Project `alpha` is registered at `/repos/alpha` (origin: `...`).
```

`forge here` exits nonzero with an actionable error when the current folder is
outside every registered Project. Registration also exits nonzero for a
non-Git folder, a linked (non-main) worktree, a repository without `origin`,
or conflicting Project slug or Registered Folder identities.

The daemon persists registrations in SQLite. By default it uses `forge.sqlite3`
in its working directory; set `FORGE_STORE_PATH` to choose another path. One
Forge daemon owns a store at a time: startup holds a non-blocking exclusive
advisory sidecar lock and rejects a second daemon before listener binding. This
is a daemon-ownership boundary only; it does not prevent deliberate direct
SQLite access.

## Manage generic Records

A Record is a generic work item with a required, non-blank Markdown body and an
opaque numeric ID. Its Project Scope is selected explicitly when it is opened:
pass one or more registered Project slugs with repeatable `--project`, or pass
`--no-projects` to deliberately create an unscoped Record. Scope is immutable.

```sh
cargo run -p forge-cli --bin forge -- record open \
  --body 'Investigate the migration' --project alpha
cargo run -p forge-cli --bin forge -- record open \
  --body 'General research' --no-projects
cargo run -p forge-cli --bin forge -- record show 1
cargo run -p forge-cli --bin forge -- record edit 1 --body 'Revised Markdown body'
```

Records begin `open` and advance only through `in-progress` to `done`:

```sh
cargo run -p forge-cli --bin forge -- record start 1
cargo run -p forge-cli --bin forge -- record done 1 --resolution 'completed'
```

A completed Record's body and optional completion resolution cannot change.
Records are retained rather than deleted; complete mistaken Records with an
explanatory resolution.

## Manage Specs and Tasks

Specs and Tasks have dedicated, valid-by-construction commands. A Spec always
has exactly one Project. A standalone Task may have zero or one Project, while
a Task opened beneath a Spec atomically inherits that Spec's Project Scope and
parent relationship.

```sh
cargo run -p forge-cli --bin forge -- spec open \
  --body 'Deliver the migration' --project alpha
cargo run -p forge-cli --bin forge -- task open --body 'Investigate options'
cargo run -p forge-cli --bin forge -- task open \
  --body 'Project-specific handoff' --project alpha
cargo run -p forge-cli --bin forge -- task under-spec \
  --body 'Implement the migration' --spec 1
```

Human-readable open, show, and list output identifies each Record's type;
`--json` returns the type supplied by the daemon. Record Type and Project Scope
membership are selected at opening and cannot change later.

Typed hierarchy is also fixed at opening: a Spec may parent Tasks, and a Task
created with `task under-spec` receives its Spec parent and exact Project Scope
atomically. A Task cannot parent another Record. Typed Records cannot be added
to or removed from a relationship after opening; use the dedicated commands
rather than `record relate` or `record unrelate`.

Use `child-of` relationships to make a generic Record hierarchy. Only Generic
Records can be related this way. A child has at most one parent; parent and
child must have identical Project Scopes. Hierarchies may be nested and cannot
contain cycles. A parent cannot be completed while any child is not done,
although generic relationships can be corrected after completion.

```sh
cargo run -p forge-cli --bin forge -- record relate 2 --kind child-of --to 1
cargo run -p forge-cli --bin forge -- record unrelate 2 --kind child-of --to 1
```

Discovery is intentionally limited to optional Project Scope membership, status,
and Record Type filters, which combine with AND. Results are newest first. There
is no full-text search, deletion, or scope editing.

```sh
cargo run -p forge-cli --bin forge -- record list --project alpha --status open --type spec
```

Record commands support `--json`; use `--server-url` or `FORGE_SERVER_URL` to
target a non-default daemon as described above.

## Typed Record release notes

Existing SQLite databases migrate automatically when opened by this release:
pre-existing Records are classified as `generic`. The migration preserves their
IDs, bodies, Project Scopes, statuses, completion resolutions, relationships,
comments, and update logs. The release verification gate passed `cargo fmt
--check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test
--workspace`, and `git diff --check`, plus an isolated legacy-database smoke
test covering migration, typed creation, filtering, and invalid hierarchy
rejection.

## Generated-by provenance release notes

This release adds the directed `generated-by` Record Relationship. It records
that a generated Record was caused by another Record's work or output; it does
not affect hierarchy, lifecycle, readiness, dispatch, or dependency behavior.

Existing SQLite databases migrate automatically when opened. The migration
extends relationship and update-log constraints for `generated-by`; legacy
relationship updates retain an absent relationship kind rather than receiving
invented history. No operator action or data rewrite is required. Back up a
production database before upgrading under the normal operational policy.

For API and CLI clients, record-opening commands accept repeatable
`--generated-by <ID>` inputs, and relationship correction accepts
`--kind generated-by`. Additions and removals are permitted only while the
generated Record is Open. New relationship add/remove updates and subscription
payloads include their relationship kind; clients consuming update events
should tolerate its absence for migrated legacy events. Assignment Context
includes directly incident provenance Records, but subscription authorization
remains limited to direct prerequisites.

## Superseded pre-autonomous dispatch handoff

> **Superseded by the autonomous-dispatch release notes below.** This section records the earlier provider model and must not be used as operational guidance for new dispatches.

This release provides a durable, daemon-owned **Herdr/Pi dispatch provider**
behind the Agent Dispatch Core. The daemon, rather than the CLI, connects to
Herdr and owns all assignment state transitions, reconciliation leases, and
provider resources.

### Operator configuration, diagnostics, and recovery

Configure an absolute Herdr Unix-socket path through the daemon, then inspect
its configured endpoint. Relative paths are rejected:

```sh
forge agent-dispatch endpoint --socket-path /absolute/path/to/herdr.sock
forge agent-dispatch endpoint
```

`endpoint` reports configuration, not reachability: `configured; Herdr adapter
will probe on reconciliation` means a socket path is saved. Run a bounded,
authoritative reconciliation to probe Herdr and progress eligible assignments:

```sh
forge agent-dispatch reconcile
```

The CLI never opens dispatch storage or contacts Herdr directly. A successful
response reports whether the daemon acquired its reconciliation lease and the
number of assignments scanned; socket, protocol, or provider errors are
reported by the daemon and use its durable retry policy. The daemon also keeps
a long-lived Herdr status observer for active Forge-owned panes. It subscribes
before snapshotting and only schedules reconciliation; events never mutate an
assignment directly. On a socket or protocol failure it reconnects, and the
next authoritative scan recovers state from persisted resource IDs.

The adapter requires Herdr Socket API protocol **22** and schema version **1**.
A probe rejects an incompatible snapshot or malformed endpoint. Use the daemon
and Herdr diagnostics to correct the endpoint or upgrade the incompatible side,
then request reconciliation again.

Each registered Project has an effective Task capacity of **1** unless it has
an explicit override. Set an override from 1 through 1024, inspect the
configured-versus-default value, or omit `--limit` to return the Project to
the default:

```sh
forge agent-dispatch project --project alpha --limit 2
forge agent-dispatch status --project alpha
forge agent-dispatch project --project alpha
```

Use `forge agent-dispatch profile` to inspect the immutable seeded Task profile
(the `herdr` provider, `pi` agent kind, model, and prompt template).

### Superseded V1 ownership and safety boundary

For an eligible Task, Forge finds exactly one existing Herdr workspace matching
the registered Project folder. It never creates, adopts by label, or closes a
Project workspace. Within that workspace it creates one unfocused, dedicated
assignment tab and split pane, starts a Pi agent with the immutable profile,
and records workspace, tab, pane, and agent identifiers immediately.

Recovery and retirement trust only those persisted identifiers. Retirement
revalidates the resource tuple, releases the named agent, and closes only the
dedicated pane and tab. Conflicting or missing resources are diagnostics, not
permission to adopt or delete similarly named user resources.

An open assignment has an opaque capability. The daemon is the authorization
gateway: a capability may read only its bounded Assignment Context and may add
Comments to or complete only its bound Task. Closed, replaced, abandoned, or
revoked assignments lose that authority immediately. Authorization outcomes
are retained as redacted audit facts; neither capability secrets nor Comment
bodies are retained in that audit history. A provider receives the capability
through a daemon-created, permission-restricted secret file outside the Agent
Workspace.

Provider state is never Task completion: Pi admission, `agentblocked`, idle,
done, or other observed agent states only drive durable assignment
reconciliation. Completion remains an explicit Forge Task action authorized by
the assignment capability or a human workflow action.

## Autonomous Task-agent dispatch release handoff

This release supersedes the pre-autonomous provider guidance above. Forge selects the oldest dependency-ready, single-Project **Open** Task while Project capacity is available (default: one), atomically creates a durable Provisioning Assignment to hold that capacity, and prepares `feat/forge-<task-id>` in a Task Managed Worktree. The immutable Base Commit is captured from the Project Registered Folder `HEAD` when the Task opens; dispatch never runs Pi in that Registered Folder.

Forge retains the Task Branch, Managed Worktree claim, Assignment, provider-resource identities, diagnostics, Dispatch Holds, and shared Forge Dispatch Workspace in SQLite. Existing databases migrate these durable facts automatically; back up production data under the normal upgrade policy. No data rewrite or adoption of legacy workspace/worktree state is supported.

Herdr is a daemon-only provider adapter. Forge creates and retains one neutral-root Forge Dispatch Workspace, then a dedicated tab and split pane for each Assignment whose cwd is the prepared Task Managed Worktree. It persists every returned resource identity immediately, recovers only matching persisted tuples, and never adopts lookalike user resources. The complete protocol-22 contract and retirement sequence are in [`docs/research/herdr-adapter-socket-contract.md`](docs/research/herdr-adapter-socket-contract.md); the architectural decision is [`ADR-005`](crates/docs/adr/0005-autonomous-task-dispatch-recovery.md).

### Operators and recovery

Configure the worktree root and Herdr socket, then use bounded reconciliation:

```sh
forge config set worktreeRoot "$HOME/.forge/worktrees"
forge agent-dispatch endpoint --socket-path /absolute/path/to/herdr.sock
forge agent-dispatch reconcile
```

Use `forge agent-dispatch status --project <slug>` for capacity/configuration and `forge worktree info <task>` or `forge worktree doctor` for retained checkout evidence. Correct the endpoint, provider state, or Git checkout before an explicit recovery action. After resolving a Dispatch Hold, run `forge agent-dispatch resume <task-id>`; to retire an assignment's provider resources while retaining its Git evidence, run `forge agent-dispatch assignment-retire <assignment-id>`; and use `forge agent-dispatch task-reopen <task-id>` only for an unassigned manual In Progress Task.

Forge does **not** reset, rebase, delete, recreate, or automatically repair a dispatcher-created worktree/branch; does not adopt unknown provider resources; does not infer Task completion from a provider state; and does not automatically retire an assignment merely because its checkout is missing or invalid. A failed setup hook, branch collision, unavailable base, invalid checkout, or provider/workspace failure retains diagnostics and requires operator investigation. After three failed provisioning attempts Forge creates a durable hold; capacity is released only after the failed attempt is durably closed or explicit retirement finishes.

### Intentionally deferred V1 limits

This release does not provide automated Git evidence cleanup/repair, automatic provider-resource adoption or replacement, a mutable agent profile/prompt/model override, user-initiated cancellation semantics beyond assignment retirement, or automated resolution of conflicting/missing shared-workspace state. Events are wake signals only; authoritative reconciliation remains the source of truth. Final task completion and broader integration/release policy remain explicit Forge workflow actions.

## Manual Managed Worktree release handoff

This surface is for explicitly requested, non-dispatch worktrees. Dispatcher-created Task Managed Worktrees follow the autonomous handoff above and are never automatically rolled back or repaired.

This release replaces the unsupported deployed legacy worktree surface. Do not
use its `--path` or `--copy-manifest` arguments and do not migrate its existing
state: Forge reports that state as unsupported and never adopts it
automatically.

A **Managed Worktree** is Forge's durable checkout for one in-progress Task
scoped to exactly one Project. Forge creates it at
`<worktree-root>/<project-slug>/<task-id>` and retains one Worktree Claim for
that Task. Set the user-wide Worktree Root before creating the first checkout;
the configured value is visible through the daemon-backed CLI:

```sh
forge config set worktreeRoot "$HOME/.forge/worktrees"
forge config get worktreeRoot
```

Create a checkout with an explicit branch and base, then inspect or remove it:

```sh
forge worktree create 42 --branch task-42 --base main
forge worktree info 42
forge worktree list
forge worktree remove 42
```

All worktree commands accept `--json`. `forge worktree list --all` includes
claims that require attention. `forge worktree doctor` diagnoses those claims;
only `forge worktree doctor --repair` performs repair. Ordinary removal is
local and offline and refuses dirty checkouts, missing or invalid Git metadata,
unresolvable upstreams, and local commits ahead of upstream. Behind or
diverged status is informational. Inspect the ordered `blockers` and
`removable` fields from `info`, `list`, or `doctor`; use `remove --force` only
after deliberately accepting the reported risk.

For a manually created checkout, a Project Setup Hook is optional and runs in the new checkout through the
platform shell. It receives `FORGE_PROJECT_SLUG`, `FORGE_RECORD_ID`,
`FORGE_WORKTREE_PATH`, `FORGE_BRANCH`, and `FORGE_BASE`. A nonzero exit rolls
back the checkout and claim; hook output is not persisted. Configure or remove
the hook with:

```sh
forge project setup-hook set --project alpha 'cargo fetch'
forge project setup-hook clear --project alpha
```

A claim moves from `reserving` to `active`. If creation rollback or removal is
incomplete, it becomes `cleanup-required`; do not try to create a second
checkout or bypass the claim. Use `forge worktree doctor` to obtain the
recovery facts and explicitly repair or force-remove only when appropriate.

Release verification completed with `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test
--workspace`, and `git diff --check`.

## Browser and RPC access

Open <http://127.0.0.1:45003/> in a browser. The page is server-rendered and
uses its locally served HTMX asset to refresh the status only when the user
selects **Refresh status**. The fragment route is
<http://127.0.0.1:45003/status>; the typed JSON-RPC endpoint is `POST /rpc`.

## Security and scope

This initial foundation deliberately uses plaintext HTTP and binds to loopback
by default. The daemon is unauthenticated and exposes mutating Project and
Record RPC operations, so do not expose it beyond a trusted local environment.
Any remote use requires an explicitly trusted private network and a redesigned
security boundary.

Authentication, authorization, TLS termination, browser mutations, CSRF
handling, CORS access, CI, and service management are deferred.

## Verify the workspace

Run the quality gate from the repository root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
