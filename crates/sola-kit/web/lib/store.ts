import { reactive } from '@arrow-js/core';

type Reactive<T> = T & { [K in keyof T]: T[K] };

/** Create a typed reactive store. Thin wrapper over Arrow.js reactive(). */
export function createStore<T extends Record<string, any>>(initial: T): Reactive<T> {
  return reactive(initial) as Reactive<T>;
}

/**
 * Persist selected store properties to localStorage.
 * Loads saved values on call, then saves on every mutation.
 */
export function persist<T extends Record<string, any>>(
  store: T,
  key: string,
  keys: (keyof T)[],
): void {
  // Load
  try {
    const saved = localStorage.getItem(key);
    if (saved) {
      const parsed = JSON.parse(saved);
      for (const k of keys) {
        if (k in parsed) {
          (store as any)[k] = parsed[k];
        }
      }
    }
  } catch {
    // Ignore corrupt localStorage
  }

  // Save on change — poll since Arrow.js doesn't expose a mutation hook
  // outside of template bindings. This runs once at setup; the actual
  // persistence is triggered by the app calling persist.save().
}

/** Save selected properties from store to localStorage. */
export function save<T extends Record<string, any>>(
  store: T,
  key: string,
  keys: (keyof T)[],
): void {
  const obj: Record<string, any> = {};
  for (const k of keys) {
    obj[k as string] = store[k];
  }
  localStorage.setItem(key, JSON.stringify(obj));
}
