# T05 — Salvage dirty and untracked interrupted work safely

## Authority and input

Parent: [local Spec](../implementation-continuations.md). Read the [shared execution contract](README.md#shared-execution-contract). Project: `factory/forge`. **Depends on T04; Builds-On T04.** Expand T04's supported clean-commit recovery without changing its safe ownership rules.

Spec AC: remaining dirty/untracked scope of **16–18**, recovery aspects of **20–21**.

## Outcome

A replacement Implementation agent can inspect and salvage committed, tracked-dirty and untracked interrupted work after explicit safe recovery authorization. Forge neither deletes that evidence nor silently represents a dirty worktree as a clean accepted checkpoint.

## Scope

- Extend the same operator-authorized workflow to dirty tracked files, staged changes and untracked work. Determine and document a recovery representation that preserves retained state and distinguishes it from ordinary clean checkpoint evidence.
- Protect canonical placement, Task/Repository identity, old-agent absence and retained-resource safety. A supported recovery authorization must be bound to observed retained state sufficiently to reject stale/conflicting adoption and duplicate launch after restart.
- Transfer inspection/salvage authority, not a claim of correctness. The replacement starts with the retained checkout and accurate recovery context; ordinary Continuation/success submission still requires actual clean verified output.
- Do not auto-commit, stash away, clean, reset or discard interrupted evidence merely to fit the normal dispatch input contract. Do not copy files outside the authorized containment boundary or expose capability secrets.
- Handle a crash before outcome submission and before a commit as first-class cases. Unknown state or failed observation remains unknown with actionable diagnostics and no automatic deletion/adoption.
- Preserve T04's clean recovery and ordinary checkpoint flows. Apply compatibility/schema/upgrade rules to new durable recovery facts and update public diagnostics/help.

## Observable acceptance and verification

1. Isolated interrupted-agent fixtures retain exact tracked contents, staged changes and untracked file contents before and after authorization, restart and replacement bootstrap; replacement can inspect and commit the salvaged output.
2. The first successor outcome reflects its actual resulting clean evidence, not a fabricated checkpoint representing dirty input. Review/publication still assess normal committed output.
3. Unsafe ownership, stale recovery facts, symlink/placement violations, unknown provider cleanup and conflicting requests preserve evidence and prevent unauthorized replacement.
4. Crash/restart at authorization and launch boundaries does not duplicate actors, lose retained state, or automatically mark work accepted.
5. Recovery never clears budget exhaustion, changes immutable Base Commit, passes a Check, or modifies original accepted history.
6. T04 clean recovery and T02 Continuations remain green; dirty output cannot slip through ordinary Continuation acceptance.

Test observable recovery and public capability behavior using isolated stores/worktrees and provider fakes. Include failure injection and reopening, then run shared root gates and report actual results.

## Routing hints

T04 recovery interfaces and durable authorization; Managed Worktree observation/validation; Assignment provisioning/bootstrap constraints; store work/attempt state and explicit upgrade tests; recovery and worktree documentation.

## Boundaries

No automatic salvage before operator approval, no implied safety from inactivity, no semantic-progress certification, no live recovery, and no broader worktree cleanup feature. If the representation requires a material policy change, return to the human rather than weaken preservation or authorization.
