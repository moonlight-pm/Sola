// @sola/kit — umbrella client helpers shipped with sola-kit.
//
// Imported by every kit-based app at startup:
//
//   import { setupKit } from "@sola/kit";
//   setupKit();
//
// Components that need event handlers also use the typed `on` from
// here instead of the raw `@remix-run/ui` one — see comment on `on`
// below.

import { on as remixOn } from "@remix-run/ui";
import { on as ipcOn } from "@sola/ipc";

// ── Theme types ─────────────────────────────────────────────────────
//
// Hand-written mirror of the Rust `sola_core::theme::*` types. serde
// serializes BTreeMap as a JSON object and unit-style enum variants
// as the variant name string, so these structural types match the
// payload byte-for-byte with no transform needed.
//
// If the Rust schema grows, mirror the change here. (Auto-generation
// would be overkill for ~6 small types and one editor consumer.)

export type TokenKind =
  | "Color"
  | "FontFamily"
  | "TextSize"
  | "Space"
  | "Radius";

// ── Token resolution ───────────────────────────────────────────────
//
// Components that accept a "design-system or raw CSS" value (Stack
// gap, Swatch size, future padding/radius props) take a SpaceValue:
// either a semantic tag like "md" — which expands to
// `var(--space-md)` — or any raw CSS length like "12px" / "0.5rem"
// which passes through verbatim. The `(string & {})` intersection
// keeps TS literal-union autocomplete for the tags without
// collapsing the type back to plain `string`.

export type SpaceTag = "xs" | "sm" | "md" | "lg" | "xl" | "xxl";
export type SpaceValue = SpaceTag | (string & {});

const SPACE_TAGS: ReadonlySet<string> = new Set([
  "xs",
  "sm",
  "md",
  "lg",
  "xl",
  "xxl",
]);

/** Tag → `var(--space-${tag})`; raw CSS lengths pass through. */
export function resolveSpace(value: SpaceValue): string {
  return SPACE_TAGS.has(value) ? `var(--space-${value})` : value;
}

export interface Token {
  kind: TokenKind;
  value: string;
  groups: string[];
}

export interface Binding {
  group: string;
  token: string;
}

export interface ComponentBindings {
  slots: Record<string, Binding>;
}

export interface Palette {
  tokens: Record<string, Token>;
}

export interface Theme {
  palette: Palette;
  components: Record<string, ComponentBindings>;
}

/**
 * Typed `on()` wrapper that pre-fixes `target = HTMLElement`.
 *
 * The raw Remix v3 `on` defaults its target type parameter to
 * `Element`, whose `ElementEventMap` only carries a small set of
 * generic events (no `keydown`, no `keyup`, etc.). Inside Remix's own
 * components that's not a problem — they call `on()` from inside a
 * `createMixin<HTMLElement>(...)` body, which propagates target
 * context. When you call `on()` directly inside a JSX `mix={[…]}`
 * array, that context isn't available, and the type-checker rejects
 * any keyboard / focus / pointer event.
 *
 * Pre-fixing target to `HTMLElement` here gives `EventType<target> =
 * keyof HTMLElementEventMap`, which covers the events kit components
 * actually use, and the handler's `event` parameter then narrows
 * automatically to the matching specific type (`KeyboardEvent` for
 * `"keydown"`, `MouseEvent` for `"click"`, etc.). No
 * call-site type parameter needed.
 *
 * If you ever need an SVG element or a non-HTML EventTarget, fall
 * back to the raw `on` from `@remix-run/ui` and supply target/type
 * yourself.
 */
export function on<type extends keyof HTMLElementEventMap>(
  type: type,
  // Derive the handler shape from the underlying `remixOn` signature
  // with both type parameters substituted — keeps us 1:1 with whatever
  // Remix considers a valid listener for `(HTMLElement, type)` (return
  // type, AbortSignal arg, event-type wrapping) without restating it.
  handler: Parameters<typeof remixOn<HTMLElement, type>>[1],
  capture?: boolean,
) {
  return remixOn<HTMLElement, type>(type, handler, capture);
}

// ── Theme store ─────────────────────────────────────────────────────
//
// Module-singleton holding the latest Theme delivered over the bus.
// `setupKit()` updates it on every `theme` event; `getTheme()` and
// `onThemeChange()` are the read paths.
//
// This is deliberately not built on a signal library — Remix v3 uses
// Handle-based reactivity (handle.update() to schedule a re-render),
// so editor components subscribe in their factory body, capture the
// fresh theme into a closure local, and call handle.update() on each
// notification. Five lines of subscriber bookkeeping is a smaller
// dependency surface than an external store.

type ThemeListener = (theme: Theme) => void;

let currentTheme: Theme | null = null;
const themeListeners = new Set<ThemeListener>();

/**
 * The most recently received Theme, or `null` if no `theme` event
 * has arrived yet (the kit's bus pump replays the sticky theme
 * immediately on subscribe, so this is null only during the first
 * tick of an app's lifetime).
 */
export function getTheme(): Theme | null {
  return currentTheme;
}

/**
 * Subscribe to theme updates. The listener is invoked once
 * synchronously with the current theme if one is already known, then
 * again on every subsequent `theme` event.
 *
 * Returns a dispose function that unregisters the listener; call it
 * when the consumer (typically a Remix v3 component) is no longer
 * interested. There's no automatic unmount cleanup — components must
 * stash the disposer and call it themselves if they have a finite
 * lifetime.
 */
export function onThemeChange(listener: ThemeListener): () => void {
  themeListeners.add(listener);
  if (currentTheme) listener(currentTheme);
  return () => themeListeners.delete(listener);
}

/**
 * Wire up the kit's renderer-side bridge. Call once from the app's
 * entry point (typically `index.tsx`) before mounting the root.
 *
 * What it does today:
 *
 * - Installs a constructable stylesheet on `document.adoptedStyleSheets`.
 *   The Rust side of the kit listens to `Topic::Theme`, lowers the
 *   theme to a `:root { … }` CSS block via `Theme::to_css()`, and
 *   pushes it through `__solaRecv` as
 *   `{ event: "theme", css: "<root>{…}", definition: { … } }`. This
 *   handler does `replaceSync(msg.css)` on the stylesheet — a single
 *   allocation, no DOM mutation, hot-reloadable on every theme
 *   update including the sticky replay at first connect.
 *
 * - Updates the in-process theme store from `msg.definition` (the
 *   structured palette + bindings input that produced the CSS) so
 *   editor components reading via `getTheme()` / `onThemeChange()`
 *   see the structured Theme, not just the rendered output.
 *
 * Anything else the kit decides to bootstrap on every app (IPC
 * lifecycle hooks, common signals, etc.) should land here so apps
 * keep their entry point a one-liner.
 */
export function setupKit() {
  const themeSheet = new CSSStyleSheet();
  document.adoptedStyleSheets = [
    ...document.adoptedStyleSheets,
    themeSheet,
  ];
  ipcOn("theme", (msg: { css?: string; definition?: Theme }) => {
    if (msg.css) themeSheet.replaceSync(msg.css);
    if (msg.definition) {
      currentTheme = msg.definition;
      for (const listener of themeListeners) listener(msg.definition);
    }
  });
}
