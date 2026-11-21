# T16 — Release/rollback gate for new durable admissions

Partial controlled deliverable: launch-eligibility gate only. Production durable dispatch and automatic recovery remain disabled.

## Delivered
- `durable_authorization::release_gate`: `ReleaseGate` wraps the production provider factory with a shared `LaunchControl` (`Withheld` default → `Controlled` explicit → sticky `Disabled`). Only `validate_profile`/`validate_preflight` for `pi-durable` profiles consult it; they never fall back to Herdr and never consult the inner factory while refused. `adapter_for` is deliberately ungated, so an existing Assignment keeps its original backend for inspection, recovery and retirement. Production composition wires the default (withheld); no flag/env can enable it before T19/T20.
- Profile pins, Workspace selection and Herdr/Grilling paths are untouched.

## Evidence
- 2 unit tests: withheld by default (inner never consulted, Herdr unaffected, adapter still resolvable); controlled enable → rollback refuses new launches with a distinct message, rollback is idempotent and sticky, original adapter still resolvable, Herdr still validates.
- Gates: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace --no-fail-fast` with real Node 24 first on PATH: 873 passed, 0 failed, 1 pre-existing opt-in ignored (run before a clippy `must_use` fix to the same file; target tests rerun green).

## Not claimed (T16 remains open)
- No new end-to-end automatic-scheduling / Implementation→Review handback / Continuation / unchanged-publication fixture was added for durable work; existing supervised Check scenarios (T07–T10) and Herdr sequencing tests are the only sequencing evidence.
- No live/closed-uncertain toggle test through real stores, no store-restart test of the control (it is in-process state, not persisted), no composed production-server test of the gate.
