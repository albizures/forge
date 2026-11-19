// Consumer-side defensive enforcement only. This module cannot issue authority.
// Delivery must be restricted to the trusted daemon; JSON is not authentication.
import { readFileSync } from 'node:fs';
import { withAbortSignal, type Context } from '@earendil-works/chord/context';
import type { Provider } from '@earendil-works/pi-ai';
import { canonical, Denied, exactKeys, requireCondition, type Launch } from './contract.ts';

const denied = () => new Denied('assignment-lease-denied');
const leaseKeys = ['launch', 'generation', 'owner', 'daemon_epoch', 'sequence', 'issued_boottime_millis', 'expires_boottime_millis'];
const ownerKeys = ['boot_id', 'time_namespace', 'host_pid', 'host_start', 'worker_pid', 'worker_start', 'lock_device', 'lock_inode'] as const;
export type Owner = Record<Exclude<typeof ownerKeys[number], 'host_pid' | 'worker_pid'>, string> & { host_pid: number; worker_pid: number };

// /proc/uptime is CLOCK_BOOTTIME in the reader's time namespace, truncated to
// centiseconds. Round UP to deny early, never authorize past the real deadline.
// The trusted host must verify matching boot/time namespace before delivery.
export function hostClockUpperBound(read = (path: string) => readFileSync(path, 'utf8')): number {
  const match = /^(\d+)\.(\d{2}) \d+\.\d{2}\n?$/.exec(read('/proc/uptime'));
  if (!match) throw denied();
  const now = Number(match[1]) * 1000 + Number(match[2]) * 10 + 10;
  requireCondition(Number.isSafeInteger(now), 'assignment-lease-denied');
  return now;
}

interface Grant {
  sequence: number;
  daemon_epoch: string;
  issued_boottime_millis: number;
  expires_boottime_millis: number;
}

/** One process incarnation; no persisted grant adoption or post-expiry revival.
 * Inputs and accept() belong to trusted composition, never a tool/agent API.
 * The JS timer is cooperative cancellation, NOT the independent host watchdog.
 */
export class Authorization {
  #controller = new AbortController();
  #binding: string;
  #execution: string;
  #previous: Grant | undefined;
  #payload: string | undefined;
  #lastNow: number | undefined;
  #clock: () => number;
  #timer: ReturnType<typeof setInterval>;

  constructor(launch: Launch, generation: number, owner: Owner, daemonEpoch: string, clock = hostClockUpperBound) {
    requireCondition(Number.isSafeInteger(generation) && generation > 0 && typeof daemonEpoch === 'string' && daemonEpoch.trim(), 'assignment-lease-denied');
    this.#binding = canonical({ launch, generation, owner, daemon_epoch: daemonEpoch });
    this.#execution = canonical({ launch, generation });
    this.#clock = clock;
    this.#timer = setInterval(() => { try { this.check(); } catch { /* check terminally aborts; no permission is returned. */ } }, 50);
    this.#timer.unref();
  }

  get signal(): AbortSignal { return this.#controller.signal; }

  #fence(): never {
    clearInterval(this.#timer);
    this.#controller.abort(denied());
    throw denied();
  }

  #now(): number {
    if (this.signal.aborted) throw denied();
    try {
      const now = this.#clock();
      if (!Number.isSafeInteger(now) || now < 0 || (this.#lastNow !== undefined && now < this.#lastNow)) return this.#fence();
      this.#lastNow = now;
      return now;
    } catch { return this.#fence(); }
  }

  /** Consume only a committed grant from authenticated daemon delivery. Exact
   * retries do not extend deadlines; a failed/late/conflicting delivery fences.
   * Safe-integer checks reject lossy u64 JSON rather than rounding authority.
   */
  accept(value: unknown): void {
    try {
      const now = this.#now();
      exactKeys(value, leaseKeys);
      exactKeys(value.owner, ownerKeys);
      requireCondition(canonical({ launch: value.launch, generation: value.generation, owner: value.owner, daemon_epoch: value.daemon_epoch }) === this.#binding, 'assignment-lease-denied');
      for (const key of ['sequence', 'issued_boottime_millis', 'expires_boottime_millis']) {
        requireCondition(Number.isSafeInteger(value[key]) && value[key] >= 0, 'assignment-lease-denied');
      }
      requireCondition(value.sequence > 0 && value.sequence <= 4294967295 && typeof value.daemon_epoch === 'string' && value.daemon_epoch.trim(), 'assignment-lease-denied');
      requireCondition(value.issued_boottime_millis <= now && now < value.expires_boottime_millis && value.expires_boottime_millis - value.issued_boottime_millis === 30000, 'assignment-lease-denied');
      const previous = this.#previous;
      if (previous) {
        requireCondition(now < previous.expires_boottime_millis, 'assignment-lease-denied');
        if (canonical(value) === this.#payload) return;
        requireCondition(value.daemon_epoch === previous.daemon_epoch && value.sequence === previous.sequence + 1 && value.issued_boottime_millis >= previous.issued_boottime_millis && value.expires_boottime_millis > previous.expires_boottime_millis, 'assignment-lease-denied');
      } else {
        requireCondition(value.sequence === 1, 'assignment-lease-denied');
      }
      // Retain primitives only; mutation of a caller-owned payload cannot renew.
      this.#previous = { sequence: value.sequence, daemon_epoch: value.daemon_epoch, issued_boottime_millis: value.issued_boottime_millis, expires_boottime_millis: value.expires_boottime_millis };
      this.#payload = canonical(value);
    } catch { this.#fence(); }
  }

  /** Recheck at the actual effect boundary, not merely at submission or hooks. */
  admit(): void {
    this.check();
    if (!this.#previous) throw denied();
  }

  /** Trusted composition must not borrow a valid lease for another Check. */
  admitFor(launch: Launch, generation: number): void {
    this.admit();
    if (canonical({ launch, generation }) !== this.#execution) this.#fence();
  }

  check(): void {
    const now = this.#now();
    if (this.#previous && now >= this.#previous.expires_boottime_millis) this.#fence();
  }

  /** Closing/revocation also cancels; disposal never leaves a valid permission. */
  close(): void {
    clearInterval(this.#timer);
    this.#controller.abort(denied());
  }
}

// Complete effect-producing request surface of pinned pi-ai 1.5.2. Cancellation
// and catalog/auth metadata stay callable after expiry. Upgrades must re-audit.
const requests = new Set<PropertyKey>(['stream', 'streamSimple', 'fetchDeferred', 'generateImages', 'classify']);
export function guardProvider<T extends Provider>(provider: T, authorization: Authorization): T {
  return new Proxy(provider, {
    get(target, key) {
      const value: unknown = Reflect.get(target, key, target);
      if (typeof value !== 'function') return value;
      if (!requests.has(key)) return value.bind(target);
      return (model: unknown, input: unknown, options: { signal?: AbortSignal } = {}) => {
        authorization.admit();
        const signal = options.signal ? AbortSignal.any([options.signal, authorization.signal]) : authorization.signal;
        signal.throwIfAborted();
        return Reflect.apply(value, target, [model, input, { ...options, signal }]);
      };
    },
  });
}

export function guardTool<Args, Api, Result>(execute: (args: Args, api: Api, context: Context) => Result, authorization: Authorization) {
  return (args: Args, api: Api, context: Context): Result => {
    authorization.admit();
    const guarded = withAbortSignal(authorization.signal, context);
    guarded.abortSignal!.throwIfAborted();
    return execute(args, api, guarded);
  };
}
