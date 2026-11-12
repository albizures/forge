# Complete a Task manually

Choose Manual Execution Mode explicitly for this workflow; normal Check-driven
dispatch never falls back to Manual. Manual work needs neither a provider nor a
Managed Worktree. Evidence is caller-attested, not fetched or
independently certified by Forge.

## Prepare

1. Select the Workspace and register a Project as described in
   [Records](records.md) and [Projects](projects.md). Projectless Tasks already
   default to Manual and receive Implementation alone.
2. Before opening a Project Task, replace its whole
   [Check Template](records.md#project-check-templates), using the observed
   template version. For this example use:

   ```json
   [
     {"name":"Implementation","description":"Deliver the described work"},
     {"name":"Review 1","description":"Review correctness"},
     {"name":"Review 2","description":"Review acceptance and verify findings"}
   ]
   ```

   Profiles may be omitted for Manual work. Optional `profile_revision` values
   pin immutable Task Agent Profile revisions; changing templates or Workspace
   selections later cannot repair or rebind existing Checks. Switching back to
   Dispatched requires every pinned profile and its prerequisites to be valid,
   but still does not enable Task dispatch in this release.
3. Open the Task, inspect its Check IDs, explicitly select Manual if it defaults
   to Dispatched, then start it:

   ```sh
   forge task open --body 'Deliver the report' --project personal/api --json
   forge record execution-mode 44 manual --expected dispatched \
     --actor human --message 'Implement and verify manually' --workspace personal
   forge record start 44 --workspace personal
   forge record check list 44 --workspace personal --json
   forge record check work 44 --workspace personal --json
   ```

   Replace all example IDs with returned values. Safe mode selection cannot
   override active/uncertain ownership or retained resources: confirm retirement
   and reconciliation first. Silence and idle state are not retirement.

## Run the review cycle

Use the strict `attempt.json` and `outcome.json` shapes in
[Manual attempts and immutable evidence](records.md#manual-attempts-and-immutable-evidence).
Submit each with `record check begin` then `record check outcome`, using
`--input-file <file> --workspace personal --json`. Use unique attempt IDs,
actual Check IDs, the current generation, attribution and a nonblank explanation.
Only one attempt can be outstanding per Task.

Let A be the initial immutable evidence and B the corrected deliverable:

| Step | Bound input / outcome | Result |
| --- | --- | --- |
| Implementation | A / pass with A | Review 1 becomes eligible |
| Review 1 | A / pass with A | Review 2 becomes eligible |
| Review 2 | A / fail with A, `disposition: "handback"`, blocking Finding | Atomic failed Assessment + Finding; generation advances; every Check becomes pending |
| Implementation rerun | A / pass with B | New output; old review passes cannot count |
| Review 1 rerun | B / pass with B | Required even if its earlier review passed |
| Review 2 rerun | B / pass with B and Finding verification | Owning Check verifies the correction |
| Human | Explicit `record done` | Completes only if every gate passes |

For handback, add these fields to the bound Review 2 outcome:

```json
{"verdict":"fail","disposition":"handback",
 "findings":[{"severity":"major","blocking":true,"message":"Correct the report"}]}
```

On the Review 2 rerun, add the actual Finding ID and observed version:

```json
{"verifications":[{"finding_id":2,"expected_version":1,"message":"Correction verified in B"}]}
```

Implementation may propose a fix with `record finding respond`, but cannot verify
Review 2's Finding. A passing Review 2 outcome without its required verification
rejects atomically. For an unlinked blocker, explicitly assign a review Check with
`record finding assign-verification`; merely assigning ownership does not resolve
it. Other unresolved blockers still prevent completion. Verification and mode
changes never implicitly clear operator-owned Dispatch Holds.

`interrupted` is not substantive handback: it closes a failed review attempt with
no Assessment, Findings or verification effects, allowing that review to retry.
Failed Implementation retains its failed Assessment and cannot advance reviews.

## Freshness, retries and completion

- Repository evidence requires a full immutable 40-hex commit SHA and repository
  identity. Artifact evidence requires a nonblank URI and full 64-hex SHA-256;
  a mutable path or URL alone is insufficient. Both work without a Managed Worktree.
- `record check invalidate-work` takes `generation`, `evidence`, `actor` and
  `message`. Use it even at the same commit when verification must rerun. It
  advances freshness, closes outstanding attempts and invalidates current passes.
  Editing the Task body also invalidates work. Inspect `check work` before retrying.
- Exact accepted outcome replay returns the original result after handback,
  invalidation, Done and restart. It does not restore old freshness. Conflicting
  replay rejects; unaccepted old-generation submissions cannot publish effects.
- Passing the last Check leaves the Task In Progress. Inspect Findings and then
  request completion explicitly:

  ```sh
  forge record finding list 44 --workspace personal --json
  forge record done 44 --resolution 'Implemented and verified' --workspace personal
  ```

  Completion rechecks Manual mode, all current ordered Checks, unresolved blockers,
  dependencies, children, open attempts and conservative ownership/resource safety.
  It neither fabricates Assessments nor clears retained safety state.

## Store and release boundary

Only fresh stores and ordinary restart of the current marked schema revision are
supported. Incompatible stores fail non-destructively with `fresh-store-required`.
Do not migrate, delete, reset or rewrite schema markers to make this workflow work.
Any separate operator reset requires safe retirement and resource reconciliation.
Restart preserves immutable Checks/profile bindings, Assessment and Finding history,
freshness, accepted-outcome replay and ownership/resource safety facts.

The deterministic release scenario is
`crates/store/tests/manual_release.rs`, exercised for repository and artifact
evidence, including same-evidence invalidation, restart and explicit Done. See
[Development verification](development.md#verification) for the complete checks.
No real-provider smoke test or autonomous continuation is part of this release.
