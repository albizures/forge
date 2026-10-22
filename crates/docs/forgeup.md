# Forgeup operations

Forgeup is the separate program that registers immutable **Forge Revisions**
and selects one **Active Forge Revision**. It is not the `forge` command-line
interface. A registered Revision comprises a matched `forge` CLI and
`forge-server` daemon; Forgeup keeps its own snapshots and never runs the
candidate paths after registration.

This document describes the supported Forgeup surface. The daemon-side
Management Protocol is specified in
[`management-protocol.md`](management-protocol.md).

## Register a Revision

Build or otherwise obtain a matching CLI and daemon pair, then register both
paths:

```sh
forgeup register \
  --source-label local \
  --cli /path/to/forge \
  --daemon /path/to/forge-server \
  --local-build-alias dev
```

Forgeup runs `--build-info --json` on both inputs. It accepts them only when
the complete Revision Identities match and advertise supported Management
Protocol revision 1. It then hashes and copies both executables into
Forgeup-owned storage, records a manifest, and assigns a loopback endpoint.
The canonical Revision Selector is `<source-label>@<full-commit>`; a unique
local-build alias such as `dev` is also accepted where a selector is accepted.
Source labels and aliases contain only ASCII letters, digits, `.`, `_`, and
`-`.

Use these inspection commands:

```sh
forgeup list
forgeup current dev
```

Registering the same selector with the same artifact hashes is idempotent.
Registering different artifacts for it fails rather than overwriting an
immutable Revision Installation. To explicitly replace it, name the existing
selector (or its alias):

```sh
forgeup register --source-label local --cli /new/forge --daemon /new/forge-server \
  --replace local@<full-commit>
```

## Select a Revision and use its daemon

Select a registered Revision without starting anything:

```sh
forgeup use dev
```

This validates the stored artifact hashes, persists the desired active
selector, and reconciles the manager-owned `forge` command path. On Unix this
is an atomically replaced symlink; if a prior update was interrupted, the next
Forgeup command repairs the path from the registry. Ensure Forgeup's `bin`
directory is on `PATH` to invoke that selected CLI as `forge`.

Starting and stopping are separate, explicit operations:

```sh
forgeup daemon status
forgeup daemon start
forgeup daemon stop
forgeup logs
forgeup deactivate
```

`daemon start` starts only the Active Forge Revision. It uses that Revision's
isolated database and allocated loopback endpoint, captures stdout and stderr,
and removes inherited `FORGE_*` settings before launching. A process is
reported as managed only after Forgeup verifies its process facts, endpoint,
Revision Identity, Management Protocol revision, and fresh daemon-instance ID.
`daemon stop` sends graceful lifecycle control only to a process that passes
those checks. `deactivate` clears the active selector and removes the
manager-owned `forge` path; it does not stop a running managed daemon.

`forgeup logs` prints the stdout and stderr paths for the active Revision's
managed daemon. `forgeup daemon status --endpoint HOST:PORT` is intentionally
the manual-daemon path: it only inspects that explicit endpoint and prints a
`manual` result. It neither registers, activates, starts, stops, nor otherwise
alters a manually launched daemon.

## State and removal

Each canonical selector has isolated Revision State: a database, runtime
information, and diagnostic logs. That state is separate from the immutable
installation and is retained by default:

```sh
forgeup remove dev              # removes the snapshot; retains Revision State
forgeup remove dev --purge-state # removes both, when the Revision is inactive
```

An active Revision cannot be removed; deactivate it first. A replacement also
retains existing state by default. Add `--purge-state` only with `--replace`
to discard it. If an installation was removed while keeping state, re-register
that same canonical selector with `--reuse-state <selector>` to require the
retained state to be present before reattaching it.

Forgeup rotates its own diagnostic log at 64 KiB and retains the current file
plus two numbered older files. Managed daemon stdout and stderr are retained
under that Revision's state logs directory until state is purged.

## Locations and recovery

Set `FORGEUP_HOME` to put all Forgeup-owned files under one root. It creates
these private subdirectories:

```text
$FORGEUP_HOME/config   registry.toml and registry.lock
$FORGEUP_HOME/data     immutable installations and per-Revision state
$FORGEUP_HOME/runtime  per-Revision runtime records
$FORGEUP_HOME/bin      manager-owned active `forge` path
```

Without `FORGEUP_HOME`, Forgeup uses platform conventions:

| Platform | Configuration and data | Runtime | Managed bin |
| --- | --- | --- | --- |
| Linux/other Unix | `$XDG_CONFIG_HOME/forgeup` (or `~/.config/forgeup`); `$XDG_DATA_HOME/forgeup` (or `~/.local/share/forgeup`) | `$XDG_RUNTIME_DIR/forgeup`, or `<data>/forgeup/runtime` | `<data>/forgeup/bin` |
| macOS | `~/Library/Application Support/Forgeup` | `~/Library/Caches/Forgeup/runtime` | `~/Library/Application Support/Forgeup/bin` |
| Windows | `%APPDATA%/Forgeup` configuration; `%LOCALAPPDATA%/Forgeup` data | `%LOCALAPPDATA%/Forgeup/runtime` | `%LOCALAPPDATA%/Forgeup/bin` |

Run this first when diagnosing locations, permissions, registry schema, or
active-path reconciliation:

```sh
forgeup doctor
```

Forgeup creates and verifies owner-private locations on Unix. It locks the
registry across processes, writes it atomically, backs up schema-0 registries
as `registry.toml.bak` during migration, and refuses a registry created by a
newer schema. Do not hand-edit the registry or managed-daemon runtime record.
If `daemon status` reports `ambiguous`, or start/stop reports that ownership
could not be proved, inspect the paths from `forgeup logs` and resolve the
process manually; Forgeup deliberately retains ambiguous state rather than
risk controlling another process. A later status removes a record only after
it can prove the recorded PID is absent.

## Boundary and follow-on work

Forgeup manages Revision registration, selection, isolated state, and the
Managed Daemon lifecycle. Direct/manual daemon operation remains supported:
start `forge-server` yourself and point `forge` at it with `--server-url` or
`FORGE_SERVER_URL`; Forgeup does not modify that daemon.

The selected `forge` path is an activation convenience, not managed-CLI
orchestration. Automatic routing of ordinary Forge CLI commands through
Forgeup, including any daemon-start policy, is deliberately separate follow-on
work and is not part of this release.
