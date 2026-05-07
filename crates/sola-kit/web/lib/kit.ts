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
 *   `{ event: "theme", css: "<root>{…}" }`. This handler does
 *   `replaceSync(msg.css)` on the stylesheet — a single allocation,
 *   no DOM mutation, hot-reloadable on every theme update including
 *   the sticky replay at first connect.
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
  ipcOn("theme", (msg: { css?: string }) => {
    if (msg.css) themeSheet.replaceSync(msg.css);
  });
}
