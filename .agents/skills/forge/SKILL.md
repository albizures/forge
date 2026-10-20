---
name: forge
description: "Operate Forge workflow records: specs, tasks, wayfinders, grilling, initiatives, dependencies, readiness, lifecycle, and comments through the global Forge CLI."
---

# Forge Workflow

Use this skill when creating or operating Forge records. This is the Forge reference for workflow skills: command shape, scoping, relationships, readiness, lifecycle, and outcome recording.

## Project scope

Forge is global. Use the global CLI directly:

```sh
forge <command>
```

Before creating project-scoped records, run `forge here` from the repository. If Forge cannot infer the project, stop and ask the human to register or select the project; do not silently create global work. Use explicit scope flags (`--project`, `--initiative`, `--all-records`) when inference would be ambiguous.

## Public commands

Use `forge --help` as the live command surface. Common commands for workflow skills:

- `forge config get [storePath]`
- `forge config set storePath <absolute-path>`
- `forge store path`
- `forge store doctor`
- `forge project add <id> --root <path> [--name <name>] [--remote <url>]`
- `forge project root add <id> <path>` / `forge project root remove <id> <path>`
- `forge project remove <id>`
- `forge projects`
- `forge here`
- `forge new initiative --title <title> (--body <md>|--body-file <file>|--body -) --projects <ids>`
- `forge new spec --title <title> (--body <md>|--body-file <file>|--body -) [--project <id>] [--initiative <id>] [--generated-by <id>]`
- `forge new wayfinder --title <title> (--body <md>|--body-file <file>|--body -) [--project <id>|--projects <ids>|--initiative <id>|--scope global]`
- `forge new task --title <title> (--description <md>|--description-file <file>|--description -) --parent <id> [--kind research|prototype|review] [--depends-on <id>]...`
- `forge new grilling --title <title> (--description <md>|--description-file <file>|--description -) --parent <id> [--depends-on <id>]...`
- `forge show <record>`
- `forge list [--state <state>] [--kind <kind>] [--project <id>|--initiative <id>|--all-records]`
- `forge history <record>` / `forge comments <record>` / `forge updates <record>`
- `forge initiatives [--project <id>|--all-records]`
- `forge ready [--blocked] [--include-hitl] [--planning] [--project <id>|--initiative <id>|--all-records]`
- `forge next [--include-hitl] [--planning] [--project <id>|--initiative <id>|--all-records]`
- `forge start <record>`
- `forge comment <record> --message <md>|--message-file <file>|--message -`
- `forge done <record> --resolution <slug>`
- `forge deps add <record> --depends-on <id>` / `forge deps remove <record> --depends-on <id>` / `forge deps <record>`
- `forge worktree create <task> --branch <branch> --base <ref> [--path <path>] [--copy-manifest <path>] [--json]`
- `forge worktree info <task> [--json]`
- `forge worktree remove <task> [--json]`
- `forge worktree list [--all] [--json]`
- `forge worktree doctor [--prune-missing] [--json]`
- `forge tree <record>`
- `forge open <record>`
- `forge edit <record>`

Forge uses generated Effect CLI help. If help shows shared parser flags on a command or repeats parent names for nested subcommands, still follow the narrower supported combinations documented above; handler validation is authoritative for unsupported record flag combinations. If a needed operation is not exposed by `forge --help`, stop and ask for the Forge command surface to be upgraded.

## Record kinds

- **Spec**: project-scoped implementation outcome. Create one with `forge new spec`. Use child Tasks and Grilling for delivery work and decisions.
- **Task**: executable child work under a Spec or Wayfinder. Ordinary implementation, verification, merge, docs, and handoff work use `forge new task` with no kind. Use `--kind research`, `--kind prototype`, or `--kind review` only when that semantic kind fits.
- **Wayfinder**: a map for discovering a route through fog. Create with project, project-set, initiative, or global scope. Use children for sharp frontier work and comments for outcomes/map notes.
- **Grilling**: human-in-the-loop decision child. Include it in readiness with `--include-hitl` only when intentionally working HITL items.
- **Initiative**: cross-project delivery envelope, not a parent. Attach Specs or Wayfinders to coordinate multi-project work.

## Creation prose

Prefer Markdown flags and stdin/files for long prose: `--body -`, `--body-file`, `--description -`, or `--description-file`. Do not create temporary JSON payloads for ordinary Forge creation; Forge creation is flag-based.

Forge task creation has no `--profile`. Put routing hints in the description only when useful.

## Relationships and readiness

- Create children with `--parent <spec-or-wayfinder-id>`.
- Preserve blockers with repeatable `--depends-on <id>` on creation, or `forge deps add` afterward.
- Inspect relationships with `forge tree <record>` and `forge deps <record>`.
- Use `forge ready` as the executable frontier. Add `--planning` to include ready Specs or Wayfinders that need planning action. Add `--include-hitl` to include ready Grilling.
- Use `forge ready --blocked` for dependency and parent/child diagnostics.
- Use explicit scope flags when a command must not rely on inferred project scope.

## Lifecycle and outcomes

All record kinds share the same lifecycle: ready, in-progress, done.

1. Start executable work with `forge start <record>`.
2. Record non-trivial outcomes with `forge comment <record> --message-file <file>` or `--message -`.
3. Complete with `forge done <record> --resolution <lowercase-kebab-case>`.

Forge refuses to complete parent records while open child work remains. Do not mark Specs or Wayfinders done until their children are done or deliberately removed/out-scoped.

## Wayfinder map maintenance

Use comments for normal child outcomes, decisions, and map-change notes. Use `forge edit <record>` only when the canonical Wayfinder body itself must change.

## Worktree boundary

Forge owns local managed worktree primitives: create, info, list, doctor, remove, and safety checks for missing, invalid, or dirty managed worktree state. Use `forge worktree` for those local lifecycle operations only.

The Spec workflow owns git and GitHub orchestration around those worktrees. Pushes, PR creation, PR comment retrieval, task branch stack orchestration, round gates, accepted-work integration, and final integration PR creation use `git` or `gh` directly according to the Spec workflow, not Forge commands.

## Boundaries

- Forge is authoritative for workflow records, relationships, readiness, lifecycle state, comments, history, and managed local worktree state.
- Do not edit Forge store files by hand; use Forge commands.
- Do not use GitHub issues as the backing store for Forge workflow records.
