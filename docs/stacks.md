# Task stacks

A Stack is an explicit linear chain of Tasks in one Project. Ordinary `depends-on`
edges continue to control readiness without changing Git ancestry. Record IDs are
opaque: a predecessor is never inferred by subtracting one.

```text
feature/payments <- feat/forge-44 <- feat/forge-49
                     PR 44             PR 49
```

The root can target any existing branch, including a feature or release branch.
Each successor starts at its predecessor's verified completed commit and opens its
PR against that predecessor's branch. No merge is needed before the next Task runs.

## Configure before execution

The Project Check Template must end with a Check named exactly `Publication`, after
Implementation and any reviews. For Dispatched Tasks it must pin a registered Task
Agent Profile with a launch policy. Checks are immutable: configure the template
before opening Tasks; incompatible existing Tasks cannot join a Stack. See
[publication profile behavior](profiles/publication.md).

Select the Workspace by canonical cwd or `--workspace`. Substitute inspected IDs:

```sh
forge task stack root 44 --repository acme/widgets --remote origin --base feature/payments
forge task stack after 49 --predecessor 44
forge task stack show 49 --json
```

`after` also creates the dependency atomically, unless it already exists. Both
Tasks must have compatible Check definitions and belong to the same Project.
A Task has at most one Stack Predecessor and one Stack Successor. Cycles and forks
are rejected. The predecessor may already be running or Done.

Changes to a destination or predecessor are permitted only while every affected
Task is Open and unprepared. Preparation pins the Base Commit even if subsequent
worktree creation fails; recovery retains that evidence. Replacing a predecessor
preserves existing ordinary dependency edges. A current Stack Predecessor dependency
cannot be removed independently. There is no automatic conversion of existing Tasks.

## Preparation

Dispatch prepares stacked Tasks automatically. For Manual execution, prepare first:

```sh
forge task stack prepare 44 --json
```

Forge verifies that the selected remote fetches and pushes the selected GitHub
repository, fetches the Base Branch into an isolated observation ref, and pins its
commit. It does not switch or alter the Project's main checkout. Stacked worktrees
must use `feat/forge-<record-id>` and the pinned Base Commit. A successor additionally
requires its predecessor Done, its PR still open and non-draft, and its branch still
at the verified completed commit.

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

Storage revision 8 adds stack persistence. Ordinary opening rejects earlier
marked stores without conversion or marker rewrites. The explicit
[Revision State upgrade](state-upgrades.md) workflow can copy schema 7 into a new
revision and migrate it to 8, preserving existing Tasks as non-stacked. Do not
point this revision at a live older store expecting automatic conversion. No CLI invocation in this document installs or activates a revision.
