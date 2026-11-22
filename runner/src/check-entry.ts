// Packaged durable runner entry (Forgeup `ENTRY_RELATIVE`). The production Check
// runner with the explicit profile's real provider; no substitution, no faux
// model and no ambient Pi discovery. Spawned only by forge-runner-host.
import { runCheckRunner } from './check-runner.ts';

const [directory, worktree] = process.argv.slice(2);
process.exitCode = await runCheckRunner(directory, worktree);
