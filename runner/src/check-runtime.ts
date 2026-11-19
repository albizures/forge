// Trusted T07 composition, not an entrypoint or agent lifecycle tool. The host
// still owns preflight, exclusive storage ownership, supervision and retention.
import { Authorization } from './authorization.ts';
import { bootstrapCheck, type CapabilityEnvironment, type CheckBinding } from './bootstrap.ts';
import { Denied, requireCondition, type Launch } from './contract.ts';
import { createProfileRuntime, type Configuration } from './profiles.ts';

/** Build model/tool inputs only after authenticated bootstrap. No conversation or
 * storage is opened here. A failure fences this consumer, never releases capacity
 * or establishes cleanup. The host must persist any uncertain execution state.
 */
export async function createCheckRuntime(
  launch: Launch,
  generation: number,
  bound: CheckBinding,
  capability: CapabilityEnvironment,
  configuration: Configuration,
  credentialFile: string,
  authorization: Authorization,
): Promise<ReturnType<typeof createProfileRuntime>> {
  try {
    // Snapshot before bootstrap's await; caller mutation cannot change the cwd,
    // selected configuration or capability environment used by tools afterward.
    const identity = structuredClone(launch);
    const expected = structuredClone(bound);
    const env = structuredClone(capability);
    const config = structuredClone(configuration);
    requireCondition(config.cwd === expected.executionDirectory, 'assignment-runtime-failed');
    const trusted = await bootstrapCheck(identity, generation, expected, env, authorization);
    authorization.admitFor(identity, generation);
    const runtime = createProfileRuntime(config, credentialFile, authorization, { ...env });
    // Preserve explicit approved resource loading; only Forge's authenticated
    // response supplies Assignment context and execution behavior. No model is
    // asked to interpret a bootstrap reply or decide whether it is authorized.
    return {
      ...runtime,
      agent: {
        ...runtime.agent,
        instructions: `${config.context.instructions}\n\nForge authenticated Assignment behavior:\n${trusted.behavior}\n\nForge authenticated Assignment context (JSON):\n${JSON.stringify(trusted.context)}`,
      },
    };
  } catch {
    authorization.close();
    throw new Denied('assignment-runtime-failed');
  }
}
