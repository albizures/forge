# Publication profile behavior

Use this behavior in a Task Agent Profile pinned by the final Check named
`Publication`. Select a runtime/model and resources appropriate to the Workspace;
preview and register the profile through `forge agent-profile`, then configure the
Project Check Template before opening Tasks. Registration is immutable and does
not update existing Task pins.

## Bootstrap and destination

Run `forge assignment bootstrap --json` before working. Stop if bootstrap fails.
Work only in `bound.executionDirectory`, follow trusted constraints, and keep
capability secrets out of arguments, reports and logs.

Run `forge assignment stack --json`. For a stacked Task, use its
`destination.repository`, `destination.remote`, `base_branch`, and deterministic
`feat/forge-<bound.recordId>` head branch. These are authoritative: a repository's
default branch is not a substitute. Read the bound Check, Task, related context,
Findings, comments and history; follow pagination. Inspect Git status, branch,
HEAD and remotes. Verify clean HEAD matches the bound Check attempt's repository
commit and that every earlier Check passed at this evidence.

A non-stacked Task uses its explicit project instructions and Task context for
destination selection; block on ambiguity. Do not opt it into a Stack here.

## Publish or reuse

Push the verified Task branch without force using the resolved remote. Inspect
existing open PRs in the selected repository with the exact head owner/branch and
base. Reuse a single exact match; multiple matches are a blocker. Check for a
matching PR on every retry, including after a push or creation response is lost.

Create a non-draft PR when no match exists. Use the pr skill for its body if
selected by the profile. Preserve an existing PR's title and body. If the exact
matching PR is a draft, verify its head/base and mark it ready with `gh pr ready`.
Verify the published full head commit and non-draft status. Leave merge, branch
rewriting, retargeting, lifecycle and dispatch administration to the operator/Forge.

For a stacked Task, independently verify publication through:

```sh
forge assignment stack --url https://github.com/acme/widgets/pull/123 --json
```

Substitute the actual PR URL. A failed verification is a blocker, not a passing
Check. Report any push/PR effects without claiming publication acceptance.

## Submit the outcome

Create an outcome JSON file outside the checkout. Copy the bound attempt ID,
numeric generation and evidence exactly. Use `pass` / `complete` only after
publication (and stack verification when applicable) succeeds. Include the verified
PR URL and full commit in the outcome message. The actor is supplied by the CLI.
Use empty `findings` and `verifications` unless this Check has substantive review
Findings to raise or verify through the established outcome protocol.

Missing prerequisites, inconsistent evidence, unavailable hosting or a human
decision require `fail` / `human-input`. Interrupted execution without a substantive
result uses `fail` / `interrupted`. Include already-published PR URLs and observed
external effects. Confirm the checkout is still clean at the bound commit.

Submit with `forge assignment outcome --file <absolute-outcome-file> --json`.
On rejection, report the diagnostic; never claim Check success. After acceptance,
stop repository/hosting mutations. Forge owns retirement, continuation and Done;
publication agents do not start successors or complete Tasks manually.
