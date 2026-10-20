# Forge

Forge is a daemon-first Rust application foundation. One running `forge-server`
daemon exposes the same read-only status through a Rust CLI, JSON-RPC, and a
server-rendered browser page.

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

## Browser and RPC access

Open <http://127.0.0.1:45003/> in a browser. The page is server-rendered and
uses its locally served HTMX asset to refresh the status only when the user
selects **Refresh status**. The fragment route is
<http://127.0.0.1:45003/status>; the typed JSON-RPC endpoint is `POST /rpc`.

## Security and scope

This initial foundation deliberately uses plaintext HTTP and binds to loopback
by default. It is a read-only, unauthenticated status interface. Explicit
trusted Tailscale/private-network use is the only intended remote exposure.

Authentication, authorization, TLS termination, browser mutations, CSRF
handling, CORS access, persistence, CI, and service management are deferred.
The exposure boundary must be redesigned before adding mutations or broader
network access.

## Verify the workspace

Run the quality gate from the repository root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
