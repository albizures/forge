import { createHash } from 'node:crypto';
import { readdirSync, readFileSync } from 'node:fs';

export class Denied extends Error {
  readonly code: string;
  constructor(code: string) { super(code); this.code = code; }
}
export function requireCondition(condition: unknown, code: string): asserts condition {
  if (!condition) throw new Denied(code);
}
export function componentHash(): string {
  const hash = createHash('sha256');
  // Every shipped source module, so a new module can never escape attestation.
  const sources = readdirSync(new URL('../src/', import.meta.url)).filter(name => name.endsWith('.ts')).sort().map(name => 'src/' + name);
  for (const path of ['package-lock.json', 'prepare-reader.mjs', ...sources]) {
    hash.update(path).update(readFileSync(new URL('../' + path, import.meta.url)));
  }
  return hash.digest('hex');
}
export const compatibility = () => ({ protocol: 1, runtime_schema: 2, component_hash: componentHash() });
export const launchKeys = ['assignment_id', 'operation_id', 'attempt_id', 'profile_revision', 'runtime_id', 'incarnation_id', 'conversation_id', 'store_id', 'forge_revision', 'component_hash', 'runtime_schema'] as const;
export type Launch = Record<Exclude<typeof launchKeys[number], 'profile_revision' | 'runtime_schema'>, string> & { profile_revision: number; runtime_schema: number };
export function exactKeys(value: unknown, keys: readonly string[]): asserts value is Record<string, any> {
  requireCondition(value !== null && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).sort().join() === [...keys].sort().join(), 'runner-request-invalid');
}
export function canonical(value: unknown): string {
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']';
  if (value !== null && typeof value === 'object') return '{' + Object.entries(value).sort(([a], [b]) => a.localeCompare(b)).map(([k, v]) => JSON.stringify(k) + ':' + canonical(v)).join(',') + '}';
  return JSON.stringify(value);
}
export function validateLaunch(launch: unknown): asserts launch is Launch {
  exactKeys(launch, launchKeys);
  for (const key of launchKeys) {
    if (key !== 'profile_revision' && key !== 'runtime_schema') requireCondition(typeof launch[key] === 'string' && launch[key].trim(), 'durable-identity-invalid');
  }
  requireCondition(Number.isInteger(launch.profile_revision) && launch.profile_revision > 0 && launch.profile_revision <= 4294967295, 'durable-identity-invalid');
  requireCondition(launch.runtime_schema === 2 && launch.component_hash === componentHash(), 'runner-schema-incompatible');
}
export function validateRequest(request: unknown): asserts request is Record<string, any> {
  exactKeys(request, ['compatibility', 'assignment_id', 'runtime_id', 'operation_id', 'operation']);
  exactKeys(request.compatibility, ['protocol', 'runtime_schema', 'component_hash']);
  requireCondition(request.compatibility.protocol === 1, 'runner-protocol-incompatible');
  requireCondition(request.compatibility.runtime_schema === 2, 'runner-schema-incompatible');
  requireCondition(request.compatibility.component_hash === componentHash(), 'runner-component-incompatible');
  for (const key of ['assignment_id', 'runtime_id', 'operation_id']) requireCondition(typeof request[key] === 'string' && request[key].trim(), 'runner-identity-conflict');
  const operation = request.operation;
  requireCondition(operation && typeof operation === 'object', 'runner-request-invalid');
  if (operation.kind === 'admit') {
    exactKeys(operation, ['kind', 'launch']);
    validateLaunch(operation.launch);
    const launch = operation.launch;
    for (const key of ['assignment_id', 'runtime_id', 'operation_id'] as const) requireCondition(request[key] === launch[key], 'runner-identity-conflict');
  } else if (operation.kind === 'observe') {
    exactKeys(operation, ['kind', 'admission_operation_id']);
    requireCondition(typeof operation.admission_operation_id === 'string' && operation.admission_operation_id.trim(), 'runner-identity-conflict');
  } else {
    // Protocol 1 defines only admit/observe requests; recovery, follow-up and
    // retirement are typed supervisor controls. Anything else is malformed.
    throw new Denied('runner-request-invalid');
  }
}
