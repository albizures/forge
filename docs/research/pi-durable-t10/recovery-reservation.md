# T10 — Transactional recovery reservation increment

Historical reservation snapshot. The subsequent [cancellation increment](recovery-cancellation.md)
adds verified revoked-retirement cancellation only; recovery-authorizing consumption
remains unimplemented. Statements below about no release API describe this earlier
snapshot, not the current cancellation path.

## Scope

This increment adds an internal trusted-daemon `claim_recovery` persistence port.
It reserves recovery; it does **not** authorize execution, issue a recovery lease,
prove old descendants safe, release ownership, reopen a Harness or enable daemon
recovery. T10 remains pending and T11 must not advance.

`Claim::for_candidate` content-binds the complete original candidate (including
attempt context, generation, admission, worktree binding and predecessor grant).
Its retained representation contains only the exact Launch, bounded operation and
daemon identities and SHA-256 of the typed candidate. Task bodies, handoffs,
repository context and provider/capability credentials are not copied into ordinary
resource diagnostics. SHA-256 is a candidate commitment, not process safety proof.

An immediate transaction authorizes Workspace and the original immutable binding
before replay, then either returns the identical original reservation or rejects
conflicting operation/epoch/content reuse. First acceptance re-runs all candidate
preconditions in the writer transaction and compares the candidate commitment.
Changed grants, stale attempts, dependencies, worktree bindings, holds, uncertain
effects/resources, retirement and competing Assignments reject without writes.

The existing opaque `agent_assignment_resources` ledger retains one
`recovery-claim` resource, its original timestamp and capacity atomically. All
claimants use the immediate writer transaction: independent database connections
cannot reserve competing recovery operations. Exact retries across restart/lost
reply return the original fence; they do not repeat admission or adopt a grant.
Retired claim history also prevents implicit operation reuse. No timeout or changed
daemon epoch transfers a claim. Ordinary grant issuance rejects the resource,
including renewal by the previous owner. Already delivered leases are **not**
revoked retroactively: host/process safety remains an independent unfinished gate.

Generic resource creation and retirement cannot forge or consume this fence.
There is deliberately no release API yet: a future specialized recovery/retirement
protocol must reconcile it with immutable history. The port is not exposed by
CLI/RPC or called by the production daemon, so no live Assignment can acquire an
unreleasable reservation through ordinary operations. Do not manually insert,
retire or delete the marker, or use generic unknown-resource resolution as a waiver.

## Compatibility

No DDL, schema version, runner wire protocol, dependency or public CLI/RPC change.
This is an additive internal Rust API and opaque resource kind within schema 22.
Older grant/readiness code already blocks unknown retained resources; it cannot
interpret this marker as execution authority. Existing candidate preflight remains
read-only; the shared implementation now also runs inside claim transactions.

## Verification

Six new persistence-policy tests cover:

- exact original-result replay after reopen/revocation and foreign/conflicting denial;
- no ordinary lease renewal, generic insertion or generic retirement bypass;
- unchanged attempt counters, histories, grants, evidence, holds and admissions;
- candidate drift between inspection/claim, including bound context and renewal;
- independent-connection competing claims and concurrent exact retries;
- insert failure rollback followed by successful retry.

The test fixture uses SQLite bindings, not host/model execution. There is no new
successful recovery demo, fake-clock recovery controller, packaged-component or
real-provider evidence in this increment. Reproducible commands:

```sh
cargo test -p forge-store durable_execution::tests::recovery_tests
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Final command results and reviews are recorded in the parent T10 evidence file.
Remaining work: specialized claim reconciliation/consumption, host cleanup proof,
recovery leases, daemon composition, authorized no-re-admission writable reopen,
restart matrices, fresh successor demonstrations and domain/operational updates.
