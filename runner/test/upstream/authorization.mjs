// Research mechanism only. Forge authority and renewal persistence are NOT
// implemented here; tests supply grants and a deterministic monotonic clock.
import { withAbortSignal } from '@earendil-works/chord/context';
import { readFileSync } from 'node:fs';

// Linux prints CLOCK_BOOTTIME (plus time-namespace offset), truncated to
// centiseconds. Use the next centisecond as a conservative upper bound for
// admission. The supervisor grants an absolute deadline in the SAME namespace.
export function hostClockUpperBound(read = path => readFileSync(path, 'utf8')) {
  const match = /^(\d+)\.(\d{2}) [\d.]+\n?$/.exec(read('/proc/uptime'));
  if (!match) throw new Error('Host boottime unavailable');
  const now = Number(match[1]) * 1000 + Number(match[2]) * 10 + 10;
  if (!Number.isSafeInteger(now)) throw new Error('Host boottime outside safe range');
  return now;
}

export class Clock {
  now = 0;
  listeners = new Set();
  advance(ms) {
    if (ms < 0) throw new Error('Monotonic clock cannot go backwards');
    this.now += ms;
    for (const listener of this.listeners) listener();
  }
}
export class Lease {
  controller = new AbortController();
  constructor(clock, duration = 30000) {
    this.clock = clock;
    this.deadline = clock.now + duration;
    this.tick = () => { if (clock.now >= this.deadline) this.controller.abort(new Error('Lease expired')); };
    clock.listeners.add(this.tick);
  }
  get signal() { return this.controller.signal; }
  admit() {
    this.tick();
    this.signal.throwIfAborted();
  }
  dispose() { this.clock.listeners.delete(this.tick); }
}
const requests = new Set(['stream', 'streamSimple', 'fetchDeferred', 'generateImages', 'classify']);
export function guardProvider(provider, lease) {
  return new Proxy(provider, {
    get(target, key) {
      const value = Reflect.get(target, key, target);
      if (typeof value !== 'function') return value;
      if (!requests.has(key)) return value.bind(target);
      return (model, input, options = {}) => {
        lease.admit();
        const signal = options.signal ? AbortSignal.any([options.signal, lease.signal]) : lease.signal;
        return value.call(target, model, input, { ...options, signal });
      };
    },
  });
}
export function guardTool(execute, lease) {
  return (args, api, context) => {
    lease.admit();
    return execute(args, api, withAbortSignal(lease.signal, context));
  };
}
