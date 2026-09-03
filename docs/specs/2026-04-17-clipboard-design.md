# Clipboard Design

**Date:** 2026-04-17
**Status:** Design, awaiting implementation plan.
**Worktree:** `.worktrees/clipboard`

## 1. Summary

Provide a uniform global copy/paste hotkey (Meta+C / Meta+V) that works across Sola apps, without requiring each app to bind the chord itself or define an Edit menu. Leaves the underlying Wayland selection machinery entirely to River.

## 2. Motivation

Under River, the system clipboard already works out of the box:
- Text selection + Ctrl+C / Ctrl+V in any WebKit WebView flows through GTK → GDK → `wl_data_device` and into River's selection.
- XWayland clients (e.g. Brave) participate via River's built-in XWayland clipboard bridge.
- The terminal (xterm.js) uses `navigator.clipboard` directly for its own Ctrl+Shift+C/V.

What's missing is a single, memorable, app-agnostic hotkey — the user wants Meta+C / Meta+V to act as universal copy/paste everywhere, not only where a given app happens to have bound them.

Previously the terminal defined an **Edit** menu with Copy/Paste entries shortcut to Meta+C/V, and the shell registered those chords via the `SetAppMenu` → `RegisteredChords` path. This spec removes the Edit menu from every app; copy/paste is now a shell-level concern, not an app-level one.

## 3. Non-goals

- Implementing our own Wayland selection bridge. River owns that.
- A `Clipboard` bus topic, cache directory, or any file-based payload plumbing. The previous (never-shipped) design's entire pipeline is discarded.
- Cut (Meta+X). Skipped for v1; adding later is symmetric.
- Primary-selection (middle-click paste). Unchanged; continues to work via River.
- "Mac-style everything" — Meta+T/W/R/L etc. inside non-Sola apps. Only Meta+C and Meta+V are synthesized. Adding more shortcuts is one-liner work when the need comes up.

## 4. Architecture

```
user presses Meta+C (Sola app focused)
   │
   ▼
sola-river delivers Chord{keysym: c, modifiers: meta} to sola-shell
   │
   ▼
sola-shell emits Topic::Copy(EditRequest { window_id: focused_window_id })
   │
   ▼
sola-app framework (each Sola app process receives the topic):
   - if window_id matches one of this app's own WindowHandles:
       send {event: "copy"} to that window's JS
   │
   ▼
app JS (e.g. terminal):
   - on("copy"): write current selection to navigator.clipboard
```

Paste is identical with `Topic::Paste` and `{event: "paste"}`.

When a **non-Sola** app is focused, Meta+C doesn't fire the chord at all — see §8. The bus topic is still emitted unconditionally when the chord fires, but no non-Sola client listens for it, so it's a silent no-op by construction (no is-Sola check needed in the shell).

## 5. Bus contract

### 5.1 New topics

| Topic | Payload | Emitted by | Consumed by |
|---|---|---|---|
| `Copy` | `EditRequest { window_id: u32 }` | sola-shell | sola-app framework |
| `Paste` | `EditRequest { window_id: u32 }` | sola-shell | sola-app framework |

```rust
pub struct EditRequest {
    pub window_id: u32,
}
```

Non-sticky; fires once per chord press. No response / confirmation topic.

### 5.2 Topics removed

None at the bus level. The per-app `SetAppMenu` / `MenuAction` flow stays exactly as it is; we're just removing specific menu *entries* (see §7).

## 6. Shell (`apps/shell`)

### 6.1 Chord registration

Add two hardcoded chords to `shell_key_chords()` (`apps/shell/src/app.rs` ~L311–337):

```rust
shell_chord(KeyCode::C, Modifier::Meta, ShellAction::Copy),
shell_chord(KeyCode::V, Modifier::Meta, ShellAction::Paste),
```

These register unconditionally, alongside the existing Super+Tab / Super+Space / Super+arrows set. They're published as part of the sticky `RegisteredChords` topic.

### 6.2 Chord handling

When `Topic::Chord(Chord { keysym, modifiers })` arrives and matches the Copy or Paste chord:

1. Read `self.focused_window_id` (already tracked — `apps/shell/src/app.rs:35`).
2. If `focused_window_id` is `None`: log at `debug` and return.
3. `ctx.emit(Topic::Copy(EditRequest { window_id }))` (or `Paste`). Done.

No is-Sola check. The shell emits the topic unconditionally. Non-Sola apps aren't bus clients, so the message has no recipient and no effect.

### 6.3 No is-Sola filter

An earlier draft of this spec had the shell consult a hardcoded `SOLA_APP_IDS` set to decide whether to emit. Dropped in favor of unconditional emission — sola-river makes its own is-Sola check on the receiving side (see §8), and each sola-app only acts on window_ids in its own `WindowHandle` list, so foreign windows are inherently ignored.

### 6.4 Removed: Edit menu generation

Shell does **not** synthesize an Edit menu for apps that lack one. Apps simply don't declare one (see §7). The shell's menubar still renders whatever menus apps do declare (File, View, Tabs, etc.).

## 7. App-level changes

### 7.1 `crates/sola-app` — framework intercept

In the bus event loop (`crates/sola-app/src/lib.rs`), add handling for `Topic::Copy` and `Topic::Paste` alongside the existing `Topic::Shutdown` handler:

```rust
Topic::Copy(req) => {
    if let Some(handle) = self.find_window_by_id(req.window_id, &ctx) {
        handle.send_to_js(&json!({ "event": "copy" }));
    }
}
Topic::Paste(req) => {
    if let Some(handle) = self.find_window_by_id(req.window_id, &ctx) {
        handle.send_to_js(&json!({ "event": "paste" }));
    }
}
```

`find_window_by_id` correlates `window_id` to a `WindowHandle` by consulting the sticky `Apps` topic (already cached somewhere reachable, or maintained by `sola-app`). Matching strategy: find the `App { window_id, app_id, title }` in `Apps`, compare `app_id` and `title` to the fields on this process's `WindowHandle`s.

For v1: most Sola apps are single-window. Single-window fast path: if the process has exactly one window *and* the `window_id`'s `app_id` equals this process's `app_id`, deliver to that window without consulting title. Multi-window correlation is future work (the existing `window_id_by_key` logic in shell already lives in one place — port or share that helper as needed).

Apps that want to customize copy/paste (terminal already does) register a JS `on("copy")` / `on("paste")` handler. Apps that don't care can optionally get "native" behavior by shipping a default handler in the platform JS lib that calls `navigator.clipboard.writeText(window.getSelection().toString())` and `navigator.clipboard.readText().then(t => document.execCommand("insertText", false, t))` respectively. Decision on the default handler: implement in `crates/sola-app/platform/` as part of this task, with apps free to override.

### 7.2 Terminal (`apps/terminal`)

- **Remove** the `Edit` menu definition in `terminal_menu()` (`apps/terminal/src/main.rs:185-203`).
- **Remove** the `"copy"` / `"paste"` arms in the `MenuAction` handler (~L121-129). That handler becomes leaner; no wiring change.
- **Keep** the existing JS-side `on("copy")` / `on("paste")` handlers in `apps/terminal/web/src/app.ts:215-233` — they already use `navigator.clipboard` and call xterm.js selection / paste. These continue to fire, but now triggered by the framework's new `Topic::Copy` / `Topic::Paste` dispatch instead of the old `MenuAction` path.

### 7.3 Other apps

Audit for any other Edit menus or Meta+C/V bindings in apps. Remove them in favor of the global path. Expected hits: zero beyond terminal (browser, monitor, switcher, launcher don't define one today), but verify during implementation.

## 8. Non-Sola app handling — virtual keyboard synthesis

For non-Sola apps (Brave, future native Wayland clients), Meta+C / Meta+V need to actually copy/paste. We synthesize real Ctrl+C / Ctrl+V keystrokes via `zwp_virtual_keyboard_unstable_v1`, targeting the focused seat. The non-Sola client receives genuine key events and responds with its own copy/paste handlers.

### 8.1 Mechanism

`sola-river` also subscribes to `Topic::Copy` / `Topic::Paste` (alongside each sola-app). When a message arrives:

1. Look up `window_id` in its `WindowRegistry` → get `app_id`.
2. If `app_id` starts with `"sola-"`: no-op. The owning sola-app process handles it.
3. Otherwise: emit a Ctrl+<key> keystroke on the virtual keyboard.

The synthesis uses explicit `modifiers()` requests rather than a LeftCtrl keycode press/release pair, both for robustness (no stuck-modifier failure mode) and to isolate our virtual keyboard state from the user's physical state on wlroots:

```
virtual_keyboard.modifiers(depressed=CTRL, latched=0, locked=0, group=0)
virtual_keyboard.key(time=T,   key=KEY_C, state=pressed)
virtual_keyboard.key(time=T+1, key=KEY_C, state=released)
virtual_keyboard.modifiers(depressed=0, latched=0, locked=0, group=0)
```

`KEY_C = 46`, `KEY_V = 47` — raw evdev keycodes from `linux/input-event-codes.h`. The compositor adds the +8 offset internally when resolving against the virtual keyboard's uploaded xkb keymap.

### 8.2 Virtual keyboard lifecycle

- `sola-river` binds `zwp_virtual_keyboard_manager_v1` (advertised by wlroots-based compositors by default).
- Once both the manager and `wl_seat` are bound, sola-river calls `create_virtual_keyboard(seat)` and uploads a standard us-layout xkb keymap via a sealed memfd. This is the same default keymap River uses for physical keyboards.
- The keyboard persists for the life of sola-river; there's no per-chord teardown.
- If River doesn't advertise the global (unusual config), sola-river logs a warning on the first synthesis attempt and the feature degrades to "Meta+C/V work only in sola apps."

### 8.3 What this does and doesn't provide

**Does provide:**
- Meta+C / Meta+V copy/paste inside Brave and any other foreign client that binds Ctrl+C / Ctrl+V.
- Symmetry — both directions use the same mechanism.
- No focus-change keymap churn; the keyboard layout is stable.
- Shell navigation chords (Super+Tab / Super+Space / Super+Numpad) continue to work regardless of which app is focused, because there's no keymap swap.

**Does not provide:**
- Meta+T = new tab, Meta+W = close tab, Meta+R = reload in foreign apps. Only the specific chords we synthesize for work. Adding more is a small incremental change: register the chord in the shell, add a synthesis entry in sola-river.
- Anything inside apps that don't bind Ctrl+C / Ctrl+V as copy/paste. In practice that's rare.

### 8.4 Custom exceptions

Not applicable at v1's scope. Every chord that reaches the synthesis path is explicitly enumerated on the sola-river side, so there's nothing to exclude.

### 8.5 Earlier alternative (rejected): focus-driven xkb remap

An earlier draft of this spec specified swapping xkb keymaps on focus transitions so that physical Meta→Ctrl inside non-Sola apps. That approach would have given Meta+T = new tab and friends "for free" but broke shell navigation chords (Super+Tab/Space/numpad arrived as Ctrl+Tab/Space/numpad and no longer matched registered chords). Remediations (register Ctrl+numpad variants, write a selective per-keysym xkb file) rapidly compounded. Abandoned in favor of the narrower, more surgical approach above.

## 9. Implementation checklist

1. Add `Topic::Copy(EditRequest)` and `Topic::Paste(EditRequest)` to `crates/sola-bus/src/topics.rs`. Update `apps/monitor/src/decode.rs` match.
2. Shell: add Meta+C and Meta+V to `shell_key_chords()`; handle them in the chord-handling code path by emitting `Topic::Copy` / `Topic::Paste` unconditionally with `focused_window_id`.
3. `crates/sola-app`: add `Topic::Copy` / `Topic::Paste` interception in the framework event loop; implement `find_window_by_id` helper (single-window fast path at minimum).
4. `crates/sola-app` platform JS: add default `copy` / `paste` handlers that use `navigator.clipboard` + `window.getSelection()` / `document.execCommand("insertText", ...)`. Apps can override.
5. Terminal: remove the Edit menu entries and the MenuAction `"copy"` / `"paste"` arms; leave JS-side handlers in place.
6. `sola-river`: add `zwp_virtual_keyboard_unstable_v1` protocol module; bind `zwp_virtual_keyboard_manager_v1`; create a per-seat virtual keyboard and upload a standard xkb keymap via sealed memfd.
7. `sola-river`: also subscribe to `Topic::Copy` / `Topic::Paste`; look up `window_id → app_id` in the existing `WindowRegistry`; if the app_id doesn't start with `"sola-"`, synthesize Ctrl+KEY_C / Ctrl+KEY_V via `modifiers(CTRL)` → `key(press)` → `key(release)` → `modifiers(0)`.
8. `cargo check --workspace`; `cargo make install`; verify:
   - Meta+C / Meta+V copy/paste inside the terminal (xterm.js selection).
   - Meta+C / Meta+V copy/paste inside a generic WebView app with a `<textarea>` (default handler path).
   - Meta+C in Brave triggers Brave's native copy (via synthesized Ctrl+C).
   - Meta+V in Brave triggers Brave's native paste.
   - Ctrl+C / Ctrl+V and Ctrl+Shift+C / Ctrl+Shift+V continue to work natively in all clients.
   - Meta+Tab / Meta+Space / Meta+Numpad chords continue to fire regardless of which app is focused.

## 10. Open questions

- **Default JS handler implementation.** `document.execCommand("insertText", ...)` is technically deprecated but still the only way to insert text at the caret in a way that respects undo history. Alternative: use `navigator.clipboard.readText()` and synthesize an `InputEvent`. Decide during implementation; revisit if platforms diverge.
- **Primary selection via Meta.** Out of scope. Ctrl+middle-click continues to work via River.
