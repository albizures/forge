# Forge

Forge is a daemon-first Rust application foundation. One running `forge-server`
daemon exposes status and supported Project and Record operations through a Rust
CLI and JSON-RPC; its browser page currently exposes read-only status.

For the released Forgeup-facing management interface, see
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
SIGTERM also triggers graceful shutdown where the platform supports it.

To select a bind address, use `--bind-addr` or `FORGE_BIND_ADDR`. The flag
wins when both are supplied:

```sh
cargo run -p forge-server --bin forge-server -- --bind-addr 127.0.0.1:45004
FORGE_BIND_ADDR=127.0.0.1:45004 cargo run -p forge-server --bin forge-server
```

A non-loopback bind is unauthenticated. Use it only for an explicitly trusted
private network such as Tailscale; startup logging warns about this boundary.

## Read status

With the daemon running, use the CLI:

```sh
cargo run -p forge-cli --bin forge -- status
cargo run -p forge-cli --bin forge -- status --json
```

The CLI always calls `system.status` through daemon JSON-RPC/HTTP; it never
starts or bypasses the daemon. Its default endpoint is
`http://127.0.0.1:45003/rpc`. Point it at another daemon with `--server-url`
or `FORGE_SERVER_URL`; the flag wins:

```sh
cargo run -p forge-cli --bin forge -- status --server-url http://127.0.0.1:45004/rpc
FORGE_SERVER_URL=http://127.0.0.1:45004/rpc cargo run -p forge-cli --bin forge -- status
```

If no daemon is reachable, `forge status` reports the problem on stderr and
exits nonzero after at most the 10-second request timeout.

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

Use `child-of` relationships to make a hierarchy. A child has at most one
parent; parent and child must have identical Project Scopes. Hierarchies may be
nested and cannot contain cycles. A parent cannot be completed while any child
is not done, although relationships can be corrected after completion.

```sh
cargo run -p forge-cli --bin forge -- record relate 2 --kind child-of --to 1
cargo run -p forge-cli --bin forge -- record unrelate 2 --kind child-of --to 1
```

Discovery is intentionally limited to optional Project Scope membership and
status filters, which combine with AND. Results are newest first. There is no
full-text search, type-specific workflow, deletion, scope editing, comments, or
history yet.

```sh
cargo run -p forge-cli --bin forge -- record list --project alpha --status open
```

Record commands support `--json`; use `--server-url` or `FORGE_SERVER_URL` to
target a non-default daemon as described above.

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
