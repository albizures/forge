# T08 — Interrupted-effect safety (in progress)

## Status

**T08 remains incomplete; T09 must not proceed.** This increment establishes the
runtime read-only inspection/admission fence. It does not deliver the required
Forge-visible interrupted-effect persistence or audited operator resolution.
Production durable dispatch and automatic recovery remain disabled. No live Forge
records, profiles, credentials or worktrees were changed.

## Implemented boundary

`runner/src/interrupted-effects.ts` uses the pinned physical read-only adapter to
inspect committed execution tasks before any writable Harness opening/recovery.
The pinned upstream `pi.tool` v3 implementation commits an `execute` checkpoint
with actual arguments and replay policy before entering the tool, then commits
its terminal result separately. Those existing protected records supply effect
intent/identity; this increment does not introduce a second uncoordinated ledger.

- The complete retained Launch must match. Interrupted initialization (no root)
  and incompatible/corrupt storage fail closed, rather than recreate/repair it.
- Paginated scans include all conversations in the one-Assignment store, not just
  its root. A maximum of 1,000 pages/1,000 blocker identities bounds inspection;
  exceeding a limit denies admission, never silently truncates to safety.
- A pre-execution `call` checkpoint has no execution intent and is not an effect
  blocker. An unsettled `execute` checkpoint blocks, even if labelled replay-safe.
  No automatic replay-safe operation is approved for the current coding tools.
- Completed tool results remain accepted runtime receipts. Failed, aborted,
  faulted and orphaned receipts conservatively block; interruption/cancellation
  errors cannot turn an external unknown into known absence. Unknown tool
  versions/checkpoints fail closed.
- Returned data contains only task, conversation and call identities. Arguments,
  outputs and results stay in protected runtime storage. No runtime repair, new
  input, bootstrap, model/tool call, Git mutation or Forge lifecycle change occurs
  during inspection.

`createCheckRuntime` now requires the canonical protected runtime directory and
checks the fence after exact lease admission but **before bootstrap**, profile
construction or caller Harness opening. Failure closes this local lease consumer
and emits only the existing fixed redacted runtime failure. The trusted caller
still owns host-exclusive locking, verified process identity and durable retention;
this read-only inspector is not ownership evidence or recovery authorization.
Both existing Check component/supervised tracer callers supply their actual bound
directory. The synthetic main remains unchanged and cannot resume execution.

Compatibility: no dependency, Forge schema, runtime schema or public CLI/RPC wire
change. The internal TypeScript helper gains a required argument; all callers are
updated. The component hash includes the new source and changed runtime helper,
so old component identities cannot silently reuse the changed implementation.

## Verification

Commands from repository root:

```sh
(cd runner && npm run check && node --test test/interrupted-effects.test.mjs test/bootstrap.test.mjs)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

All final gates pass: **647 Rust harness tests passed, 0 failed, 1 pre-existing
opt-in probe ignored; all 6 parallel supervised-tracer scenarios passed; runner
typecheck and 48 Node tests passed**. `git diff --check` passes. Verification covers
the combined checkout, preserving the user's existing planning changes, not an
isolated commit. No clean-install, packaged-component or real-provider release
verification was run. The historical supervisor-test timeout did not recur here;
this is not a diagnosis or general flake-resolution claim.

### Actual process/effect evidence

Two isolated process scenarios in `runner/test/interrupted-effects.test.mjs` use a
real pinned Harness, the actual configured unsafe Bash tool and an independent
loopback HTTP publication-like mutation endpoint. Only the model and lease are
fixtures; these are **not** supervised Forge dispatch or real-provider evidence.
The worker uses a separate process group solely for test teardown, not product
retirement or proof against escaped same-user descendants.

1. The endpoint counts/accepts a POST effect and deliberately withholds its reply.
   The test synchronizes on that acceptance, then SIGKILLs the worker group before
   a tool result can commit. Inspection finds one unsettled effect on two separate
   reads, and the real Check construction boundary rejects before bootstrap and
   closes authorization on each attempted reopen. Mutation count remains one.
   The test never opens a second Harness or sends a retry to the endpoint.
2. When the endpoint returns normally, the tool result commits and inspection finds
   no interrupted effect. This establishes the receipt distinction, not permission
   to recover an Assignment without other policy gates.

Both scenarios compare tracked bytes, staged index contents, untracked bytes and
Git status before/after death and inspection, and verify unchanged runtime DB
bytes during inspection. Read-only SQLite coordination sidecars may change; this
is not a byte-immutability claim about SHM reader locks or a power-loss test.
Foreign Launch identity rejects. Worker output excludes the configured model-key
canary. No repository repair/adoption or hold clearance is used.

A separate committed-storage fixture covers more than one page, a nested call in
another conversation, an alleged replay-safe intent (still blocked), all four
non-completed terminal receipt classes, completed receipts, missing root, unknown
tool version and unknown checkpoint. These are storage-contract fixtures, not
additional process/effect evidence.

## Remaining T08 work / handoff

1. Add an application-owned trusted interrupted-effect observation contract and
   transactional Forge retention of exact runtime/effect identities, a visible
   recovery blocker/hold and retained capacity. Deliver actual runtime observations
   to that boundary, including daemon restart and lost transport/commit cases.
   Preserve existing accepted Outcomes and unrelated resources/holds.
2. Define/implement a bounded operator CLI/RPC inspection/resolution operation:
   scope authorization before replay; exact retries return original audit;
   conflicting reuse rejects; verified ownership/absence and current binding
   preconditions cannot race the resolution commit. Unsupported external outcomes
   remain unknown. Resolution records evidence, never reruns the command or
   authorizes dirty-commit adoption, and cannot clear unrelated holds.
3. Compose the fence and retained Forge blocker with that resolution policy before
   T10 same-Assignment recovery is possible. Prevent a fresh model/tool request
   from bypassing any unresolved effect, including within a live runtime after
   failed/cancelled effects, not just reopening through this helper.
4. Extend synchronized tests to the post-tool-return/pre-result-commit boundary,
   push/publication analogues and daemon restart. Verify denied wrong-scope,
   conflicting and unsafe-ownership resolutions, preservation of accepted results,
   audit and capacity, then renew all affected verification/reviews.

There is currently **no operator resolution path**. Do not manually mutate runtime
SQLite or use existing unknown-provisioning resolution/retained-work salvage as an
interrupted-effect waiver. This partial increment does not establish exactly-once
execution, external reconciliation or universal process death.

## Task-local reviews

Implementation review against T08: “Translate runtime interruption into
Forge-visible uncertainty” and “Establish a bounded, explicit operator
inspection/resolution path” remain missing; the runtime gate alone does not meet
these acceptance checks. “On reopen, neither tool replay nor a fresh model request
bypasses the blocker” is implemented at Check construction but not yet a full
recovery controller or live-runtime failure policy. Resolution rejection/audit and
daemon-restart composition remain unverified. No scope creep or completion claim.

Standards self-review: no introduced violations found. The new capability is a
read-only runner adapter over pinned runtime machinery, not duplicated Forge
lifecycle/recovery policy; no external calls occur inside Forge transactions.
Incompatible records and unknown external effects fail closed, no destructive
worktree operations occur, and process tests isolate roots/listeners/credentials
with explicit effect synchronization. T18 independent review remains required.

Summary: runtime interruption inspection/fencing delivered; Forge-visible durable
retention and operator resolution are the blocking missing T08 deliverables.
