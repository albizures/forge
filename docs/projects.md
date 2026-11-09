# Workspace-owned Projects

A Project belongs to exactly one Forge Workspace. Its immutable local slug is unique within that Workspace: `personal/api` and `work/api` identify independent Projects, even when their repositories share an origin URL. Project setup hooks, dispatch limits, records' Project references, Assignments and Worktree Claims use the immutable Project identity rather than a daemon-wide local slug.

## Register and select

```sh
forge workspace add personal --root /repos/personal
forge add project /repos/personal/api api --workspace personal
forge here --workspace personal --project api --json
forge here --project personal/api --json
```

Workspace IDs and Project slugs use ASCII letters, digits, hyphens and underscores; Project slugs may also contain dots, except `.` and `..`. A registration may use a qualified slug (`personal/api`) instead of a separate Workspace selector. Contradictory selectors fail before registration or configuration changes.

Workspace selection prefers `--workspace` or the Workspace in a qualified Project selector, otherwise canonical cwd containment. A local `--project api` is interpreted only within that selected Workspace. There is no saved default: outside all registered Workspaces, supply a Workspace or qualified Project selector. An explicit Project selector overrides cwd inference, including cwd within another Project or Workspace.

Without a Project selector, `forge here` chooses the deepest containing canonical Project root. A Workspace directory outside all of its Projects has no inferred Project; select one explicitly. Canonical symlink aliases behave like their targets. Selection is routing, not authentication or process isolation.

## Project roots

A Project root must be an existing main Git checkout with an `origin` remote. It may equal the Workspace root, or be nested beneath it. Independently registered main checkouts may nest; a subdirectory of an existing checkout is not a separate Project root. Independent clones remain independent repositories even when their remote URLs match.

Registration rejects:

- Roots outside the selected Workspace, including symlink escapes.
- Canonical duplicate roots or an already-used local slug in that Workspace.
- Manual and Forge-created linked Git Worktrees, even outside the reserved subtree.
- Roots within `<workspace-root>/.forge/worktrees`, including aliases to that reserved subtree.

Workspace ownership, Project identity and the registered root are immutable. There is no Project reparenting operation. Removing a Workspace containing Projects fails and never deletes its files.

A development store containing pre-Workspace, unowned Projects is rejected with an instruction to select a new store. Legacy Project import and implicit adoption into a default Workspace are unsupported.

## Configuration and APIs

Use either qualified references or a selected Workspace with Project-owned commands:

```sh
forge project setup-hook set --project personal/api 'cargo fetch'
forge dispatcher config set project-limit 2 --workspace personal --project api
forge dispatcher status --project personal/api --json
forge spec open --project personal/api --body 'Deliver the API'
```

The CLI resolves Project selectors before sending a mutation. `project.here` also supports `workspace` and `project` selectors alongside its client-supplied cwd (`folder`). `project.register` takes `folder`, `slug`, optional `workspace`, and client-supplied `cwd`. RPC filesystem paths must be absolute; the daemon never resolves client-relative paths or infers selection from its own working directory.

Project responses expose `reference` (the immutable identity to carry into scope and configuration requests), `workspace_id`, local `slug`, `project_root`, and `origin_url`. A local slug alone is not a daemon-wide identity. Separate Projects with the same local slug retain independent setup hooks, concurrency limits and consumed dispatch capacity across daemon/store reopen.

See [Workspaces](workspaces.md) for the full ownership contract, [Records](records.md) for relationship confinement, [Managed Worktrees](worktrees.md) for the fixed Workspace-rooted layout, and [dispatch operations](dispatch.md) for Workspace-owned provider/profile configuration.
