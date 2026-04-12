# Arrow.js Frontend Port — Design Spec

**Date:** 2026-04-12
**Scope:** Port sola-terminal frontend from imperative DOM to Arrow.js reactive templates. Establish reusable patterns for future `crates/sola-app` shared crate.

## Goal

Replace imperative DOM rendering with Arrow.js `html` templates and `reactive()` state. Structure the frontend so that IPC, state patterns, and UI primitives are extractable to a shared `sola-app` crate that all Sola WebView apps can use.

## Module Structure

```
web/src/
  lib/                    # Extractable to sola-app crate
    ipc.ts                # WebKit message handler bridge (postMessage + evaluate_javascript)
    store.ts              # createStore<T>() wrapper over reactive(), persist() for localStorage
    theme.ts              # Theme system via CSS custom properties
  components/             # Extractable UI primitives
    sidebar.ts            # Generic sidebar: tabs, collapse, resize, drag-reorder, rename
  app.ts                  # Terminal-specific: reactive state, tab lifecycle, layout
  terminal-pane.ts        # Terminal-specific: xterm.js wrapper (renamed from terminal.ts)
  main.ts                 # Entry point
  theme.css               # Terminal theme values
web/tsconfig.json         # Editor tooling only (not part of build)
```

**Separation principle:** `lib/` and `components/` have zero terminal knowledge. Root `src/` files are app-specific. When extracted, `lib/` + `components/` + Rust WebView host code become `crates/sola-app/`.

## Reactive State Pattern (lib/store.ts)

Arrow.js `reactive()` creates observable state. Property mutations auto-trigger re-renders in any `html` template that references them.

`store.ts` provides:
- `createStore<T>(initial): Reactive<T>` — typed wrapper over `reactive()`
- `persist(store, key, pick)` — localStorage round-trip for selected properties

IPC integration is explicit, not automatic:
```ts
const result = await invoke('spawn_pty', { cols, rows });
state.tabs = [...state.tabs, { id: result.pty_id, ... }];
```

No auto-sync framework. Apps call `invoke()` and mutate state explicitly.

## IPC (lib/ipc.ts)

Moved from `src/ipc.ts`. API unchanged:
- `invoke(cmd, args): Promise<any>` — send command via `postMessage`, receive response via `__solaRecv`
- `on(event, callback): () => void` — subscribe to server-pushed events

## Theme (lib/theme.ts)

CSS custom properties set on `:root`, reactive to state changes. Apps provide theme values, the lib applies them. Minimal — just the plumbing for other apps to follow the same pattern.

## Sidebar Component (components/sidebar.ts)

Replaces 300 lines of imperative DOM with Arrow.js `html` templates (~60 lines of template + event handlers).

Takes a generic config:
- Data accessors: tabs list, active tab, collapsed state, width
- Callbacks: select, close, create, toggle, resize, reorder, rename

Drag-reorder and rename stay as imperative mouse-state logic. Rendering becomes declarative via Arrow.js templates.

**WebKit6 risk:** Arrow.js `html` template mounting (`html\`...\`(element)`) previously threw "Invalid HTML position" under `load_html()` with null origin. Now running under `app:///` with a proper origin — may work. If it fails, debug and adapt (manual fragment mounting as fallback).

## App State (app.ts)

Single `reactive()` store for all terminal state:
```ts
const state = createStore({
  tabs: [] as Tab[],
  activeTabId: null as string | null,
  sidebarCollapsed: false,
  sidebarWidth: 160,
});
```

Tab management functions mutate state directly — no more `rerenderSidebar()` calls. Arrow.js auto-updates the sidebar template.

TerminalPane lifecycle and pane container management stays imperative (xterm.js needs real DOM elements).

## terminal-pane.ts

Renamed from `terminal.ts`. No Arrow.js changes — xterm.js wrapper with PTY lifecycle. Keeps callback-based API (doesn't know about the store).

## tsconfig.json

Editor tooling only. Provides type checking and autocomplete for vendored deps:
```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ES2022",
    "moduleResolution": "bundler",
    "strict": true,
    "noEmit": true,
    "paths": {
      "@arrow-js/core": ["./vendor/arrow/index.mjs"],
      "@xterm/*": ["./vendor/*"]
    }
  },
  "include": ["src/**/*.ts"]
}
```

## Rust Side Changes

Update URI scheme handler in `main.rs`:
- Add routes for new paths: `/src/lib/ipc.ts`, `/src/lib/store.ts`, `/src/lib/theme.ts`, `/src/components/sidebar.ts`
- Update existing routes: `/src/terminal.ts` → `/src/terminal-pane.ts`
- Remove old routes for moved files
- Embed new `include_str!` assets

## Future: crates/sola-app

Once patterns stabilize across 2-3 apps, extract:
- **Rust:** WebContext setup, `app:///` URI scheme, TS stripping, UserContentManager bridge, glib↔tokio channel plumbing
- **TypeScript:** `lib/` + `components/` embedded in the crate

Apps would depend on `sola-app` and get the full WebView host + JS runtime with one crate dependency.
