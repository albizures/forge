---
name: forge
description: "Operate Forge records, project scope, relationships, lifecycle, managed worktrees, agent dispatch, and assignment recovery through the Forge CLI."
---

# Forge workflow

## Select the CLI and endpoint

Use the CLI matching the intended Forge Revision. When working on this repository, inspect help by running from the repository root:

```sh
cargo run -p forge-cli --bin forge -- --help
```

An installed `forge` may be a different Revision. Examples in [commands.md](commands.md) use `forge` as shorthand; in this checkout replace that executable with `cargo run -p forge-cli --bin forge --`. Read that reference before creating records, changing relationships, administering dispatch, or recovering assignments/worktrees.

Ordinary commands resolve the Active Forge Revision through Forgeup and verify that the invoking CLI matches it. For a source-built CLI talking to a manually launched daemon, supply the intended RPC endpoint with the command's `--server-url` or `FORGE_SERVER_URL`. Help and parser tests need no daemon. Forgeup is a separate program, not a Forge subcommand.

## Establish scope before writing

```sh
forge here
```

Confirm the intended Project before creating project-scoped work. If it cannot be determined, ask the human to select or register it. Creation commands require explicit Project or parent arguments where applicable; `here` does not supply implicit creation scope. Use a scope-free generic Record or standalone Task only when work is deliberately outside a Project.

- Spec: exactly one Project; may parent Tasks.
- Wayfinder: one or more Projects; may parent Tasks and Grillings.
- Task: leaf work, standalone or under a Spec/Wayfinder. A Wayfinder Task selects exactly one of its Projects; a Spec Task inherits its Project.
- Grilling: human-in-the-loop leaf child of a Wayfinder; inherits its Project Scope unless a non-empty subset is selected.
- Generic Record: explicit Project Scope, including deliberately empty scope.

Use the Record Body for Markdown work content. Creation supports `--body -` for stdin; titles are not separate flags. Task Kind is optional `research`, `prototype`, or `review`; ordinary implementation Tasks need no kind. Scope, typed parent membership, and Task Kind are fixed when opened.

## Operate records

1. Inspect the Record, its Comments, and Update Log before changing it.
2. Preserve dependencies with `record relate --kind depends-on`; `--to` names the prerequisite. `generated-by` records provenance, not a dependency or parent.
3. For manual work, advance Open → In Progress with `record start`. An assigned Task instead starts through its assignment bootstrap below.
4. Record substantive outcomes with `record comment --message`. Comments are immutable; revise a non-Done Record's canonical content with `record edit --body`.
5. Complete In Progress work with `record done`. Completion Resolution is optional free text, not a required slug. Complete a parent only after its children are Done.

`record list --status open` is an inventory, not a readiness guarantee: dependencies and dispatch constraints still apply. This CLI does not expose the old `ready`, `next`, Initiative, or store-management commands. If a required capability is absent, report that gap rather than inventing a command or editing persistence directly.

## Assigned agents

When launched with Forge Assignment Capability Delivery, bootstrap before beginning work:

```sh
forge agent-dispatch bootstrap
```

Bootstrap reads `FORGE_ASSIGNMENT_ID`, `FORGE_ASSIGNMENT_DAEMON_URL`, and the owner-only file named by `FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE`. It acknowledges the Assignment, confirms the bound Task has started, and only then prints instructions. If any step fails or is uncertain, stop implementation and report the diagnostic. Keep the capability secret out of prompts, comments, shell arguments, and logs.

Work in the assigned Agent Workspace and follow the returned instructions. Bootstrap's assignment endpoint is separate from ordinary CLI endpoint selection; do not assume bootstrap configured later ordinary commands. Do not bypass assignment ownership with a manual `record start` or replace an agent merely because it has not acknowledged.

## Operators

Dispatch administration, recovery assertions, setup hooks, and destructive worktree actions are operator responsibilities. Inspect persisted evidence before recovery; use the relevant sections of [commands.md](commands.md).

Forge owns managed local worktrees and dispatch state. Git pushes, PRs, and integration policy remain the surrounding workflow's responsibility. Use Forge commands for Forge state and `git`/`gh` for repository hosting operations; never edit Forge persistence by hand.

## Maintaining this skill

The command reference uses fenced `sh` blocks containing one complete command per line, with concrete illustrative arguments and shell quoting. These are parser fixtures, not a script to execute in sequence. Source-run examples may use the exact Cargo prefix above. Keep every executable example in this format so `cargo test -p forge-cli --bin forge documented_skill` checks both files against the actual Clap parser without invoking handlers. Passing checks establish syntax only, not daemon validation, authorization, or operational safety.
