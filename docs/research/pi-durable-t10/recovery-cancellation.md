# T10 — Cancel reserved recovery through verified retirement

Historical cancellation snapshot. The subsequent [authorization increment](recovery-authorization.md)
adds the internal first-grant transaction behind a trusted external-verification
boundary. Concrete verification, delivery and writable recovery remain missing.

## Scope and safety

This increment closes the cancellation side of a retained recovery claim. It does
not implement recovery-authorizing consumption, fresh recovery leases, daemon
recovery or writable Harness reopen. T10 remains pending; do not advance T11.

The existing revoked retirement transaction already authorizes Workspace and exact
Launch, matches the retained retirement operation/original grant owner, and requires
a validated host cleanup receipt. Its trusted caller holds exclusive host inspection
through commit; process/effect inspection remains outside database transactions.
Within that boundary, application policy now validates each retained recovery claim
and requires the exact retirement Launch. The store retires matching claims in the
same transaction as runtime/liveness resources and the reconciliation timestamp.
Malformed JSON, invalid claim fields, foreign bindings or write failures roll back
all reconciliation, preserving capacity. No missing heartbeat, lease timeout,
unknown-resource confirmation or generic resource retirement can cancel a claim.

Cancellation preserves the claim's resource identity and creation timestamp; its
retirement timestamp equals the original reconciliation timestamp. Exact claim
retries still return the original historical reservation, even after cancellation.
They never revive authority or transfer ownership to a different daemon epoch.
Effects, unknown resources, holds, worktree bytes, runtime storage, attempts,
admissions, Outcomes and evidence are not repaired or adopted by this operation.

This uses the existing schema-22 resource ledger and retirement receipt contract.
No DDL/schema revision, public CLI/RPC field, runner wire contract or dependency
changes. Older code conservatively retains this resource instead of cancelling it.
No production daemon or public command creates recovery claims yet; existing
retirement composition can consume one if the internal trusted port reserved it.

## Verification

Five new persistence-policy tests in
`crates/store/src/durable_execution/cancellation_tests.rs` cover:

- verified cancellation, preserved identity/timestamps and immutable exact retries
  across reopened connections, with continued denial of grants and takeover;
- revocation, Workspace scope, original ownership and complete receipt requirements,
  plus denial of finalization after generic unknown-resource confirmation;
- malformed/foreign persisted claims rejecting with unchanged database contents;
- claim-update failure rolling back the receipt and runtime-resource retirement,
  followed by successful retry;
- concurrent exact reconciliations and preservation of effects, unknown resources,
  capacity and Dispatch Holds.

Actual checks on this snapshot:

```sh
cargo test -p forge-store durable_execution::tests::recovery_tests
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

All passed: 20 focused recovery tests (five new), 725 Rust harness tests plus
seven custom-harness supervised scenarios, one pre-existing opt-in test ignored,
runner typecheck and 64 Node tests. These new tests are SQLite policy fixtures,
not new real-process recovery or cleanup demonstrations. Existing supervised tests
were rerun unchanged. No packaged-component, real-provider or physical power-loss
verification ran. No live configuration, Assignment or worktree was modified.

## Implementation review

T10 asks to “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates”: **partial**.
Only safe cancellation of a reservation is added here. Recovery-authorizing claim
consumption, daemon wiring, fresh bounded permission and writable reopen remain.

“Safe restart resumes the same attempt/conversation with no duplicate admission”
and “Daemon and runner restart order cannot bypass gates” remain unfulfilled as a
complete integrated flow. This increment exercises persisted cancellation/retry,
not successful recovery. “Continuation and independent Review successors receive
fresh conversations and exact approved context” still lacks the new durable
recovery/successor demonstration. Glossary/ADR recovery completion also remains.

Summary: T10 remains incomplete; authorized writable recovery and integration
verification are the main gaps. No scope creep or incorrect authority claim found.

## Coding-standards review

Reviewed changed application policy, store reconciliation, new tests and storage/
operations evidence against `CODING_STANDARDS.md` and surrounding retirement and
claim contracts. Policy is application-owned, persistence is transactional, and no
external I/O enters the writer transaction. Scope precedes historical replay;
failed updates preserve receipt/resource state together. Tests cover denied scope,
revocation, malformed bindings, conflicts, concurrent reconciliation and rollback
using isolated databases. No credential or prompt exposure, schema/wire break,
worktree cleanup or blanket uncertainty waiver is introduced.

Summary: no introduced coding-standards violations found. This self-review does
not replace independent T18 review or establish T10/release completion.
