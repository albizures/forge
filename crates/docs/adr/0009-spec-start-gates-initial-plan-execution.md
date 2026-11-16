# Gate initial Spec plan execution through Spec start

[ADR 0011](0011-derive-stacks-from-builds-on-relationships.md) supersedes the separate prerequisite/Stack declaration model below, retaining this Spec-start execution gate and immutable Check safeguards.

Task planning requires several configuration writes, and executing a partially assembled plan can bind a successor to the wrong repository evidence. Use the existing Open-to-In-Progress Spec transition to validate the initial plan and enable child execution, rather than introduce a separate plan-release lifecycle or rely on planner timing; dependency ordering and Stack inheritance remain distinct.

Initially reject ordinary structural edits after a Spec starts, while preserving explicit recovery under ownership and evidence safeguards. This is a bounded product limitation, not a claim that an In Progress Spec is inherently immutable: future replanning may safely change remaining work, but must preserve the simple Record lifecycle and protect started and completed evidence. Existing Tasks and immutable Check pins are not automatically converted.
