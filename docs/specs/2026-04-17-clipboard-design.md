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
- Filtering by app identity in the shell. See §6.3.

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

An earlier draft of this spec had the shell consult a hardcoded `SOLA_APP_IDS` set to decide whether to emit. Dropped in favor of unconditional emission:

- Non-Sola apps don't listen on the bus — a stray `Copy(window_id=42)` where window 42 is Brave is received by nobody. Zero cost.
- The shell never has to track which `app_id`s are Sola vs. foreign — the bus's subscribe model handles it for us.
- (Separately, the focus-driven xkb remap in §8 means Meta+C doesn't even fire the chord when non-Sola is focused — so in practice the bus emission only happens with a Sola app focused anyway. But we don't rely on that invariant; the no-filter story holds either way.)

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

## 8. Non-Sola app handling — focus-driven xkb remap

For non-Sola apps (Brave, future native Wayland clients), we want Meta+C / Meta+V (and more generally Meta+letter) to behave the way Ctrl+C / Ctrl+V / Ctrl+letter does — Meta-as-Cmd, Mac-style. Applied universally inside non-Sola apps this also gives Meta+T = new tab, Meta+W = close tab, Meta+R = reload, etc. for free.

### 8.1 Mechanism

Shell watches focus transitions (it already has `focused_app_id` for this). When focus enters a non-Sola window, shell pushes an xkb configuration to River via `river-xkb-config-v1` where the Meta (Super/Mod4) key is rebound to Ctrl. When focus returns to a Sola window, shell restores the default keymap.

Effect chain:
- Physical Meta+C pressed while Brave is focused.
- River's keymap evaluation (using the remap) produces keysym `c` with modifier `Ctrl`, not `Meta`.
- The shell's chord registration for Meta+C doesn't match (it expected modifier `Meta`), so no chord fires.
- River routes Ctrl+C to the focused client (Brave), which copies as usual.

Symmetrically, Super+Tab / Super+Space / Super+arrows — all currently shell chords — *also* arrive as Ctrl+... under the remap and don't fire their registered chords. That's a *known tradeoff*, discussed below.

### 8.2 Known tradeoff: shell navigation chords break under remap

While focused on a non-Sola app, the usual "escape to Sola" chords (Super+Tab, Super+Space) don't fire, because the keymap swap transforms them into Ctrl+Tab / Ctrl+Space before they reach the chord layer. The user ends up having to click on a Sola panel / menubar / another window to get focus back.

For v1 this is acceptable — the user is already on a foreign app by choice, and focus return via mouse is a single motion. If it proves annoying in practice, two refinements are available:

1. **Selective remap** — write a custom xkb layout that remaps Meta only for alphabetic keysyms (a–z, 0–9, and printable symbols), leaving Meta+Tab / Meta+Space / Meta+arrows on Meta. This requires a hand-authored xkb variant file shipped with sola-assets. Not hard, just extra work; defer until the tradeoff is felt.
2. **Reserved return chord** — register a non-Meta chord (e.g. Ctrl+Alt+Escape) as a "return to Sola" action that always fires regardless of keymap state.

### 8.3 Custom exceptions

Some Meta+letter keys arguably shouldn't Meta→Ctrl translate even inside non-Sola apps — e.g. if we ever want Meta+Q to do something Sola-level (quit session) globally, it shouldn't become Ctrl+Q (quit Brave). v1 doesn't ship any such exceptions; add them as the selective-remap file from §8.2 (1) if needed.

### 8.4 Alternative: `zwp_virtual_keyboard_v1` synthesis

If the xkb-remap approach in §8.1 ends up not working out (e.g. `river-xkb-config-v1` isn't expressive enough, or modifier-state tracking under focus churn proves unreliable), an alternative is available: on each Meta+C / Meta+V with non-Sola focus, the shell sends a synthetic Ctrl+C / Ctrl+V via `zwp_virtual_keyboard_manager_v1` to the focused seat. The non-Sola app receives a real Ctrl+C / Ctrl+V.

Tradeoffs: symmetric and no focus-change keymap churn, but requires careful modifier-state hygiene (release physical Meta in the virtual keyboard state before emitting Ctrl, restore on completion) and only solves C/V — *doesn't* give the free Meta+T / Meta+W / Meta+R benefit the xkb approach provides.

Documented here so we can switch approaches without re-designing. Not implemented in v1.

## 9. Implementation checklist

1. Add `Topic::Copy(EditRequest)` and `Topic::Paste(EditRequest)` to `crates/sola-bus/src/topics.rs`. Update `apps/monitor/src/decode.rs` match.
2. Shell: add Meta+C and Meta+V to `shell_key_chords()`; handle them in the chord-handling code path by emitting `Topic::Copy` / `Topic::Paste` unconditionally with `focused_window_id`.
3. `crates/sola-app`: add `Topic::Copy` / `Topic::Paste` interception in the framework event loop; implement `find_window_by_id` helper (single-window fast path at minimum).
4. `crates/sola-app` platform JS: add default `copy` / `paste` handlers that use `navigator.clipboard` + `window.getSelection()` / `document.execCommand("insertText", ...)`. Apps can override.
5. Terminal: remove the Edit menu entries and the MenuAction `"copy"` / `"paste"` arms; leave JS-side handlers in place.
6. `sola-river`: bind `river_xkb_config_manager_v1` (or whatever the actual crate name resolves to) and expose a method on the bus-side translator for "push this xkb config now" driven by a new bus topic from shell. Topic shape TBD in the implementation plan — simplest is `Topic::XkbProfile(String)` with profile names like `"default"` and `"meta-as-ctrl"`, keymaps stored in sola-assets.
7. Shell: on focus transitions, emit the appropriate `XkbProfile` topic (`"meta-as-ctrl"` when the focused window's app_id does not start with `"sola-"`; `"default"` otherwise). The `"sola-"` prefix is the Sola-app convention.
8. `cargo check --workspace`; deploy to canto; verify:
   - Meta+C / Meta+V copy/paste inside the terminal (xterm.js selection).
   - Meta+C / Meta+V copy/paste inside a generic WebView app with a `<textarea>` (default handler path).
   - Meta+C in Brave triggers Brave's native copy (via the remap).
   - Meta+T in Brave opens a new tab (via the remap).
   - Ctrl+C / Ctrl+V and Ctrl+Shift+C / Ctrl+Shift+V continue to work natively in all clients.
   - Moving focus back to a Sola app restores Meta+Tab / Meta+Space chord behavior.

## 10. Open questions

- **Default JS handler implementation.** `document.execCommand("insertText", ...)` is technically deprecated but still the only way to insert text at the caret in a way that respects undo history. Alternative: use `navigator.clipboard.readText()` and synthesize an `InputEvent`. Decide during implementation; revisit if platforms diverge.
- **Primary selection via Meta.** Out of scope. Ctrl+middle-click continues to work via River.
