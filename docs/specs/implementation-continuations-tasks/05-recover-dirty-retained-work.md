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

## Implementation evidence

- Status: complete (local source implementation only).
- Input commit: `a594b95f17cd3931871be67831e02a8eb7356d22` (T04 implementation and recorded verification), in the
  user-authorized `forge/` checkout. No managed Assignment or Forge Record created.
- Output commit: `5f5ac6ec73369441a95439fefaffbf03f7cb018b`.
- Extended the existing operator-only `dispatcher recover-retained-work` /
  `record.recoverRetainedWork` operation without changing its request shape or
  weakening T04 retirement, ownership, resource, placement, Repository, branch,
  Base Commit ancestry, scope or freshness gates. No automatic adoption/cleanup.
- Dirty input has separate durable `recovery.retained_state.sha256`, not fabricated
  clean checkpoint evidence. Read-only observation binds HEAD, index entries,
  stages/flags, file names, Unix modes and working bytes, including staged-only
  differences, deletions and untracked/ignored files. Two authorization observations
  must agree. Git and streamed file hashing remain outside writer transactions;
  no temporary index, object writes, file copying, commit, stash, reset or clean.
  Added server `sha2` dependency reuses the workspace's existing locked library;
  the standard library has no cryptographic digest implementation.
- Dirty links/submodules, unsupported path encodings, unreadable files and failed
  Git observations fail closed as unknown. Symlinks are never followed by the
  content observer; unsupported state is preserved for operator investigation.
  Operators must still exclude arbitrary external writers, as in T04.
- Replacement reobserves the exact authorized dirty state before and after provider
  preflight. Its writer transaction compares the immutable recovery and latest
  source identity while exclusively reserving the successor. Missing, conflicting,
  stale or already-consumed recovery cannot authorize another dirty attempt.
  Reservation consumes the exception even before launch/bootstrap; retries return
  the existing Assignment, never a second launch. Holds/provider/resource/ordinary
  execution gates remain. Source capability stays revoked.
- Bootstrap retains the actual staged, tracked and untracked checkout, with explicit
  recovery context. Outcome acceptance still observes clean actual output; the
  successor can commit salvage and submit its new commit normally. Recovery passes
  no Check, accepts no Continuation, resets no budget and changes no immutable base
  or historical outcome. Existing clean recovery and Continuations remain supported.
- Compatibility: schema **18** is an explicit semantic safety revision despite
  unchanged SQL tables. Older dispatchers must not ignore dirty input identity.
  Ordinary schema-17 opening rejects before writes. Explicit isolated upgrades
  support schemas 7–18, preserve JSON/history/accounting byte-exactly and invent no
  fingerprints/authorizations. Historical recovery JSON defaults `retained_state`
  to null; response addition requires matched strict clients. Request shape and
  Management Protocol are unchanged. Updated CLI help/diagnostics, storage,
  upgrade, dispatch, worktree, recovery and Assignment-context documentation.
- Verification: real-Git operator RPC fixtures cover exact staged/working/binary
  untracked preservation, rejected unsafe scope/placement/Repository/branch/links,
  reopening, unchanged request replay, stale tracked/untracked/staged observations,
  transactional missing/conflicting binding denial, public replacement bootstrap
  and actual committed output acceptance. Application/provider fixtures cover
  retained/unknown resources, authorization audit failure rollback, concurrent exact
  authorization, hold/readiness blocking, reopening and one successor launch,
  dirty Continuation rejection, clean output acceptance and consumed-authority
  denial after successor retirement. Existing tests retain base/history/budgets.
  Upgrade tests cover schema-17 read-only rejection, byte-exact isolated transfer,
  historical null decoding and injected schema-18 stamp failure rollback.
- Final required root gates passed: `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (**555 passed, 0 failed, 1 existing ignored**), and
  `git diff --check`. Initial test diagnostic/fixture/compile/lint failures were
  corrected before the final run. Shipped skills were unchanged; skill-only gates
  were not applicable. The existing ignored test was not run. Local checks do not
  establish release readiness or replace T07's full-feature review.
- Implementation and coding-standards self-review: no unresolved findings.
  Atomic source/binding revalidation was added during review to prevent reuse of
  an observation after concurrent successor reservation/retirement.
- Blockers: none for T05. Unsupported/unknown retained filesystem states deliberately
  remain blocked, not silently adopted. No live installation/activation, migration,
  profile registration/selection, Check repinning, #37 recovery/dispatch, branch
  reset, merge or force-push performed; tests used isolated fixtures only.

## Routing hints

T04 recovery interfaces and durable authorization; Managed Worktree observation/validation; Assignment provisioning/bootstrap constraints; store work/attempt state and explicit upgrade tests; recovery and worktree documentation.

## Boundaries

No automatic salvage before operator approval, no implied safety from inactivity, no semantic-progress certification, no live recovery, and no broader worktree cleanup feature. If the representation requires a material policy change, return to the human rather than weaken preservation or authorization.
