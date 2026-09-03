# sola-mail — Design Spec

**Date:** 2026-04-20
**Branch:** `sola-mail`
**Scope:** Port the Cogsworth mail app (`apps/mail` + `crates/cogsworth-mail` in
`../Cogsworth`) to a Sola app that uses `sola-app` for the WebView host and
Arrow.js for the frontend.

## Goal

Produce `sola-mail`: a Wayland-native mail client with feature parity to the
Cogsworth original — IMAP list/fetch/move/search, SMTP send, IDLE push, move
rules, smart mailboxes, wicket alias fetcher — running as a single-window Sola
app with an Arrow.js frontend.

## Non-goals

- In-app settings UI for mail account configuration. The config file
  (`~/.config/sola/mail.toml`) is still edited by hand for v1; a future
  `sola-settings` integration will replace that.
- Multi-account support.
- Attachments (send or receive) beyond what Cogsworth already does.
- HTML compose. Plain-text only, matching Cogsworth.

## Template

The new app follows the `apps/agent` pattern (as of the `sola-agent` worktree):
single window, `AsyncDispatcher` + `AppHandler`, mpsc event bridge drained into
`send_raw_json_to_js` from a `glib::timeout_add_local`. Menu via sticky
`Topic::SetAppMenu`, quit via `Topic::MenuAction`.

## Structure

```
apps/mail/
  Cargo.toml
  src/
    main.rs       — SolaApp impl (MailApp), menu, startup auto-connect, event bridge
    handler.rs    — AppHandler impl (async command dispatch via AsyncDispatcher)
    state.rs      — MailState (client + config + idle/keepalive handles, event_tx)
    imap.rs       — IMAP client (port of cogsworth-mail/client.rs)
    idle.rs       — IDLE watcher (port of cogsworth-mail/idle.rs)
    sender.rs     — SMTP send (port of cogsworth-mail/sender.rs)
    wicket.rs     — alias-address fetcher (port of cogsworth-mail/wicket.rs)
    config.rs     — MailConfig load/save (~/.config/sola/mail.toml)
    rules.rs      — MailRule + MailRuleCondition + rule matching
                    (port of cogsworth-mail/types.rs)
    menu.rs       — mail_menu() AppMenuPayload
  web/
    index.html
    src/
      main.ts         — entry
      app.ts          — reactive state tree, wire-up, keyboard handlers
      types.ts        — Folder, MessageSummary, MessageBody, MailRule,
                        matchesRule / matchesAnySmartMailbox (pure fns)
      theme.css       — CSS vars + global styles (from Cogsworth Svelte <style>)
      components/
        folder-list.ts
        message-list.ts
        message-view.ts
        compose-view.ts
        toast.ts
```

The Cogsworth mail library is inlined under `apps/mail/src/` (not a separate
workspace crate). Only `sola-mail` consumes it and the previous split was
historical.

## Rust host

### Trait impl (`main.rs`)

```rust
struct MailApp {
    main_window: WindowHandle,
    dispatcher: AsyncDispatcher,
}

impl SolaApp for MailApp {
    const APP_ID: &'static str = "sola-mail";

    fn new(ctx: &mut AppCtx) -> Self { /* see below */ }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.on(TopicKind::MenuAction, Self::on_menu_action);
    }

    fn on_js_command(
        &mut self,
        cmd: &str,
        args: &Value,
        id: Option<u64>,
        _source: &WindowHandle,
        _ctx: &mut AppCtx,
    ) {
        let source = self.main_window.clone();
        let args = args.clone();
        self.dispatcher.dispatch(cmd.to_string(), args, move |result| {
            if let Some(id) = id {
                source.send_to_js(&json!({ "id": id, "result": result }));
            }
        });
    }
}
```

`on_menu_action` handles only `quit` in v1 (`std::process::exit(0)`).

### Window

Single `WindowConfig` with:

- `title: "main"`, `size: (1280, 820)`, `position: None`
- `decorated: false`, `transparent: false`
- `assets: APP_ASSETS`, `initial_state: None`
- `zoned: true`, `keyboard_target: true`

### Event bridge

```rust
let (event_tx, event_rx) = std::sync::mpsc::channel::<String>();
let mw_for_events = main_window.clone();
gtk4::glib::timeout_add_local(Duration::from_millis(5), move || {
    while let Ok(msg) = event_rx.try_recv() {
        mw_for_events.send_raw_json_to_js(&msg);
    }
    glib::ControlFlow::Continue
});
```

Push events are JSON strings shaped `{"event": "...", ...}` — matching the
frontend `on('event_name', cb)` convention.

### Handler state (`state.rs`)

```rust
pub struct MailState {
    pub client: tokio::sync::Mutex<Option<Arc<std::sync::Mutex<ImapClient>>>>,
    pub config: tokio::sync::RwLock<Option<MailConfig>>,
    pub idle_handle: tokio::sync::Mutex<Option<IdleHandle>>,
    pub idle_move_rules: Arc<std::sync::Mutex<Vec<MailRule>>>,
    pub keepalive_abort: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub event_tx: std::sync::mpsc::Sender<String>,
}
```

No `window_senders` map — events always go through `event_tx` and the single
main window.

### Handler commands (`handler.rs`)

`MailHandler` implements `sola_app::AppHandler`. Commands mirror Cogsworth:

| cmd                     | args                                           | return                                                    |
| ----------------------- | ---------------------------------------------- | --------------------------------------------------------- |
| `mail_connect`          | — (uses stored config)                         | `{folders, smart_counts, from_addresses, rules}`          |
| `mail_test_connection`  | `{imap_host, imap_port, username, password}`   | `{success, error?}`                                       |
| `mail_list_folders`     | —                                              | `{folders, smart_counts}`                                 |
| `mail_list_messages`    | `{folder, offset, limit}`                      | `{messages, total}`                                       |
| `mail_search`           | `{folder, query}`                              | `{messages, total}`                                       |
| `mail_fetch_body`       | `{folder, uid}`                                | `MessageBody`                                             |
| `mail_send`             | `{from, to, cc?, subject, body, in_reply_to?}` | `"ok"`                                                    |
| `mail_move`             | `{folder, uid, dest}`                          | `"ok"`                                                    |
| `mail_mark_read`        | `{folder, uid}`                                | `"ok"`                                                    |
| `mail_empty_folder`     | `{folder}`                                     | `"ok"`                                                    |
| `apply_rules`           | —                                              | `{moved: N}` and a pushed `mail:new` event if `moved > 0` |
| `open_url`              | `{url}`                                        | `"ok"` — emits `Topic::OpenUrl` on the bus                |

`open_url` takes an `mpsc::Sender<sola_bus::Message>` dispatch path or, more
directly, an `AppCtx` closure stashed on the handler. The simplest route:
handler holds a `Sender<Topic>` channel, main thread drains it and calls
`ctx.emit(Topic::OpenUrl(...))`. Pattern is established elsewhere — finalise
during implementation.

### Startup auto-connect

```rust
let dispatcher = AsyncDispatcher::spawn(MailHandler { state, event_tx });
// Kick off a connect in the background so the first mail_connect from the
// frontend is instant.
dispatcher.dispatch(
    "mail_connect".into(),
    json!({}),
    |_result| { /* ignore — UI will re-call on mount */ },
);
```

If config is absent or incomplete, `mail_connect` returns an error and the
frontend falls back to the disconnected state. Safe because the frontend calls
`mail_connect` itself on mount anyway.

### Menu

```rust
fn mail_menu() -> AppMenuPayload {
    AppMenuPayload {
        app_id: MailApp::APP_ID.into(),
        menus: vec![MenuDefinition {
            label: "Mail".into(),
            items: vec![MenuItem::Action {
                id: "quit".into(),
                label: "Quit Mail".into(),
                shortcut: Some(KeyCode::Q.meta()),
                disabled: false,
                checked: false,
            }],
        }],
    }
}
```

Compose / Reply / Archive / Delete menu items are deferred. All message-level
shortcuts (j/i/a/d/u/w/s) are handled by the frontend via `window.addEventListener('keydown', ...)`.

### Dependencies

Add to `apps/mail/Cargo.toml` (following agent's layout):

- `sola-app`, `sola-bus`, `sola-core`
- `gtk4 = "0.9"`
- `tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync"] }`
- `serde`, `serde_json`, `tracing`, `anyhow`, `async-trait`
- Mail-specific: `imap`, `rustls-connector`, `lettre`, `mail-parser`,
  `rustls = { version = "0.23", features = ["aws_lc_rs"] }`, `ureq`, `toml`,
  `base64`

Add to `[workspace.dependencies]` in root `Cargo.toml` any of these that are
not yet present, pinned to the versions Cogsworth uses.

### Rustls provider

`main` installs the aws_lc_rs crypto provider once before any TLS usage, same
as Cogsworth:

```rust
let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
```

## Frontend (Arrow.js)

Follows the agent/terminal/browser pattern. `@sola/ipc` and `@sola/store` are
the only framework imports; templates use `@arrow-js/core`.

### `index.html`

```html
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>sola-mail</title>
  <link rel="stylesheet" href="/src/theme.css">
</head>
<body>
  <div id="app"></div>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
```

### State tree (`app.ts`)

One `reactive()` object, array fields reassigned on change (proven pattern):

```ts
const state = reactive({
  folders: [] as Folder[],
  smartCounts: [] as Folder[],
  selectedFolder: 'INBOX',
  messages: [] as MessageSummary[],
  inboxMessages: [] as MessageSummary[],
  totalMessages: 0,
  selectedUid: null as number | null,
  messageBody: null as MessageBody | null,
  composing: false,
  replyTo: null as MessageBody | null,
  fromAddresses: [] as string[],
  rules: [] as MailRule[],
  loading: true,
  fatalError: null as string | null,
  toastError: null as string | null,
  searchQuery: '',
  searchActive: false,
  searchTotal: 0,
  isLoadingMore: false,
  bulkInProgress: false,
  folderLoading: false,
  lastMove: null as { uid: number; fromFolder: string; toFolder: string } | null,
});
```

`smartMailboxNames` and `isSmartMailbox` become plain derived getters
(`() => state.rules.filter(...)`). Arrow.js re-evaluates accessor closures in
templates automatically.

### Components

Each component is `createX(config, target: HTMLElement): void` — same shape as
`createSidebar`. Accessor closures for state, callbacks for actions:

```ts
createMessageList({
  messages: () => state.messages,
  selectedUid: () => state.selectedUid,
  hasMore: () => computeHasMore(),
  isLoadingMore: () => state.isLoadingMore,
  folderLoading: () => state.folderLoading,
  searchActive: () => state.searchActive,
  searchTotal: () => state.searchTotal,
  folderName: () => state.selectedFolder,
  isSmartMailbox: () => state.selectedFolder.startsWith('smart:'),
  isBulkOperating: () => state.bulkInProgress,
  onSelect: selectMessage,
  onSearch: searchMessages,
  onClearSearch: clearSearch,
  onLoadMore: loadMore,
  onArchiveAll: () => bulkMove('Archive'),
  onTrashAll: () => bulkMove('Trash'),
  onEmptyFolder: emptyFolder,
}, messageListTarget);
```

Internal UI state (search input value, rename input, drag position) uses a
local `reactive()` scoped to the component, same as `createSidebar` does.

### IPC

```ts
const { folders, smart_counts, from_addresses, rules } =
  await invoke('mail_connect');
state.folders = folders;
state.smartCounts = smart_counts ?? [];
state.fromAddresses = from_addresses;
state.rules = rules ?? [];

on('mail:new', () => handleNewMail());
```

### Keyboard shortcuts

Window-level `keydown` listener in `app.ts` handles j / i / a / d / u / w / s
(Junk / Inbox / Archive / Trash / Undo / prev / next). Bails when:

- `state.composing` is true, or
- target is `INPUT` or `TEXTAREA`, or
- Ctrl / Alt / Meta is held.

Reply / Compose are triggered by buttons in `MessageView` / `MessageList` —
no chord shortcuts in v1 (they'll move to menu items later).

### Styles

Port Svelte `<style>` blocks to plain CSS. A single `theme.css` holds CSS
custom properties and shared rules; per-component selectors go in the same
file. If it gets unwieldy, split by component (`folder-list.css`, etc.)
during implementation.

### Message HTML rendering

Cogsworth's `MessageView` renders the message HTML inline. Port verbatim,
including its link-interception (anchor clicks call `invoke('open_url', ...)`
instead of following navigation). The Rust handler maps that to
`Topic::OpenUrl`, which `sola-browser` picks up.

## Config

`~/.config/sola/mail.toml`. Same schema as Cogsworth:

```toml
[account]
email = "..."
imap_host = "..."
imap_port = 993
smtp_host = "..."
smtp_port = 587
username = "..."
password = "..."

[[rule]]
name = "GitHub"
action = "smart_mailbox"
conditions = [{ field = "from", match = "domain", value = "github.com" }]

[[rule]]
name = "Move newsletters"
action = "move"
dest = "Newsletters"
conditions = [{ field = "from", match = "contains", value = "newsletter" }]
```

`MailConfig::config_path()` returns
`${XDG_CONFIG_HOME:-~/.config}/sola/mail.toml`. Missing file → default config
(empty fields). Parse errors → startup warning, disconnected UI.

Cogsworth had test coverage in `config.rs`; port those tests unchanged.

## Wicket (alias fetcher)

`wicket.rs` ports verbatim. `handle_mail_connect` calls `wicket::fetch_from_addresses(host, user, pass)` via `tokio::task::spawn_blocking` — if it returns an empty vec, falls back to `[config.email]`. Unchanged from Cogsworth.

## Build / deploy

- `cargo make build mail` and `cargo make install mail` (short-name
  convention).
- Register the new app in `sola-make` if the crate requires explicit listing.
  Confirm during step 1 of the plan by reading `crates/sola-make/src`.

## Testing

Unit tests:

- Port all `config.rs` tests (parse, serialize, round-trip, legacy `[wicket]`).
- Port `rules.rs` matching tests (domain/address/contains/equals across
  `from` / `subject`).
- IMAP client and IDLE have no unit coverage in Cogsworth; skip.

Manual smoke test on a TTY (required before claiming done):

1. Launch sola-mail with valid `mail.toml`.
2. Verify: startup spinner → folder list + INBOX messages.
3. Click a message → body renders; auto-marks read; folder counts refresh.
4. Compose → send → message appears in `Sent`.
5. `d` → message moves to Trash; selection advances; `u` restores.
6. Receive new mail (IDLE) → toast-free refresh, folder counts tick.
7. Search → filtered list; Escape → restored.
8. Smart mailbox → shows only rule-matched messages.
9. Click an http(s) link in a message → opens in sola-browser.
10. Cmd+Q → process exits cleanly.

## Build sequence

The implementation plan will expand on this; these are the milestones.

1. Skeleton `apps/mail` (Cargo.toml, main.rs SolaApp stub, empty handler,
   placeholder `ready` command, empty index.html). `cargo check` passes.
2. Port `config.rs` + `rules.rs` (plus tests). `cargo check + test` passes.
3. Port `imap.rs`, `idle.rs`, `sender.rs`, `wicket.rs`.
4. Wire commands in `handler.rs`; state struct; startup auto-connect; IDLE
   event bridge. `cargo check` passes.
5. Menu (`Quit Mail` only); `on_menu_action`.
6. Frontend skeleton: `index.html`, `main.ts`, `app.ts` with state + connect
   flow + loading/error UI. Smoke test on a TTY: connects, shows folder list.
7. `folder-list.ts` component. Verify folder selection.
8. `message-list.ts` component (search, infinite scroll, bulk actions).
9. `message-view.ts` component (body render, reply / delete / compose buttons,
   link interception).
10. `compose-view.ts` component (reply-to prefill, from-address dropdown,
    send).
11. `toast.ts` component; keyboard shortcuts; IDLE `mail:new` handler.
12. Styling pass. Feature-complete smoke test (section above).
13. Install locally.

## File-by-file Cogsworth → Sola map

| Cogsworth                                         | Sola                                |
| ------------------------------------------------- | ----------------------------------- |
| `apps/mail/src/main.rs`                           | `apps/mail/src/main.rs`             |
| `apps/mail/src/handler.rs`                        | `apps/mail/src/handler.rs`          |
| `apps/mail/src/state.rs`                          | `apps/mail/src/state.rs`            |
| `apps/mail/src/mail_bridge.rs`                    | merged into `idle.rs` + `handler.rs` |
| `crates/cogsworth-mail/src/client.rs`             | `apps/mail/src/imap.rs`             |
| `crates/cogsworth-mail/src/idle.rs`               | `apps/mail/src/idle.rs`             |
| `crates/cogsworth-mail/src/sender.rs`             | `apps/mail/src/sender.rs`           |
| `crates/cogsworth-mail/src/config.rs`             | `apps/mail/src/config.rs`           |
| `crates/cogsworth-mail/src/types.rs`              | `apps/mail/src/rules.rs`            |
| `crates/cogsworth-mail/src/wicket.rs`             | `apps/mail/src/wicket.rs`           |
| `apps/mail/frontend/src/App.svelte`               | absorbed into `web/src/main.ts`     |
| `apps/mail/frontend/src/lib/MailWindow.svelte`    | `web/src/app.ts`                    |
| `apps/mail/frontend/src/lib/mail/FolderList.svelte`   | `web/src/components/folder-list.ts`  |
| `apps/mail/frontend/src/lib/mail/MessageList.svelte`  | `web/src/components/message-list.ts` |
| `apps/mail/frontend/src/lib/mail/MessageView.svelte`  | `web/src/components/message-view.ts` |
| `apps/mail/frontend/src/lib/mail/ComposeView.svelte`  | `web/src/components/compose-view.ts` |
| `apps/mail/frontend/src/lib/mail/types.ts`        | `web/src/types.ts`                  |

## Open items to resolve during implementation

- `open_url` command → `Topic::OpenUrl` plumbing: pick the clean way to emit a
  bus topic from a dispatcher-side async handler. Likely: handler holds a
  `std::sync::mpsc::Sender<Topic>`; main drains it in the same glib timeout
  that drains `event_rx` and calls `ctx.emit(...)`.
- `sola-make` registration for the new app — confirm whether apps are
  auto-discovered from `apps/*` or need explicit listing.
- Exact version pinning for `imap` / `lettre` / `mail-parser` / `rustls` in
  `[workspace.dependencies]`.
