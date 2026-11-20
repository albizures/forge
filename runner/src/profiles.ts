// Explicit T06 host composition. No discovery, store open or scheduling here.
// The trusted T07 launcher must bind this input to its Check and deliver behavior
// only after authenticated bootstrap. None of these inputs is an agent API.
import { closeSync, constants, fstatSync, lstatSync, openSync, readSync, realpathSync, statSync } from 'node:fs';
import { dirname, isAbsolute } from 'node:path';
import { createHash } from 'node:crypto';
import { createModels, type AssistantMessageEvent, type AssistantMessage, type Provider, type Model, type Api } from '@earendil-works/pi-ai';
import { AssistantMessageEventStream } from '@earendil-works/pi-ai/utils/event-stream';
import { openaiProvider } from '@earendil-works/pi-ai/providers/openai';
import { createRegistry, defineExtension, type AgentChange, type HarnessOptions, type ToolRegistration } from '@earendil-works/pi-durable';
import { NodeExecutionEnv } from '@earendil-works/pi-durable/env/node';
import { createBashTool, createEditTool, createReadTool, createWriteTool } from '@earendil-works/pi-durable/tools';
import { Authorization, guardProvider } from './authorization.ts';
import { guardEffectTool } from './interrupted-effects.ts';
import { Denied, exactKeys, requireCondition } from './contract.ts';

const MAX_CONTEXT = 256 * 1024;
const allowedTools = ['read', 'write', 'edit', 'bash'];
const capabilityNames = ['FORGE_ASSIGNMENT_ID', 'FORGE_ASSIGNMENT_DAEMON_URL', 'FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE', 'FORGE_ASSIGNMENT_FORGE_CLI'];
type Resource = { name: string; path: string; sha256: string };
export interface Configuration {
  version: 1;
  model: 'openai/gpt-4.1';
  thinkingLevel: 'off';
  tools: string[];
  context: { instructions: string; resources: Resource[] };
  cwd: string;
}

function regularCanonical(path: string): void {
  requireCondition(isAbsolute(path) && realpathSync(path) === path && lstatSync(path).isFile(), 'durable-resource-invalid');
}

function boundedRead(fd: number, limit: number): Buffer {
  const bytes = Buffer.alloc(limit + 1);
  let used = 0;
  while (used < bytes.length) {
    const count = readSync(fd, bytes, used, bytes.length - used, null);
    if (!count) break;
    used += count;
  }
  requireCondition(used <= limit, 'durable-input-too-large');
  return bytes.subarray(0, used);
}

/** The sole initial auth mechanism: deliberately selected raw API-key file.
 * No environment value, Pi credential store, capability or OAuth fallback.
 * All failures omit upstream errors, paths and credential bytes.
 */
export function loadModelKey(path: string | undefined): string {
  let fd: number | undefined;
  try {
    requireCondition(typeof path === 'string', 'durable-model-auth-unavailable');
    regularCanonical(path);
    const parent = lstatSync(dirname(path));
    requireCondition(realpathSync(dirname(path)) === dirname(path) && parent.isDirectory() && parent.uid === process.getuid!() && (parent.mode & 0o7777) === 0o700, 'durable-model-auth-unavailable');
    fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
    const meta = fstatSync(fd);
    requireCondition(meta.isFile() && meta.nlink === 1 && meta.uid === process.getuid!() && (meta.mode & 0o7777) === 0o600, 'durable-model-auth-unavailable');
    const bytes = boundedRead(fd, 4097);
    const token = bytes.at(-1) === 10 ? bytes.subarray(0, -1) : bytes;
    requireCondition(token.length > 0 && token.length <= 4096 && token.every(byte => byte >= 0x21 && byte <= 0x7e), 'durable-model-auth-unavailable');
    return token.toString('ascii');
  } catch {
    throw new Denied('durable-model-auth-unavailable');
  } finally {
    if (fd !== undefined) {
      try { closeSync(fd); } catch { throw new Denied('durable-model-auth-unavailable'); }
    }
  }
}

function configuration(input: unknown): Configuration {
  try {
    exactKeys(input, ['version', 'model', 'thinkingLevel', 'tools', 'context', 'cwd']);
    requireCondition(input.version === 1 && input.model === 'openai/gpt-4.1' && input.thinkingLevel === 'off', 'durable-profile-incompatible');
    requireCondition(Array.isArray(input.tools) && input.tools.every(tool => typeof tool === 'string' && allowedTools.includes(tool)) && new Set(input.tools).size === input.tools.length, 'durable-profile-incompatible');
    requireCondition(typeof input.cwd === 'string' && isAbsolute(input.cwd) && realpathSync(input.cwd) === input.cwd && statSync(input.cwd).isDirectory(), 'durable-profile-incompatible');
    exactKeys(input.context, ['instructions', 'resources']);
    requireCondition(typeof input.context.instructions === 'string' && Buffer.byteLength(input.context.instructions) <= MAX_CONTEXT && Array.isArray(input.context.resources), 'durable-profile-incompatible');
    let suffix = '';
    for (const resource of input.context.resources) {
      exactKeys(resource, ['name', 'path', 'sha256']);
      requireCondition(typeof resource.name === 'string' && resource.name.trim() && typeof resource.path === 'string' && resource.path.endsWith('.md') && typeof resource.sha256 === 'string' && /^[a-f0-9]{64}$/.test(resource.sha256), 'durable-profile-incompatible');
      regularCanonical(resource.path);
      const fd = openSync(resource.path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
      let bytes: Buffer;
      try {
        requireCondition(fstatSync(fd).isFile(), 'durable-profile-incompatible');
        bytes = boundedRead(fd, MAX_CONTEXT);
      } finally { closeSync(fd); }
      requireCondition(createHash('sha256').update(bytes).digest('hex') === resource.sha256, 'durable-profile-incompatible');
      suffix += '\n\n' + new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
      requireCondition(Buffer.byteLength(suffix) <= MAX_CONTEXT, 'durable-profile-incompatible');
    }
    requireCondition(input.context.instructions.endsWith(suffix), 'durable-profile-incompatible');
    return structuredClone(input) as unknown as Configuration;
  } catch { throw new Denied('durable-profile-incompatible'); }
}

// Defense at actual provider dispatch, not generation hooks. Only the selected
// model and immediate chat APIs are allowed; retries and ambient API env are off.
function redact(value: unknown, secret: string): unknown {
  if (typeof value === 'string') return value.replaceAll(secret, '[REDACTED]');
  if (Array.isArray(value)) return value.map(item => redact(item, secret));
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, redact(item, secret)]));
  return value;
}

function immediateOpenAI(provider: Provider, secret: string): Provider {
  return new Proxy(provider, {
    get(target, key) {
      if (key === 'auth') return { apiKey: target.auth.apiKey };
      if (['fetchDeferred', 'generateImages', 'classify', 'refreshModels'].includes(String(key))) return () => { throw new Denied('durable-request-unsupported'); };
      const value: unknown = Reflect.get(target, key, target);
      if (typeof value !== 'function') return value;
      if (key !== 'stream' && key !== 'streamSimple') return value.bind(target);
      return (model: Model<Api>, input: unknown, options: object = {}) => {
        requireCondition(model.provider === 'openai' && model.id === 'gpt-4.1', 'durable-model-unsupported');
        const source: AssistantMessageEventStream = Reflect.apply(value, target, [model, input, { ...options, maxRetries: 0, env: {} }]);
        const output = new AssistantMessageEventStream();
        void (async () => {
          try {
            for await (const event of source) output.push(redact(event, secret) as AssistantMessageEvent);
          } catch {
            const error: AssistantMessage = { role: 'assistant', api: model.api, provider: model.provider, model: model.id, content: [], stopReason: 'error', errorMessage: 'durable-model-request-failed', timestamp: Date.now(), usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } } };
            output.push({ type: 'error', reason: 'error', error });
          } finally { output.end(); }
        })();
        return output;
      };
    },
  });
}

/** Build explicit runtime inputs without opening storage or admitting work.
 * Capability ENV contains metadata and the protected secret-file PATH only.
 * Model keys live solely in a private auth-context closure, never child env.
 * Caller owns the lease, exclusive storage ownership and bootstrap gates.
 */
export function createProfileRuntime(input: unknown, credentialFile: string | undefined, authorization: Authorization, capabilityEnv: Record<string, string> = {}): { options: HarnessOptions; agent: AgentChange } {
  const config = configuration(input);
  requireCondition(Object.keys(capabilityEnv).every(name => capabilityNames.includes(name)) && Object.values(capabilityEnv).every(value => typeof value === 'string' && !value.includes('\0')), 'durable-capability-env-invalid');
  requireCondition(credentialFile === undefined || credentialFile !== capabilityEnv.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE, 'durable-model-auth-unavailable');
  const apiKey = loadModelKey(credentialFile);
  const models = createModels({ authContext: { env: async name => name === 'OPENAI_API_KEY' ? apiKey : undefined, fileExists: async () => false } });
  models.setProvider(guardProvider(immediateOpenAI(openaiProvider(), apiKey), authorization));
  const cli = capabilityEnv.FORGE_ASSIGNMENT_FORGE_CLI;
  requireCondition(cli === undefined || isAbsolute(cli), 'durable-capability-env-invalid');
  const environment = { PATH: cli ? `${dirname(cli)}:/usr/bin:/bin` : '/usr/bin:/bin', LANG: 'C.UTF-8', ...capabilityEnv };
  const env = new NodeExecutionEnv({ cwd: config.cwd, shellPath: '/bin/sh', shellEnv: environment });
  const factories: Record<string, () => ToolRegistration> = {
    read: createReadTool, write: createWriteTool, edit: createEditTool,
    bash: () => createBashTool({ prepare: execution => { execution.cwd = config.cwd; execution.env = { ...environment }; execution.inheritEnv = false; } }),
  };
  const tools = config.tools.map(name => {
    const tool = factories[name]();
    return { ...tool, replay: 'unsafe' as const, execute: guardEffectTool(tool.execute, authorization) };
  });
  const registry = createRegistry();
  const extension = defineExtension({ name: 'forge-coding', tools });
  registry.install(extension);
  return {
    options: {
      models, registry,
      settings: { retry: { enabled: false }, compaction: { enabled: false } },
      env: target => {
        requireCondition(target.cwd === config.cwd, 'durable-cwd-mismatch');
        return env;
      },
    },
    agent: { model: { provider: 'openai', modelId: 'gpt-4.1' }, thinkingLevel: 'off', cwd: config.cwd, instructions: config.context.instructions, tools, extensions: [extension] },
  };
}
