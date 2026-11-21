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

For controlled durable-profile preview/registration only, a manual daemon may
set `FORGE_DURABLE_OPENAI_KEY_FILE` to an explicit protected key-file **path**.
Never put the key value in the environment. See the
[profile/authentication contract](agent-profile-policy.md#durable-profiles-and-authentication-controlled-pre-dispatch-boundary)
for permissions, supported inputs and fail-closed behavior. This setting neither
enables durable dispatch nor supplies a Forge Assignment capability. Managed
launches do not inherit it; matched managed packaging is later release work.

For controlled interrupted-effect resolution, a manual daemon may set both
`FORGE_DURABLE_NODE` and `FORGE_DURABLE_EFFECT_INSPECTOR` to explicit absolute
trusted executable/entrypoint paths. Partial or relative selection rejects;
there is no PATH/npm/current-checkout discovery. The retained runtime's component
hash must match the inspector, which uses committed read-only storage. The paths
are host configuration, never RPC input. Managed launches do not inherit these
ambient settings. A Revision registered with `forgeup register --runner-bundle` instead
receives its own attested components, which `forgeup daemon start` passes explicitly
(see [matched runner packaging](#matched-runner-packaging)). Missing
components block new resolutions, but exact accepted-result audit reads still work.
This configuration supplies no model key/capability and enables no durable dispatch
or automatic recovery. See [effect investigation](assignment-recovery.md#interrupted-durable-effects-controlled-implementation).

### Matched runner packaging

`node runner/package-bundle.mjs --out DIR --forge-commit <40-hex SHA>` (run with the pinned
Node 24.18.0 on `PATH`) assembles the pinned Node executable, runner sources and the
`npm ci --omit=dev --ignore-scripts` dependency closure plus the derived read-only adapter into a
bundle with `forge-runner-bundle.json` (runner protocol, Node version, Forge commit, lockfile
hash and a content tree hash). `forgeup register ... --runner-bundle DIR` verifies it against
the candidate Revision (commit, protocol 1, Node version, lock, tree hash, no symlinks), copies it
into the immutable Installation and re-verifies the *installed copy*. `forgeup daemon start`
re-attests the whole bundle and then launches the managed daemon with
`--durable-node`/`--durable-effect-inspector` pointing inside the Installation; any missing,
modified or mismatched component blocks startup with a redacted diagnostic. Global `PATH`, npm
and the checkout are never consulted. `forgeup runner verify SELECTOR` re-attests on demand and
`forgeup runner resolve --component-hash H` finds the Revision that shipped exactly that tree.
`forgeup remove` refuses to delete a Revision's runner while its state exists unless `--purge-state`
is explicit, and selecting another Revision never changes a retained Assignment's component.
The management protocol is unchanged (the new daemon flags are optional and only passed to a daemon
built from the same attested commit); runner wire protocol 1 and storage schema are unchanged.
This does not enable durable dispatch or automatic recovery.

## Browser and RPC

On the default manual listener, open <http://127.0.0.1:45003/>. Browser routes
and JSON-RPC share one listener. The server-rendered status page uses locally
served HTMX and refreshes when the user selects **Refresh status**. Its fragment
route is `/status`; the JSON-RPC endpoint is `POST /rpc`. Managed daemon ports
are allocated by Forgeup rather than fixed at 45003.

Open `/records` (or use the status page's **Records** link) for the read-only
Records dashboard. Select a Forge Workspace, optional Record Type/Status
checkboxes, and a date order, then submit **Apply selections (page one)**.
Selections within a filter are ORed; filters are ANDed; no selections mean all.
The default order is Recently updated. Least recently updated, Newest created,
and Oldest created are also available. Equal dates use ascending Record Identifier;
unknown historical dates sort last in every order and are never backfilled.
Cards show the relevant UTC date: creation is the opening Record Update;
last update is the latest Record Update, including Comment additions/deletions and
Relationship changes. Unknown historical dates are labeled, not guessed. No Workspace is selected implicitly, and no
cross-Workspace inventory is shown. All Record Types and Projectless work are
included before filtering. Matching results are counted and paginated at 50 cards.
Normal GET URLs preserve Workspace, repeated `type`/`status` parameters, `sort`,
and `page` (omitted values use defaults). Previous/Next and Refresh preserve
selections; applying the form resets page one. Refresh reads current data without
polling. Invalid query/page input returns 400; missing Workspaces return 404 and
read/render failures return 500, never a successful empty inventory. No registered
Workspaces, no selected Workspace, no matching Records, and unavailable data have
distinct messages. Valid-query error pages retain a retry URL and a return URL;
an out-of-range page offers return to page one with selections preserved. Malformed
queries offer a clean dashboard entry rather than guessing the intended selections.

Open a card title to visit `/records/{id}`. Details show the full safely rendered
Markdown Body, identifier, type/status, Project Scope, UTC creation/update dates,
and optional Completion Resolution. Embedded HTML is displayed as text and unsafe
link/image schemes are disabled. **Return to Records** restores the originating
Workspace, filters, sort and page. Missing/foreign Records return a non-disclosing
404; failures return 500 with retry/return navigation. Browser Back and reload use
the ordinary GET URLs; no application-specific JavaScript is required. Cards become
one column on narrow screens; controls have visible labels and keyboard focus.
Long code blocks scroll within the detail view. Browser access is read-only, but
the shared unauthenticated listener also exposes mutating operator RPC: restrict
it to trusted clients as described in [Security](../README.md#security).
See [dashboard verification](records-dashboard-verification.md) for evidence and
current limitations.

CLI status calls `system.status` over JSON-RPC/HTTP and reports failures on
stderr, with a request timeout of at most 10 seconds.

## Maintaining skills

When explicitly developing Forge, inspect the source CLI with
`cargo run -p forge-cli --bin forge -- --help` from the repository root. Installed
skills use the installed executable instead.

Keep required skill references inside their skill directory and resolve other
skills by name. Use installed CLI help for syntax rather than duplicating the
manual in a skill. [CLI examples](cli-examples.md) remain repository documentation.
Their fenced `sh` blocks and those in the skills contain one complete command per
line, with concrete illustrative arguments and shell quoting; they are parser
fixtures, not a batch script. `cargo test -p forge-cli --bin forge documented_skill`
checks their syntax without invoking handlers; it does not establish operational
safety or authorization.

`python3 -m unittest discover -s tests -p 'test_skill_*.py'` copies each skill into
an isolated directory and checks local documentation links without sibling skills
or repository docs. Fenced examples describe destination-project files rather
than installation dependencies and are excluded from link checks.

## Verification

Run from the repository root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Historical runs have encountered ownership-lock/listener failures under
parallel tests (`WouldBlock` / `AddrInUse`) and Forgeup executable registration
failures (`Text file busy`). These are not blanket waivers: the Manual release
verification passed both strict Clippy and the default-parallel workspace suite.
A serial run can help diagnose a failure, but does not substitute for a passing
default-parallel gate:

```sh
cargo test --workspace -- --test-threads=1
```

Recheck these limitations against the current checkout; historical test results
are not release sign-off. Passing local checks does not establish package/install
verification, rollback readiness, or production migration/recovery safety. A
deployment owner must apply the release policy rather than silently waive gates.

Before opening an existing database with upgraded binaries, back it up under the
normal operational policy. Incompatible stores fail with `fresh-store-required`
before initialization; there is no automatic conversion or reset. Explicit managed
copy-and-upgrade is documented in [Revision State upgrades](state-upgrades.md),
including its separate activation, compatibility fence, and recovery tests. Safely retire
ownership and reconcile retained resources before any separate operator reset.
Preserve recovery facts and never rewrite SQLite markers to bypass compatibility,
ownership, or capacity checks.
