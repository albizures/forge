# Preserve evidence when autonomous Task dispatch cannot proceed

Forge will dispatch eligible Tasks through durable Provisioning Assignments, holding Project capacity until the attempt either provisions or closes. It deliberately retains Task Branches, Managed Worktrees, provider-resource identities, and diagnostics on failure rather than automatically resetting, deleting, or adopting state; this favors recoverable, auditable operator decisions over unsafe automatic repair.
