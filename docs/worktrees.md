# Managed Worktrees

Forge creates and durably tracks one Managed Worktree for an eligible Open Task
scoped to exactly one Project. Its only placement is:

```text
<workspace-root>/.forge/worktrees/<project-slug>/<record-id>
```

The base belongs to the Forge Workspace, not each nested Project. A Project may
be the Workspace root itself. Identical Project slugs in separate Workspaces
have separate checkout paths and claims. Worktree roots are not configurable;
external `--path`, `--root`, and copy-manifest placement overrides are unavailable.

## Select and create

Select a Workspace explicitly with `--workspace`, or run inside its canonical
root. Outside Workspaces, selection is required; there is no saved default.
Worktree IDs must belong to the selected Workspace. Lists and doctor operations
inspect only that Workspace, including when cwd is within a nested Project.

```sh
forge worktree create <task-id> --workspace personal --branch task-branch --base main
forge worktree info <task-id> --workspace personal
forge worktree list --all --workspace personal
```

All worktree commands accept `--json`. Explicit creation chooses branch and base;
automatic dispatch uses the Task's persisted Base Commit and deterministic Task
Branch `feat/forge-<record-id>`. Explicit [Task stacks](stacks.md) resolve their
Base Commit at preparation from the selected root branch or completed predecessor;
stacked worktrees require that pinned commit and deterministic branch even for
explicit creation. Repeating creation returns the active claim,
not a second checkout.

Forge validates canonical existing ancestors before creating missing directories
or invoking Git. Symlinks redirecting `.forge`, `worktrees`, the Project folder,
or the checkout itself are rejected, including dangling links. Inspection,
recovery and forced cleanup also reject redirected or out-of-layout paths;
restore the fixed placement before retrying. Unrelated manual Git operations
are not policed, but linked worktrees cannot be registered as Projects.

## Inspect before removal or repair

```sh
forge worktree list --all
forge worktree doctor
forge worktree remove <task-id>
```

`list --all` includes all claims; the default list and `doctor` show claims
requiring cleanup. Only `doctor --repair` performs repair. Inspect `facts`,
`blockers`, and `removable` before acting.

Ordinary removal is local and offline. It refuses dirty checkouts, missing or
invalid Git metadata, unresolvable upstreams, and commits ahead of upstream.
Behind status alone is informational. Use `remove --force` only after accepting
the reported risk. Force and doctor repair do not bypass placement validation
or the guard against open Assignments and unresolved provider resources.

Claims move from `reserving` to `active`. Failed preparation or removal leaves
`cleanup-required` and retains evidence; investigate rather than creating a
second checkout. Task completion retains the claim and checkout. Removing a
checkout does not delete its Task branch.

## Setup hooks and recovery

Project Setup Hooks run through the platform shell in the new checkout:

```sh
forge project setup-hook set --workspace personal --project alpha 'cargo fetch'
forge project setup-hook clear --workspace personal --project alpha
```

Hooks receive `FORGE_PROJECT_SLUG` (the qualified Project reference),
`FORGE_RECORD_ID`, `FORGE_WORKTREE_PATH`, `FORGE_BRANCH`, and `FORGE_BASE`.
A nonzero exit retains the checkout and cleanup-required claim for investigation
and prevents automatic preparation retry; there is no implicit rollback.

```sh
forge worktree recover <task-id> --confirm-no-active-agent
```

Recovery adopts only an unclaimed checkout at the fixed path with the expected
Repository, deterministic Task Branch, and persisted Base Commit ancestry.
Confirmation is an operator assertion, not agent-exit evidence. Recovery requires
no open Assignment and retains a Dispatch Hold until explicitly resumed.
See [dispatch operations](dispatch.md) and [Assignment recovery](assignment-recovery.md)
for dispatch and provider-resource procedures.
