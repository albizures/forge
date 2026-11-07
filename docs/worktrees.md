# Manual Managed Worktrees

This guide covers explicitly requested, non-dispatch worktrees. For
dispatcher-created Task Managed Worktrees, follow [dispatch operations](dispatch.md)
and [assignment recovery](assignment-recovery.md): they are not automatically
rolled back or repaired.

A manual Managed Worktree is Forge's durable checkout for an In Progress Task
scoped to exactly one Project. Forge places it at
`<worktree-root>/<project-slug>/<task-id>` and retains one Worktree Claim per Task.

## Configure and create

Set the user-wide root before creating the first checkout:

```sh
forge config set worktreeRoot "$HOME/.forge/worktrees"
forge config get worktreeRoot
```

Create a checkout with an explicit branch and base:

```sh
forge worktree create <task-id> --branch task-branch --base main
forge worktree info <task-id>
forge worktree list
```

All worktree commands accept `--json`. Unsupported legacy state is not adopted or
migrated automatically; the old `--path` and `--copy-manifest` arguments are not
supported.

## Inspect before removal or repair

```sh
forge worktree list --all
forge worktree doctor
forge worktree remove <task-id>
```

`list --all` includes claims needing attention. `doctor` diagnoses them; only
`doctor --repair` performs repair. Inspect the ordered `blockers` and `removable`
fields in `info`, `list`, or `doctor` before choosing an action.

Ordinary removal is local and offline. It refuses dirty checkouts, missing or
invalid Git metadata, unresolvable upstreams, and local commits ahead of upstream.
Behind or diverged status is informational. Use `remove --force` only after
deliberately accepting the reported risk.

A claim moves from `reserving` to `active`. Incomplete creation rollback or
removal leaves `cleanup-required`; do not bypass the claim or create a second
checkout. Investigate with `doctor`, then explicitly repair or force-remove only
when appropriate.

## Optional setup hook

A Project Setup Hook runs in the new checkout through the platform shell:

```sh
forge project setup-hook set --project alpha 'cargo fetch'
forge project setup-hook clear --project alpha
```

It receives `FORGE_PROJECT_SLUG`, `FORGE_RECORD_ID`, `FORGE_WORKTREE_PATH`,
`FORGE_BRANCH`, and `FORGE_BASE`. For **manually created** checkouts, a nonzero
exit rolls back the checkout and claim; hook output is not persisted. Incomplete
rollback requires explicit cleanup. Dispatcher-created worktrees instead retain
failed-hook evidence for operator investigation; do not apply manual rollback
assumptions to dispatch recovery.
