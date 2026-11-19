// Local bounded diagnostic, not the T11 operator CLI/RPC or pagination contract.
import { snapshot, validateDirectory } from './storage.ts';
import { Denied } from './contract.ts';
try {
  const [directory, worktree] = process.argv.slice(2);
  validateDirectory(directory, worktree);
  console.log(JSON.stringify(await snapshot(directory)));
} catch (error) {
  console.error(error instanceof Denied ? error.code : 'runner-inspection-failed');
  process.exitCode = 1;
}
