# Development and source setup

## Build and register a Revision

Install Rust 1.98.0 or newer on macOS, Windows, or Linux. The repository selects
its toolchain through `rust-toolchain.toml`.

From the repository root, build Forgeup and a matching CLI/daemon pair together:

```sh
cargo build --release -p forgeup -p forge-cli -p forge-server
```

The following examples use a POSIX shell. On Windows, use the corresponding
shell syntax and executables with the `.exe` suffix.

```sh
export PATH="$PWD/target/release:$PATH"
forgeup register --source-label local \
  --cli "$PWD/target/release/forge" \
  --daemon "$PWD/target/release/forge-server" \
  --local-build-alias dev
```

Forgeup snapshots both executables. Select that registered Revision:

```sh
forgeup use dev
```

Put Forgeup's **managed bin directory before `target/release` on `PATH`** so
`forge` resolves to the selected snapshot rather than a later local rebuild.
Find the platform-specific location in
[Forgeup locations](../crates/docs/forgeup.md#locations-and-recovery). For example,
on Linux with default XDG locations:

```sh
export PATH="$HOME/.local/share/forgeup/bin:$PATH"
forgeup daemon start
forge status
```

Persist the appropriate PATH entries in your shell configuration. Ordinary
`forge` commands attest their executable to the Active Forge Revision, ensure
its managed daemon, and use its registered endpoint. An absent selection, stale
CLI, invalid registry, or daemon failure produces recovery guidance; there is
no fallback to a default manual endpoint. Use `forgeup doctor` and `forgeup logs`
for diagnosis. `forge --build-info --json` works offline.

Rebuilding does not update an existing immutable registration. See
[Replacement Registration](../crates/docs/forgeup.md#register-a-revision) before
replacing artifacts under the same selector; retained state is not disposable.

## Run a manual daemon

For direct development without managed lifecycle resolution:

```sh
cargo run -p forge-server --bin forge-server
```

In another shell, explicitly target that daemon:

```sh
export FORGE_SERVER_URL=http://127.0.0.1:45003/rpc
cargo run -p forge-cli --bin forge -- status
```

`--server-url` overrides `FORGE_SERVER_URL`. Either bypasses Forgeup lookup,
attestation, and lifecycle work completely. Unset `FORGE_SERVER_URL` before
returning to managed usage.

The default listener is `127.0.0.1:45003`. Set `--bind-addr` or `FORGE_BIND_ADDR`
to change it; the flag wins:

```sh
cargo run -p forge-server --bin forge-server -- --bind-addr 127.0.0.1:45004
cargo run -p forge-cli --bin forge -- status --server-url http://127.0.0.1:45004/rpc
```

Stop the daemon with Ctrl-C; SIGTERM also requests graceful shutdown where
supported. A non-loopback bind is unauthenticated: use only an explicitly
trusted private network. Startup logging warns about this boundary.

A manual daemon defaults to `forge.sqlite3` in its working directory;
`FORGE_STORE_PATH` selects another store. Startup takes a non-blocking exclusive
advisory sidecar lock and rejects a second owner before binding the listener.
This prevents competing daemon ownership, not deliberate direct SQLite access.
Managed daemons instead use Forgeup's isolated per-Revision state.

## Browser and RPC

On the default manual listener, open <http://127.0.0.1:45003/>. Browser routes
and JSON-RPC share one listener. The server-rendered status page uses locally
served HTMX and refreshes when the user selects **Refresh status**. Its fragment
route is `/status`; the JSON-RPC endpoint is `POST /rpc`. Managed daemon ports
are allocated by Forgeup rather than fixed at 45003.

CLI status calls `system.status` over JSON-RPC/HTTP and reports failures on
stderr, with a request timeout of at most 10 seconds.

## Verification

Run from the repository root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Known verification limitations include existing `forge-app` strict-Clippy debt
and intermittent server ownership-lock/listener failures under default-parallel
workspace tests (`WouldBlock` / `AddrInUse`). A serial run can help diagnose the
latter, but does not substitute for a passing default-parallel gate:

```sh
cargo test --workspace -- --test-threads=1
```

Recheck these limitations against the current checkout; historical test results
are not release sign-off. Passing local checks does not establish package/install
verification, rollback readiness, or production migration/recovery safety. A
deployment owner must apply the release policy rather than silently waive gates.

Before opening an existing database with upgraded binaries, back it up under the
normal operational policy. Incompatible stores fail with `fresh-store-required`
before initialization; there is no automatic conversion or reset. Safely retire
ownership and reconcile retained resources before any separate operator reset.
Preserve recovery facts and never rewrite SQLite markers to bypass compatibility,
ownership, or capacity checks.
