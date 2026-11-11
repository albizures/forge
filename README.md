# Forge

Forge manages project work from planning through agent execution. A local daemon
keeps Projects, Specs, Tasks, managed Git worktrees, and agent assignments in
SQLite; the `forge` CLI operates on that shared state.

- **Plan work:** register a Forge Workspace and its Git Projects, then create Specs and Tasks.
- **Isolate implementation:** give Tasks their own managed worktrees and branches.
- **Dispatch agents:** run Pi through Herdr with durable assignments and explicit
  recovery controls.

The browser interface currently provides read-only daemon status.

## Quickstart

You need a supported platform (macOS, Windows, or Linux), `forgeup`, and a matching
`forge` / `forge-server` pair registered with Forgeup. To build and register them,
follow [source setup](docs/development.md#build-and-register-a-revision)
(Rust 1.98.0 or newer).

Select your registered Revision, replacing `dev` with its selector or alias:

```sh
forgeup use dev
forgeup daemon start
forge status
```

Ensure [Forgeup's managed bin directory](crates/docs/forgeup.md#locations-and-recovery)
is on `PATH`. Ordinary `forge` commands use the selected Revision and ensure its
managed daemon is running; you do not need to start a separate server.

## Example workflow

A Forge Workspace owns a fixed canonical root containing its Projects. A Project
registers a Git repository's **main checkout**, with an `origin` remote and an
initial commit. Replace `/repos` and `/repos/alpha` with existing absolute paths:

```sh
forge workspace add personal --root /repos
forge add project /repos/alpha alpha --workspace personal
cd /repos/alpha
forge here
forge spec open --body 'Deliver the migration' --project alpha
```

Use the Spec ID printed by that command to open a Task:

```sh
forge task under-spec --body 'Implement the migration' --spec <spec-id>
```

For manual work, use the returned Task ID to advance it:

```sh
forge record start <task-id>
# Do the work, then record its completion.
forge record done <task-id> --resolution 'Implemented and verified'
```

Records normally advance from `open` to `in-progress` to `done`. A Spec can be
completed after its Tasks are done. For automatic execution, configure
[the Dispatcher](docs/dispatch.md) instead of manually starting the Task.

## Agent skills

Install the skills with the [Skills CLI](https://skills.sh):

```sh
npx skills add albizures/forge

# Required extra skills
npx skills add mattpocock/skills --skill prototype research

# Optional extra skills
npx skills add mattpocock/skills --skill grill-me writing-for-agents improve-codebase-architecture
```

| Skill | Description |
| --- | --- |
| [forge](skills/forge/SKILL.md) | Manage Forge records, worktrees, agent dispatch, and assignment recovery. |
| [spec](skills/spec/SKILL.md) | Define, plan, and deliver a Spec through task rounds, agent assignments, and review. |
| [grilling](skills/grilling/SKILL.md) | Stress-test a plan or idea through structured questions and explicit decisions. |
| [domain-modeling](skills/domain-modeling/SKILL.md) | Clarify domain terminology, maintain a glossary, and record architectural decisions. |

## Documentation

| Guide | Contents |
| --- | --- |
| [Workspaces](docs/workspaces.md) | Registration, selection, filesystem boundaries, and configuration ownership |
| [Projects](docs/projects.md) | Workspace-local identities, nested main checkouts, and canonical roots |
| [Records](docs/records.md) | Scope, lifecycle, hierarchy, and provenance |
| [Managed worktrees](docs/worktrees.md) | Manual checkout creation, safety checks, and repair |
| [Dispatcher](docs/dispatch.md) | Configuration, explicit Record dispatch, launch, and ownership boundaries |
| [Assignment recovery](docs/assignment-recovery.md) | Investigation, retirement, unknown resources, and holds |
| [Forgeup operations](crates/docs/forgeup.md) | Revision registration, selection, daemon lifecycle, and recovery |
| [Development](docs/development.md) | Source setup, manual daemon endpoints, browser/RPC, and verification |
| [Database structure](crates/store/README.md) | SQLite tables, relationships, constraints, indexes, and initialization |
| [Management Protocol](crates/docs/management-protocol.md) | Forgeup's daemon attestation and lifecycle contract |

## Security

The daemon uses **unauthenticated plaintext HTTP** and binds to loopback by
default. Its operator RPC surface can mutate state; keep access restricted to
trusted clients. A non-loopback bind is suitable only for an explicitly trusted
private network, not public exposure. Assignment capabilities restrict agent
operations but do not authenticate operator clients.

Workspace selection is routing, not authentication or OS/process isolation.
Forge and Herdr Workspaces are distinct; no Herdr plugin or upstream change is required.

Authentication, TLS termination, browser mutations, CSRF handling, and CORS access
are not provided by this foundation.

## Development

Run the intended quality gate from the repository root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

These are verification commands, not a claim that the current checkout passes
every gate. See [verification limitations](docs/development.md#verification)
before using results as release sign-off.
