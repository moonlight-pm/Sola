# sola-monitor kit port — design

Port `sola-monitor` from the legacy GTK4/WebKit (`sola-app`) stack to
the CEF/Remix v3 (`sola-kit`) stack. Same functionality, kit
primitives, and one piece of legacy debt resolved (the
sidebar-width-persistence TODO).

## Scope

In scope:

- Replace `sola-app` + `gtk4` deps with `sola-kit` + `include_dir`.
- Rewrite the web frontend on Remix v3 + `@sola/*` kit components.
- Add `Topic::MonitorConfig` (sticky) so sidebar width survives restart.
- Wire a new `monitor_set_sidebar_width` JS command.

Out of scope:

- Virtualization of the messages list (5000-row cap is retained as-is).
- JSON tree viewer (the flat-token highlighter from legacy is ported verbatim).
- Per-topic checkbox filter (single PopoverSelect dropdown is retained).
- Replay / scrubbing controls.
- Detail-pane / drawer UX (per user choice, the inline row-expansion
  preview behavior is preserved).

## Crate layout

```
crates/sola-monitor/
  Cargo.toml          # sola-app + gtk4  →  sola-kit + include_dir
  src/
    main.rs           # subprocess gate + run::<MonitorApp>()
    app.rs            # MonitorApp impl (bus handlers, JS commands)
    decode.rs         # unchanged — message_to_json + decode_payload
  web/
    main.tsx          # Main: Root → Stack(toolbar, Split(messages, sticky))
    panels/
      messages.tsx    # toolbar + filtered list + selection + inline expansion
      sticky.tsx      # right-side sticky-state list with click-to-expand
    lib/
      categories.ts   # TOPIC_CATEGORIES map + categoryOf()
      json-tokens.tsx # syntax highlighter (port of tokenizeJson)
```

## sola-bus addition — `Topic::MonitorConfig`

Mirrors `MailConfig` / `TerminalConfig` / `BrowserConfig`. Sticky-replayed.

```rust
// crates/sola-bus/src/topics.rs (additions)
pub struct MonitorConfig {
    pub sidebar_width: u32,
}
impl Default for MonitorConfig {
    fn default() -> Self {
        Self { sidebar_width: 240 }
    }
}

// Topic enum (in the appropriate sticky group):
MonitorConfig(MonitorConfig),
```

Owner: `sola-monitor` itself. Reads on startup (sticky replay seeds the
local `MonitorConfig`); emits on resize commit. No external writers.

## Rust app

```rust
// crates/sola-monitor/src/app.rs

struct MonitorApp {
    main_window: WindowHandle,
    config: MonitorConfig,
}

impl SolaApp for MonitorApp {
    const APP_ID: &'static str = "sola-monitor";

    fn new(ctx: &mut AppCtx) -> Self {
        let main_window = ctx.add_window(WindowConfig {
            title: "main".into(),
            size: (900, 600),
            // …other kit defaults…
        });

        // App menu (unchanged from legacy): "Monitor → Quit Monitor (Cmd+Q)".
        ctx.emit(Topic::SetAppMenu(/* … */));

        Self { main_window, config: MonitorConfig::default() }
    }

    fn register_bus(&mut self, bus: &mut BusRegistry<Self>, _ctx: &mut AppCtx) {
        bus.subscribe_all();                              // firehose for the audit table
        bus.on(TopicKind::MonitorConfig, Self::on_config);
        bus.on(TopicKind::MenuAction,    Self::on_menu_action);
    }

    fn on_raw_bus_message(&mut self, msg: &Message, _ctx: &mut AppCtx) {
        let event = decode::message_to_json(msg);
        self.main_window.send_to_js(&event);
    }

    fn on_js_command(
        &mut self, cmd: &str, args: &Value, _id: Option<u64>,
        _source: &WindowHandle, ctx: &mut AppCtx,
    ) {
        if cmd == "monitor_set_sidebar_width" {
            if let Some(w) = args.get("width").and_then(|v| v.as_u64()) {
                self.config.sidebar_width = w as u32;
                ctx.emit(Topic::MonitorConfig(self.config.clone()));
            }
        }
    }
}

impl MonitorApp {
    fn on_config(&mut self, d: &Delivery, ctx: &mut AppCtx) {
        if let Topic::MonitorConfig(cfg) = &d.topic {
            self.config = cfg.clone();
            // Push fresh state to JS so the Split picks up the restored width.
            self.main_window.send_to_js(&json!({
                "event": "state",
                "sidebar_width": cfg.sidebar_width,
            }));
        }
    }

    fn on_menu_action(&mut self, d: &Delivery, _ctx: &mut AppCtx) {
        if let Topic::MenuAction(p) = &d.topic
            && p.app_id == Self::APP_ID
            && p.action_id == "quit"
        {
            std::process::exit(0);
        }
    }
}
```

Sticky replay drives `on_config` once at startup, before any user
interaction, so the Split mounts with the restored width without needing
the legacy `initial_state` mechanism.

## Web layout

```tsx
// web/main.tsx
<Root>
  <Stack gap="0" fill>
    {/* Toolbar */}
    <Stack direction="row" gap="md" align="center" style="padding: var(--space-sm) var(--space-md); border-bottom: 1px solid var(--border-subtle)">
      <TextInput placeholder="Filter messages…" onInput={(v) => { state.filter = v; applyFilter(); handle.update(); }} />
      <PopoverSelect value={state.topicFilter} placeholder="All topics" options={knownTopics()}
                     onChange={(v) => { state.topicFilter = v; applyFilter(); handle.update(); }} />
      <Button variant={state.paused ? "primary" : "ghost"} onPress={togglePause}>
        {state.paused ? `Resume (${pauseBuffer.length})` : "Pause"}
      </Button>
      <Button variant="ghost" onPress={clearMessages}>Clear</Button>
      <div style="flex: 1"/>
      <Text tone="muted" kind="caption">{state.count} msgs</Text>
    </Stack>

    {/* Body */}
    <Split direction="row" rightWidth={state.sidebarWidth}
           onResizeCommit={(w) => invoke("monitor_set_sidebar_width", { width: w })}>
      <MessagesPanel state={state} />
      <StickyPanel   state={state} />
    </Split>
  </Stack>

  {!state.autoScroll
    ? <Button variant="ghost" onPress={jumpToBottom} class="autoscroll-pill">
        ↓ Auto-scroll paused — click to resume
      </Button>
    : null}
</Root>
```

### MessagesPanel

Fixed table-header row (Time, Topic, Source, S, Preview) plus a
scrollable body of `.message-row` divs. Selected row's preview cell
swaps to highlighted JSON inline (matches legacy).

### StickyPanel

Renders one entry per `(topic, source)` in `state.stickyMessages`.
Each entry is a click-to-expand card showing the topic + source, with
the pretty-printed payload revealed inline when expanded.

## Bespoke CSS (retained, slimmed)

Keep three small CSS files (auto-injected via `platform_assets()` per
kit convention) — everything else inherits from theme tokens:

- **Messages table grid** — `grid-template-columns: 88px 200px 120px 24px 1fr` for
  the row layout; selected-row preview expansion.
- **Category color stripe** — `border-left: 2px solid <category-color>` per
  `data-category`. Category-to-color map matches legacy.
- **JSON token colors** — `.token-string`, `.token-number`, `.token-key`, etc.,
  using theme atoms (`--accent`, `--success`, `--text-primary`).

The autoscroll pill is positioned `absolute` over the bottom of
MessagesPanel via a small selector in the messages CSS.

## JS commands

| Command                       | Args            | Effect                                     |
|-------------------------------|-----------------|--------------------------------------------|
| `monitor_set_sidebar_width`   | `{ width: u32 }` | Updates `config.sidebar_width`, emits sticky `Topic::MonitorConfig`. |

## Kit additions

Verify during implementation; pull only what we actually need:

- **`Split.onResizeCommit`** — if the existing Split exposes only
  continuous resize events, add a debounced `onResizeCommit` callback
  fired on mouseup so monitor doesn't spam `invoke` on every mousemove.
  If `Split` doesn't have any resize callback at all, monitor can
  install its own mousemove/mouseup pair locally (mirroring legacy);
  kit-side is preferred if it's a small prop add.

No other kit changes anticipated. Badge, Stack, Split, Button,
TextInput, PopoverSelect, Text, Root all already exist.

## Out of scope / follow-up

- **5000-row render performance under Remix v3 (follow-up test).**
  Today's `@arrow-js/core` is fine-grained reactive; Remix v3 is
  element-tree diff. After install, smoke-test by emitting a sustained
  high-volume topic (e.g., toggle the river `Frame` topic on) and watch
  for stutter / dropped frames. If perf is bad, the fast follow is to
  cap visible rows (e.g. last 500) with a "show more" pagination — not
  designing-in now. Track as a post-port issue.

## Risks

- **5000-row perf** (above) — flagged for measurement, not blocking.
- **Sticky replay timing** — `MonitorConfig` arrives via the bus pump,
  not before the first render. Default `sidebar_width = 240` is used
  until replay lands (typically within tens of milliseconds of mount).
  Acceptable; same pattern as theme replay.
