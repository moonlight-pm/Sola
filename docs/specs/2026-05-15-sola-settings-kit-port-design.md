# sola-settings → sola-kit port (design)

**Status:** approved (brainstorm). Next: implementation plan.
**Date:** 2026-05-15

## Goal

Replace `sola-settings`'s GTK4/WebKit6 stack with `sola-kit` (CEF/Remix
v3) in place, and rework the UI flow to adopt the kit's primitives
end-to-end. Use the port as an opportunity to evolve `sola-kit` itself
where the settings UI surfaces a missing piece.

## Decisions taken in brainstorm

- **Replace in place.** No parallel crate; the legacy stack inside
  `crates/sola-settings/` is gone in the same change.
- **No worktree.** Per user instruction for this task — work directly
  on `master`. (General project rule remains worktree-first; this is a
  one-off override.)
- **Scope:** approach C — feature parity is the floor, but the UI flow
  is redesigned to adopt kit idioms. Extend the kit as needed.
- **Bus contract:** unchanged with one addition (`mail_update_rule`).
  The existing `Application` / `MailConfig` / `Windows` sticky topics
  and per-handler `send_state_event` push remain authoritative.
- **No `initial_state`.** The kit's IPC layer already orphan-buffers
  events delivered before a listener registers (see
  `web/lib/ipc.ts:184–222`); sticky `state` pushes drain into
  `on('state', …)` on first registration. This neutralises the only
  hard dependency the legacy app had on synchronous HTML bootstrap.

## Crate layout

```
crates/sola-settings/
├─ Cargo.toml          # sola-kit + sola-bus + sola-core (drop sola-app + gtk4)
├─ src/
│  ├─ main.rs          # short-circuit subprocess + sola_kit::run::<SettingsApp>
│  ├─ app.rs           # SettingsApp (impl SolaApp), bus handlers, JS dispatch
│  └─ procfs.rs        # suggest_command + resolve_*; lifted from current main.rs
└─ web/
   ├─ main.tsx         # Main component; section state; <Root><Split>…</Split></Root>
   └─ panels/
      ├─ applications.tsx
      └─ mail.tsx
```

`main.rs` mirrors the kit's own `app/main.rs`: subprocess gate +
`sola_kit::run`. `app.rs` mirrors `app/app.rs`: state struct,
`KitApp`/`SolaApp` impl, `register_bus`, `on_js_command`, per-topic
handlers, `push_state` helper, optional `kit_menu` builder.

`procfs.rs` carries the cluster currently at the bottom of
`main.rs`: `suggest_command`, `resolve_from_app_id`,
`resolve_binary_for_pid`, `is_multi_arg_launcher`,
`cmdline_positional`, `is_system_app`. ~160 lines, self-contained,
unrelated to bus/UI plumbing — separating it removes the bulk of the
"why is this file this long" smell.

## Rust side — `SettingsApp`

```rust
pub struct SettingsApp {
    applications: ApplicationsConfig,
    mail: MailConfig,
    main_window: WindowHandle,
    running: Vec<BusWindow>,
}

impl SolaApp for SettingsApp {
    const APP_ID: &'static str = "sola-settings";

    fn new(ctx: &mut AppCtx) -> Self { … }       // one window, no initial_state
    fn register_bus(&mut self, bus, ctx) { … }   // CloseApp, Windows, MenuAction,
                                                 // MailConfig, Application
    fn on_js_command(&mut self, cmd, args, id, source, ctx) { … }
}
```

JS commands (all return the canonical state payload, or `{ error }`):

- `applications_add { app_id, label, command, icon }`
- `applications_update { old_app_id, app_id, label, command, icon }`
- `applications_remove { app_id }`
- `mail_save_account { email, imap_host, imap_port, smtp_host, smtp_port, username, password }`
- `mail_add_rule { name, action, dest?, conditions[] }`
- **`mail_update_rule { index, name, action, dest?, conditions[] }`** — **new.**
  The current API forces delete+add to edit a rule, which doesn't survive
  edit-in-place UX. Implementation: replace `self.mail.rules[index]` after
  validation, then `Topic::MailConfig(…)`.
- `mail_remove_rule { index }`

State pushes: every bus handler that mutates app state ends in
`push_state()`. `push_state()` serializes via the existing
`state_payload`/`mail_for_js` helpers and `send_to_js`'s a single
`{ event: "state", applications, mail }` payload.

## Window

```rust
ctx.add_window(WindowConfig {
    title: "Settings".into(),
    size: (900, 620),       // current is 760×560; widen for the Split sidebar
    position: None,
    decorated: false,
    transparent: false,
    assets: APP_ASSETS,
    zoned: true,
    keyboard_target: true,
});
```

App menu: same `Quit` action (`KeyCode::Q.meta()`) the current settings
ships, plus an Edit menu mirroring the kit's showcase (Cut / Copy /
Paste / Select All wired to `WindowHandle::{cut, copy, paste,
select_all}`) — the kit gives this for free now.

## JS side — component tree

```
<Main>
  <Root>
    <Split direction="row" position="240px">
      <Sidebar>
        <SidebarSection title="Settings">
          <SidebarItem active={section === "apps"}  onSelect={() => setSection("apps")}>
            Applications
          </SidebarItem>
          <SidebarItem active={section === "mail"}  onSelect={() => setSection("mail")}>
            Mail
          </SidebarItem>
        </SidebarSection>
      </Sidebar>
      <Container maxWidth="article">
        {section === "apps" ? <ApplicationsPanel … /> : <MailPanel … />}
      </Container>
    </Split>
  </Root>
</Main>
```

Sidebar position width is the default the kit picks; can be overridden
by the user via the Split divider. Container uses the `article` (880 px)
preset for readable form widths.

### State store

A single module-scope object in `main.tsx`:

```ts
interface SettingsState {
  apps: Application[];
  missing: string[];
  candidates: Candidate[];
  mail: MailConfig;
}
```

Updated only via `on('state', applyState)`. Each panel takes the
relevant slice as props and owns its own *draft* state in closures.

### ApplicationsPanel

Stack of two Cards:

1. **"Configured"** — Stack of app rows. Each row is editable in place:
   four `TextInput`s for `label` / `app_id` / `command` / `icon`,
   inline `Badge kind="warning"` next to the label when the app is in
   `missing`, and a `Button kind="danger"` with confirm semantics for
   remove.
   Edits commit on blur via debounced `applications_update`. Errors from
   the Rust side render inline as a Field-style `error` slot under the
   offending field; the row's `prev` snapshot rolls back on confirmed
   error.
   `+ Add application` button at the bottom appends a fresh blank row
   that is "draft-only" (not yet sent to Rust); first non-empty blur
   that passes validation calls `applications_add`.

2. **"Running, not configured"** — Stack of candidate rows. Each row
   shows `app_id`, `title`, and `suggested_command` (or "command
   unknown" Text muted style). One `Button` per row labeled "Configure"
   that calls back into the Configured card to insert a pre-filled
   draft row at the top, scrolls it into view, and focuses `label`.

### MailPanel

Stack of two Cards:

1. **"Account"** — Field-wrapped TextInputs for email, IMAP host,
   IMAP port (NumberInput), SMTP host, SMTP port (NumberInput),
   username, password (TextInput with new `secret` prop). Footer row:
   `Button kind="primary"` "Save" + `Button` "Revert", both disabled
   when the draft equals the canonical mail state. Save calls
   `mail_save_account`. Explicit save (not autosave) because passwords.

2. **"Rules"** — Stack of rule cards plus a `+ Add rule` button.
   Each rule card has:
   - `TextInput` for `name`
   - `PopoverSelect` for `action` (smart_mailbox / move)
   - `TextInput` for `dest` (only when action === "move")
   - Stack of condition rows. Each condition row: `PopoverSelect` for
     `field` (from / to / subject), `PopoverSelect` for `match`
     (contains / equals / address / domain), `TextInput` for `value`,
     remove button (confirm). Last row in the stack is a `+ Add condition`
     button.
   - Footer: Save + Discard buttons, enabled only when draft ≠ saved.
     Save calls `mail_update_rule` (for existing rules) or
     `mail_add_rule` (for the freshly-added draft).
   - Top-right corner: `Button kind="danger"` with confirm for "Remove rule".

### Drafts vs. canonical state

Each editable row holds a local draft initialized from the canonical
slice. When `state` event arrives:

- If draft is clean (draft === canonical), draft re-syncs to the new
  canonical.
- If draft is dirty (user mid-edit), draft is preserved; an
  unobtrusive "remote update available" hint appears at the row
  level. (Settings rarely sees remote updates — this is defense in
  depth.)

For the `applications_update` debounce: 500 ms after last keystroke,
issue the command; if it fails, roll back the row's `prev` snapshot
and surface the error inline.

## Kit additions

Three small kit changes, kept independent so each is reviewable on its
own.

### 1. `TextInput`: `secret` prop

Add a `secret?: boolean` prop on `TextInputProps`. When true, render
`<input type="password">`; otherwise `<input type="text">`. No CSS
changes; no token additions. Used by the Mail password field.

### 2. `Badge` component

New kit component:
`crates/sola-kit/web/lib/components/badge.{tsx,css}` plus
`crates/sola-kit/src/components/badge.rs`.

```tsx
interface BadgeProps {
  kind?: "neutral" | "info" | "success" | "warning" | "danger";
  children?: RemixNode;
}
```

Renders a small pill: text + tinted background. Slots:
`--sola-badge-{kind}-bg`, `--sola-badge-{kind}-text`,
`--sola-badge-radius`, `--sola-badge-padding-block`,
`--sola-badge-padding-inline`, `--sola-badge-text-size`. Default
`kind="neutral"`. Wire into `components/mod.rs::all_bindings`, the
asset bundle, and `build_importmap`. Add to `categories.rs` so the
showcase's bindings editor can theme it.

Used by:
- Configured app "not found" indicator (`kind="warning"`)
- Anywhere status display is needed (future)

### 3. Confirmation pattern on `Button`

User-preferred: extend `Button` rather than introduce a new component.
Add a `confirm?: boolean` prop (default `false`). When set:

- Click 1 → button label swaps to "Click again to confirm" (or
  `confirmLabel?: string` for override) and re-styles itself
  `kind="danger"` regardless of its idle kind.
- Click 2 within 2000 ms → fires `onClick`.
- 2 s of inactivity → resets to idle without firing.
- Component-internal timer; no kit-level dependency.

If the final API turns out to be ≥3 props or needs a state machine
that doesn't fit cleanly on Button, fall back to a dedicated
`ConfirmButton` component — but the bias is to fit on Button. Decided
during implementation.

## Migration steps (summary; full step-by-step plan comes next)

1. **Kit primitives in place first.** Land `TextInput secret`,
   `Badge`, and the `Button confirm` flow as their own commits — each
   exercises the kit showcase to validate.
2. **Rewrite `crates/sola-settings/Cargo.toml`.** Drop `sola-app` +
   `gtk4`, add `sola-kit`.
3. **Delete `web/{index.html, src/main.ts, src/app.ts, src/theme.css}`.**
4. **Add `web/main.tsx` + `web/panels/{applications,mail}.tsx`.**
5. **Replace `src/main.rs`.** Split into `main.rs` (subprocess gate +
   run) + `app.rs` (SettingsApp impl) + `procfs.rs` (`/proc` helpers).
6. **Add `mail_update_rule` to the JS-command dispatch.**
7. **Build verification** (`cargo make build`).

The user installs and smoke-tests; no `cargo make install` from the
assistant per project rule.

## Testing

- Build passes (`cargo make build`).
- `sola-kit` unit tests still pass (theme snapshot updates to include
  badge bindings).
- Manual smoke: user launches `/opt/sola/bin/sola-settings`,
  exercises Applications (add, edit, remove, "Configure" from
  candidate) and Mail (account save/revert, rule add/edit/remove,
  conditions).

No new automated tests for the UI — consistent with the rest of the
kit's app-level surface.

## Risks

- **`mail_update_rule` validation parity with `mail_add_rule`.**
  Both must run identical validation (name non-empty, at least one
  condition, `dest` non-empty when `action === "move"`). Centralize in
  a private fn rather than duplicating.
- **Edit-in-place + remote state pushes.** Theoretically the bus could
  re-broadcast `Application` mid-edit and clobber the user's draft.
  Mitigation: per-row "dirty" flag, draft preservation as described
  above. Realistically rare for settings.
- **Password field cleartext over IPC.** Same as today — the existing
  flow round-trips the password to JS via `mail_for_js`. No regression.
  Long-term hardening (reveal-on-demand, masked-by-default in the
  state push) is out of scope for this port.

## Out of scope

- Theme editing UI (lives in the kit's own showcase).
- Password reveal toggle.
- Live validation of IMAP/SMTP credentials.
- Auto-discovery of mail server settings from email domain.
- Application icon picker UI (still a free-text field).
