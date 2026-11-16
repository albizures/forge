# Task stacks

A Stack is a linear chain of Tasks in one Project derived from `builds-on`
Relationships. Each edge declares both prerequisite completion and immediate code
inheritance. `waits-for` requires completion without changing Git ancestry. Record
IDs are opaque: a predecessor is never inferred by subtracting one.

```text
feature/payments <- feat/forge-44 <- feat/forge-49
                     PR 44             PR 49
```

The root can target any existing branch, including a feature or release branch.
Each successor starts at its predecessor's verified completed commit and opens its
PR against the applicable verified publication branch, carrying that branch through
unchanged predecessors. If no Task has published changes, it retains the selected
starting branch. No merge is needed before the next Task runs.

## Configure before execution

The Project Check Template must end with a Check named exactly `Publication`, after
Implementation and any reviews. For Dispatched Tasks it must pin a registered Task
Agent Profile with a launch policy. Checks are immutable: configure the template
before opening Tasks. Open plans may declare relationships and root configuration
before execution is complete; preparation and Spec start reject incompatible Checks
without rewriting their immutable definitions. Every Dispatched Check, not only
Publication, must retain a registered Task profile with a launch policy. Standalone
chains use the same configuration gates as Spec children. Read-only
`forge project check-template preflight <project> --json` checks the future
Dispatched Task criteria and every pinned Task profile's registered launch policy.
It fails on blockers or an older server without preflight support, without changing
configuration. Successful output includes the observed template version, not a
reservation or hosting/provider readiness guarantee. See
[publication profile behavior](profiles/publication.md).

Select the Workspace by canonical cwd or `--workspace`. Substitute inspected IDs:

```sh
forge task stack root 44 --repository acme/widgets --remote origin --base feature/payments
forge record relate 49 --kind builds-on --to 44
forge task stack show 49 --json
```

No second Stack declaration is needed. Both endpoints must be Tasks in the same
Project. A Task has at most one immediate Builds-On predecessor and one successor.
Cycles (including mixed prerequisite kinds), forks and multiple code inputs are
rejected. The predecessor may already be running or Done. Successors inherit the
root's repository and remote rather than choosing competing starting inputs.

`stack after`, ordinary dependencies, and historical Stack links remain transitional
compatibility paths during implementation; new plans should use canonical Relationships.
Historical intent is not automatically converted.

Changes to a destination or predecessor are permitted only while every affected
Task is Open and unprepared. Preparation pins the Base Commit even if subsequent
worktree creation fails; recovery retains that evidence. Replace relationship intent
explicitly with `record unrelate <task> --kind builds-on --to <predecessor>` followed
by a new declaration. Removal is subject to the same affected-suffix restrictions.
The same ordered pair cannot carry both prerequisite kinds. Independent direct waits
may remain alongside the immediate code predecessor, including waits on ancestors.
An incomplete chain without root configuration can be planned, but Stack inspection
and execution report the missing root configuration until it is supplied.

## Sequential Spec planning

The source Spec planning skill creates one cumulative chain by default, including
review and verification Tasks that consume code without editing it. Use returned
IDs, not arithmetic. Configure every member while the Spec is Open, read back
Checks, parentage, prerequisite Relationships and derived Stack, then `forge record start <spec-id>`.
Task-creation authorization includes that validated start, not dispatch. Partial
publication or failed validation is reported as an incomplete Open plan, not
silently repaired. Completion-only edges use `waits-for`; missing configuration or
prose is not an exception. Started Spec structural
edits are initially restricted; see [Records](records.md#relationships-and-lifecycle).

A Stack carries repository commits only, not external files, untracked artifacts,
services, credentials or state. Those require explicit handoff contracts.

## Preparation

Dispatch prepares stacked Tasks automatically. For Manual execution, prepare first:

```sh
forge task stack prepare 44 --json
```

Forge verifies that the selected remote fetches and pushes the selected GitHub
repository, fetches the Base Branch into an isolated observation ref, and pins its
commit. It does not switch or alter the Project's main checkout. Stacked worktrees
must use `feat/forge-<record-id>` and the pinned Base Commit. A successor additionally
requires its immediate predecessor Done with current verified result evidence.
Through unchanged predecessors, Forge follows the retained publication base: the
nearest published Task's PR must remain open and non-draft and its branch must
still identify the exact inherited commit. An all-unchanged prefix instead retains
the selected starting branch at that commit. Missing, contradictory, stale or moved
evidence blocks; there is no fallback to Project Root. Repeated preparation checks
the retained pin and reports drift without changing it. Manual start, authenticated
Assignment start (including idempotent retries), and dispatched Check entry observe
required predecessor hosting evidence again; a successful earlier preparation is
not continuing authorization. Writer transactions separately recheck persisted
lineage, immutable Checks, pins and current result generations.

Workflow `ready` excludes Tasks with disabled Spec execution, missing root
configuration, incompatible Checks or stale retained predecessor results. With
`--blocked`, these diagnostics appear in `execution_blockers`, separately from
unfinished prerequisite IDs in `blocked_by`. This is an additive readiness response
field; older responses default to no configuration diagnostics. Readiness is not a
hosting observation or provider-dispatchability guarantee: execution still performs
the authoritative external checks.

## Publication and completion

The publication agent uses `git`/`gh` to push without force, reuse or create an open
non-draft PR, and mark a matching draft ready after verification. Creation is still
an external operation; retry must inspect for a matching PR before creating another.
Forge independently verifies the published result:

```sh
forge assignment stack --json
forge assignment stack --url https://github.com/acme/widgets/pull/123 --json
```

Only the bound final Publication Check Assignment may submit publication evidence.
Operator-driven Manual work uses:

```sh
forge task stack publish 44 --url https://github.com/acme/widgets/pull/123 --json
```

Verification persists the canonical URL, repository, head branch, full head commit,
base branch and current evidence generation. Prior Checks must already pass.
A final passing outcome without matching verified publication is rejected. A
failure or uncertain hosting response leaves the Task In Progress; repeating
verification is safe. Changed evidence requires fresh publication verification.

### Unchanged repository output

A Task whose final commit exactly equals its pinned Base Commit can verify an
unchanged result instead of publishing a PR:

```sh
forge assignment stack --unchanged --json
# Explicit Manual Execution Mode:
forge task stack publish-unchanged 44 --json
```

Forge observes the active Managed Worktree for assigned work, or the registered
Project's main checkout for Manual work. The checkout must be clean (including
untracked files), its full HEAD must equal both the prepared commit and current
repository output evidence, and all preceding Checks must pass. Equal trees at
different commits do not qualify. No worker-provided commit or clean flag is
accepted by these commands. Verification persists `unchanged_result` separately
from `publication`; it never invents a PR URL or passes/deletes the final Check.
Submit the ordinary bound passing Publication outcome after verification.

Completion observes the checkout again and revalidates evidence and generation.
Dirty state, drift, or invalidation blocks completion. Exact verification retries
do not duplicate audit history; a new generation requires fresh passing Checks and
verification. Successors inherit the unchanged Task's exact verified final commit
and carry its Base Branch forward, without requiring that Task to have a branch or
PR. For example, changed A → unchanged B → changed C gives C predecessor B and
A's verified final commit as input, while C's PR targets A's Task Branch. Consecutive
unchanged Tasks use the same rule; review-only work receives no special exemption.

Publication does not directly mark Done. The existing Checks, blockers, ownership,
retirement and worktree safety gates remain. Forge observes hosting state again
before completing and rechecks evidence transactionally. It does not wait for merge.

## Limits and recovery

Initial hosting support is GitHub (`github.com`) through `git` and `gh`, with
same-repository PR heads. Unsupported hosts, missing credentials, unavailable
branches and ambiguous remotes block explicitly. Existing non-stacked Tasks keep
their previous behavior.

Keep predecessor PRs open and their branches retained through downstream publication.
Unexpected predecessor branch movement, merged/closed PRs and deleted branches
block preparation or publication; Forge does not rebase, force-push, merge, retarget,
or continuously monitor already-completed stacks. Operators investigate and restore
the required verified state before retrying; uncertain state is not accepted as success.

Storage revision 8 adds stack persistence; revision 10 adds unchanged-result
persistence. Ordinary opening rejects incompatible marked stores without conversion
or marker rewrites. The explicit [Revision State upgrade](state-upgrades.md) workflow
can copy schemas 7, 8, or 9 into a new revision and migrate to 10, preserving
existing Tasks, PR evidence and Check pins without inventing unchanged results. Do not
point this revision at a live older store expecting automatic conversion. No CLI invocation in this document installs or activates a revision.
