import { createSubscriber } from "svelte/reactivity";

export function else_if_NaN(value: unknown, defaultValue: number): number {
  if (typeof value === "number" && !Number.isNaN(value)) {
    return value;
  }
  return defaultValue;
}

export function normalizeSeconds(value: unknown): number {
  const n = Number(value);
  if (!Number.isFinite(n) || n <= 0) return 0;
  return Math.floor(n);
}

export function safeJson<T>(json: string): T | null {
  try {
    return JSON.parse(json) as T;
  } catch {
    return null;
  }
}

export async function safeJsonAsync<T>(res: Response): Promise<T | null> {
  try {
    return (await res.json()) as T;
  } catch {
    return null;
  }
}

export class InternalReactiveValue<T> {
  #fn;
  #subscribe;

  constructor(fn: () => T, onsubscribe: (update: () => void) => void) {
    this.#fn = fn;
    this.#subscribe = createSubscriber(onsubscribe);
  }

  get current() {
    this.#subscribe();
    return this.#fn();
  }
}
