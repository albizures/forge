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
in its working directory; set `FORGE_STORE_PATH` to choose another path.

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

## Agent Dispatch Core handoff

This release provides the durable, daemon-owned **Agent Dispatch Core**. It
stores Task-assignment provenance, enforces per-Project capacity, and exposes
configuration and diagnostics through the daemon. It does **not** yet launch or
control a provider agent: the current daemon reports a configured endpoint as
`configured; provider adapter unavailable`.

### Operator configuration and diagnostics

Configure an absolute Herdr Unix-socket path through the daemon, then inspect
its configuration. Relative paths are rejected:

```sh
forge agent-dispatch endpoint --socket-path /absolute/path/to/herdr.sock
forge agent-dispatch endpoint
```

Each registered Project has an effective Task capacity of **1** unless it has
an explicit override. Set an override from 1 through 1024, inspect the
configured-versus-default value, or omit `--limit` to return the Project to
the default:

```sh
forge agent-dispatch project --project alpha --limit 2
forge agent-dispatch status --project alpha
forge agent-dispatch project --project alpha
```

`forge agent-dispatch reconcile` requests one bounded reconciliation pass from
the daemon. The CLI neither opens dispatch storage nor contacts Herdr directly;
the daemon owns the reconciliation lease and durable state transitions. It is
therefore safe to request from an external scheduler, but it cannot provide
provider behavior until the provider adapter is installed.

### Security and provider boundary

An open assignment has an opaque capability. The daemon is the authorization
gateway: a capability may read only its bounded Assignment Context and may add
Comments to or complete only its bound Task. Closed, replaced, abandoned, or
revoked assignments lose that authority immediately. Authorization outcomes
are retained as redacted audit facts; neither capability secrets nor Comment
bodies are retained in that audit history. A provider receives a capability via
a daemon-created, permission-restricted secret file outside the Agent
Workspace.

The follow-up Herdr/Pi provider work owns all concrete provider behavior:
protocol-22 endpoint probing, workspace/tab/pane and agent resource discovery,
provisioning and cleanup, and delivery of the initial prompt and capability to
the hosted Pi agent. It must implement the core's provider-neutral adapter
contract. Do not infer any of those behaviors from a configured socket or a
successful core reconciliation request.

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
