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
[Forgeup locations](forgeup.md#locations-and-recovery). For example,
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
[Replacement Registration](forgeup.md#register-a-revision) before
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

For durable-profile preview/registration and, when launch is composed, as the
launched runner's model credential, a manual daemon may set `FORGE_DURABLE_OPENAI_KEY_FILE` to an explicit protected key-file **path**.
Never put the key value in the environment. See the
[profile/authentication contract](agent-profile-policy.md#durable-profiles-and-authentication)
for permissions, supported inputs and fail-closed behavior. This setting neither
enables durable dispatch nor supplies a Forge Assignment capability. Managed
launches do not inherit it and have no equivalent yet; see the
[supported configuration](durable-execution.md#supported-configuration).

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
or automatic recovery. A manual daemon that also sets both
`FORGE_DURABLE_RUNNER_HOST` and `FORGE_DURABLE_RUNNER_ENTRY` (absolute paths to the
matched `forge-runner-host` and `check-entry.ts`) composes durable launch and
automatic same-attempt recovery; new durable launches still stay withheld by the
[release gate](durable-execution.md#current-release-state). See [effect investigation](assignment-recovery.md#interrupted-durable-effects-controlled-implementation).

### Matched runner packaging

Build the bundle with the pinned Node 24.18.0 on `PATH`:

```sh
cargo build --release -p forge-runner-host
node runner/package-bundle.mjs --out DIR --forge-commit <40-hex SHA> \
  --runner-host target/release/forge-runner-host
```

The packager assembles a **schema 2** bundle:

- the pinned Node executable (`node/node`);
- the `forge-runner-host` supervisor (`host/forge-runner-host`), which must
  report runner protocol 1 and the same commit through `--compatibility`;
- the runner sources (`runner/src`);
- the `npm ci --omit=dev --ignore-scripts --no-bin-links` dependency closure,
  plus the derived read-only adapter.

`forge-runner-bundle.json` records:

- the runner protocol, Node version and Forge commit;
- the lockfile hash and a content tree hash;
- the **runner component hash**. This is the `component_hash` that every
  durable Launch records, as attested by the bundled sources with the bundled
  Node.

`forgeup register ... --runner-bundle DIR` verifies the bundle against the
candidate Revision before it copies anything:

- commit, protocol 1, Node version, lock and tree hash;
- no symlinks or special files;
- the packaged host's compatibility and Revision;
- the recomputed runner component hash.

It then copies the bundle into the immutable Installation and verifies the
*installed copy* again. Schema 1 bundles still register. They have no host
and serve only retained inspection, never a launch.

`forgeup daemon start` re-attests the whole bundle and then passes the
Installation's components to the managed daemon:

- **schema 2:** `--durable-runner-bundle ROOT`;
- **schema 1:** `--durable-node`/`--durable-effect-inspector`.

For a schema 2 Revision, Forgeup also passes `--durable-retained-runner-bundle
ROOT` for every other registered Revision whose schema 2 bundle verifies (see the
[management protocol](../crates/docs/management-protocol.md#optional-durable-runner-inputs)).
A missing, modified or mismatched component blocks startup with a redacted
diagnostic. Global `PATH`, npm and the checkout are never consulted.

**Launch preflight.** Before any allocation, the daemon checks the selected
components on every durable profile validation, adapter probe and provision:

- every component is present at its canonical path;
- the host and Node files are unchanged since the daemon composed them;
- Node is exactly v24.18.0;
- the host reports this daemon's runner protocol, runtime schema and Forge
  Revision;
- the runner sources still attest the composed component hash.

A failure is `durable-components-incompatible` and names the component.

**Retained Assignments after a Revision switch.** The daemon serves an existing
Launch only with the configured bundle whose runner component hash equals the
Launch's `component_hash`. This covers transcripts, effect inspection and
resolution, retirement reconciliation and recovery. When no configured bundle
matches, the operation refuses with `durable-components-unavailable`, naming the
hash. Find the Revision that shipped it with `forgeup runner resolve
--component-hash H`, which accepts either a Launch component hash or a bundle
tree hash. Register that Revision and restart the daemon.

**Other commands.**

- `forgeup runner verify SELECTOR` re-attests a Revision's runner on demand.
- `forgeup remove` refuses to delete a Revision's runner while its state exists,
  unless `--purge-state` is given.
- Selecting another Revision never changes a retained Assignment's components.

**Compatibility.** The management protocol stays at revision 1. The new daemon
inputs are optional, and Forgeup passes them only to a daemon built from the
bundle's own commit. Runner wire protocol 1 and the storage schemas are
unchanged.

None of this enables durable dispatch. A managed daemon receives a model
credential only from `forgeup daemon start --durable-openai-key-file PATH`,
passed as the daemon's `--durable-openai-key-file`.

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

Durable-execution tests have environment prerequisites:

- **Real Node 24.18.0 first on `PATH`.** Several tests spawn `node` with a cleared
  environment. A Volta shim refuses to run that way, so every `durable_check_bootstrap` test
  fails. Prepend the real Node `bin` directory, for example
  `PATH="$(dirname "$(volta which node)"):$PATH" cargo test --workspace`.
- **Owner-only `.forge`.** Durable path validation correctly rejects a group-writable
  ancestor. Under a `0002` umask, fixtures must create an owner-only `.forge` before anything
  beneath it.

Runner checks, from `runner/`: `npm run check && npm test`. After changing pi-durable, pi-ai,
chord or Node, also run `npm run test:upstream`
([upstream constraints](durable-execution.md#upstream-pi-durable-152-constraints)).

### Real-provider release gate

Durable release verification also requires a real-provider Implementation → independent
Review cycle on the exact Revision
([requirements](durable-execution.md#release-verification-requirements)). It is opt-in and
billed, so run it only with operator authorization:

```sh
cargo build -p forge-runner-host   # must be built from the same Revision
FORGE_REAL_PROVIDER_KEY_FILE=/abs/owner-only-dir/openai.key \
  cargo test -p forge-cli --test durable_real_provider -- --nocapture
```

Without the variable the test prints a skip line and passes. It needs a clean tree (a dirty tree voids the run),
an owner-only `0600` key file that is never printed, and network access to OpenAI. It composes
the production durable path with the shipped runner entry, the real host and the real CLI and
daemon in scratch Workspaces under `/tmp`; the only test-only substitution is launch
eligibility (`LaunchControl::enable_controlled`). Each run writes an `evidence-summary.json`.
Record the Revision, component identities, commands, every run (including failures and their
cause) and any unrun gates in `docs/release-verification/<revision>.md`.

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
