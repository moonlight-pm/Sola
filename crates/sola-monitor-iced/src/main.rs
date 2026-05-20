//! Iced port of sola-monitor. Visually parallels the existing
//! kit-based monitor: top toolbar (filter + pause + clear), main row
//! split between a scrollable messages list and a sticky-topics
//! sidebar. Click a row to expand its full pretty-printed payload.
//!
//! Window chrome is off — sola-shell frames + decorates every app
//! itself via its menubar; the app surface is just content.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use iced::futures::SinkExt;
use iced::futures::Stream;
use iced::stream;
use iced::widget::{
    Space, button, column, container, mouse_area, pick_list, rich_text, row, scrollable, span, stack,
    text, text_input,
};
use iced::widget::Id as ScrollId;
use iced::widget::operation;
use iced::widget::scrollable::RelativeOffset;
use iced::widget::text::{Span, Wrapping};
use iced::{Color, Element, Event, Font, Length, Never, Padding, Subscription, Task, Theme};
use iced::{event, mouse};

use sola_bus::topics::{
    AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, Topic, TopicKind,
};
use sola_bus::{BusClient, Message};
use sola_core::KeyCode;

const APP_ID: &str = "sola-monitor-iced";

/// Font pack directory shared with every other sola process. Each
/// pack is a single TTF written here by `cargo make assets sync`.
const FONT_DIR: &str = "/opt/sola/share/fonts";

/// Mono font for row bodies and JSON. JetBrainsMono-Regular.ttf
/// declares itself as `JetBrains Mono`.
const F_MONO: Font = Font::with_name("JetBrains Mono");

/// Condensed sans for column headers and widget chrome (buttons,
/// pick_list values). The classic static `RobotoCondensed-Regular.ttf`
/// declares the family as `Roboto Condensed`.
const F_CONDENSED: Font = Font::with_name("Roboto Condensed");

/// Default sans for "normal" text — topic/source cells, sticky title,
/// text inputs. Variable Roboto Flex, family name `Roboto Flex`.
const F_NORMAL: Font = Font::with_name("Roboto Flex");

/// Font files we try to register at startup, relative to [`FONT_DIR`].
/// Missing files are warned about but not fatal so a binary built
/// against an out-of-date `/opt/sola/share` still launches.
const FONT_FILES: &[&str] = &[
    "JetBrainsMono/JetBrainsMono-Regular.ttf",
    "RobotoFlex/RobotoFlex.ttf",
    "RobotoCondensed/RobotoCondensed-Regular.ttf",
];
const MAX_MESSAGES: usize = 5_000;
const SIDEBAR_W_DEFAULT: f32 = 280.0;
const SIDEBAR_W_MIN: f32 = 160.0;
const SIDEBAR_W_MAX: f32 = 700.0;
/// Sentinel option in the topic-filter pick_list meaning "no filter".
const FILTER_ALL: &str = "(all topics)";
const ROW_FONT_PX: f32 = 12.0;
const HEADER_FONT_PX: f32 = 11.0;
const SELECTED_PAYLOAD_FONT_PX: f32 = 12.0;

/// Single global BusClient for the process. iced's `application`
/// builder doesn't thread caller-supplied state into the state
/// constructor, so the alternative is either a static (this) or
/// a thread-local. Static fits since there's exactly one bus
/// connection per process.
static BUS: OnceLock<Arc<Mutex<BusClient>>> = OnceLock::new();

fn bus() -> &'static Mutex<BusClient> {
    BUS.get().expect("bus not initialized").as_ref()
}

fn main() -> iced::Result {
    sola_core::log::init(APP_ID);
    tracing::info!("{APP_ID} starting");

    let wayland_display = sola_core::env::activate_wayland_session(10_000);
    tracing::info!(socket = %wayland_display, "wayland socket resolved");

    // Set up the bus before iced takes over the thread: connect, sub
    // to every topic kind, publish our app menu so Cmd+Q works.
    let mut client = BusClient::new();
    client.connect_blocking(std::time::Duration::from_millis(250));
    if let Err(e) = client.subscribe(TopicKind::ALL) {
        tracing::warn!("bus subscribe failed: {e}");
    }
    let _ = client.emit(Topic::SetAppMenu(AppMenuPayload {
        app_id: APP_ID.into(),
        menus: vec![MenuDefinition {
            label: "Monitor".into(),
            items: vec![MenuItem::Action {
                id: "quit".into(),
                label: "Quit Monitor".into(),
                shortcut: Some(KeyCode::Q.meta()),
                disabled: false,
                checked: false,
            }],
        }],
    }));
    BUS.set(Arc::new(Mutex::new(client)))
        .map_err(|_| ())
        .expect("BUS set twice");

    // The wayland `xdg_toplevel.app_id` is what river and the shell
    // use to identify this window — including looking up the SetAppMenu
    // entry above. On Linux iced reads it from
    // `window::Settings.platform_specific.application_id`, NOT from
    // the top-level `Settings::id` (that field is only wired to
    // `winit::with_name` on dragonfly/freebsd/netbsd/openbsd — Linux
    // is omitted from the cfg in iced_winit 0.14's
    // `conversion::window_attributes`). Without setting it here, the
    // window has empty app_id and the shell can't match our menu.
    // Note: `.window(...)` wholesale replaces the window settings,
    // so `decorations: false` has to live inside this struct rather
    // than as a separate `.decorations(false)` call (which would be
    // overwritten by the subsequent `.window(...)`).
    let mut app = iced::application(App::default, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(F_NORMAL)
        .window(iced::window::Settings {
            decorations: false,
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: APP_ID.into(),
                ..Default::default()
            },
            ..iced::window::Settings::default()
        });
    for relative in FONT_FILES {
        let path = format!("{FONT_DIR}/{relative}");
        match std::fs::read(&path) {
            Ok(bytes) => {
                tracing::info!(path = %path, bytes = bytes.len(), "registering font");
                app = app.font(bytes);
            }
            Err(e) => {
                tracing::warn!(path = %path, "skipping font: {e}");
            }
        }
    }
    app.run()
}

fn messages_scroll_id() -> ScrollId {
    ScrollId::new("messages")
}

struct App {
    messages: Vec<Entry>,
    sticky: BTreeMap<String, Entry>,
    filter: String,
    /// Sticky topic name to filter on, or `None` for "all". The
    /// pick_list emits the sentinel `FILTER_ALL` string to mean
    /// none — `Msg::TopicFilterChanged` maps that back to `None`.
    topic_filter: Option<String>,
    paused: bool,
    pause_buffer: Vec<Entry>,
    /// Currently expanded message row, by sequence number. `None`
    /// collapses every payload back to a single-line preview.
    selected_seq: Option<u64>,
    /// Currently expanded sticky topic. `None` collapses every
    /// sticky entry to its one-line form.
    selected_sticky_topic: Option<String>,
    /// Sidebar width in logical pixels. Bound to the draggable
    /// divider between messages and the sticky panel.
    sidebar_w: f32,
    /// True while the user is mid-drag on the divider.
    dragging_divider: bool,
    /// Most-recent global cursor x, tracked continuously (even
    /// when not dragging) so that `drag_anchor` can be captured
    /// at `DividerPress` time without needing the press event
    /// to carry a position.
    last_cursor_x: Option<f32>,
    /// `(cursor_x_at_press, sidebar_w_at_press)`. The drag uses
    /// the press position as a fixed anchor and recomputes
    /// `sidebar_w` from absolute cursor displacement, NOT from
    /// accumulated frame-deltas. Anchor-based fixes the drift
    /// that delta-accumulation produces when the cursor moves
    /// past the clamp range and back (a missed delta or a
    /// clamped frame would leave the divider permanently offset
    /// from the cursor).
    drag_anchor: Option<(f32, f32)>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            sticky: BTreeMap::new(),
            filter: String::new(),
            topic_filter: None,
            paused: false,
            pause_buffer: Vec::new(),
            selected_seq: None,
            selected_sticky_topic: None,
            sidebar_w: SIDEBAR_W_DEFAULT,
            dragging_divider: false,
            last_cursor_x: None,
            drag_anchor: None,
        }
    }
}

struct Entry {
    seq: u64,
    timestamp: f64,
    topic: String,
    source: String,
    /// Single-line abbreviated form for the table preview.
    payload_preview: String,
    /// Multi-line pretty-printed form shown when the row is selected.
    /// Empty for rows without a payload.
    payload_pretty: String,
    is_sticky: bool,
}

impl Entry {
    fn from_message(msg: &Message, seq: u64) -> Self {
        let kind = TopicKind::from_str(&msg.topic);
        let is_sticky = kind.map(|k| k.behavior().is_sticky()).unwrap_or(false);
        // Bus payloads are postcard-encoded, not JSON. Topic::parse
        // decodes via the schema generated by `define_topics!`, then
        // to_json_value converts to a serde_json::Value we can render.
        // Falls back to a `<N bytes>` placeholder for malformed or
        // unknown traffic — same strategy as the kit-based monitor.
        let (payload_preview, payload_pretty) = match Topic::parse(msg) {
            Some(topic) => {
                let v = topic.to_json_value();
                if v.is_null() {
                    (String::new(), String::new())
                } else {
                    let compact = v.to_string();
                    let preview = if compact.len() > 240 {
                        format!("{}…", &compact[..240])
                    } else {
                        compact
                    };
                    let pretty = serde_json::to_string_pretty(&v).unwrap_or_default();
                    (preview, pretty)
                }
            }
            None => match &msg.payload {
                None => (String::new(), String::new()),
                Some(bytes) if bytes.is_empty() => (String::new(), String::new()),
                Some(bytes) => {
                    let s = format!("<{} bytes, unparsed>", bytes.len());
                    (s.clone(), s)
                }
            },
        };
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        Self {
            seq,
            timestamp,
            topic: msg.topic.clone(),
            source: msg.source.clone(),
            payload_preview,
            payload_pretty,
            is_sticky,
        }
    }
}

#[derive(Debug, Clone)]
enum Msg {
    BusMessage(Arc<Message>),
    FilterChanged(String),
    TopicFilterChanged(String),
    TogglePause,
    Clear,
    ToggleSelect(u64),
    ToggleSelectSticky(String),
    /// User pressed the mouse button on the divider.
    DividerPress,
    /// Global cursor moved (only meaningful during `dragging_divider`).
    CursorMoved(f32),
    /// Global mouse-button-released (only meaningful during drag).
    CursorReleased,
}

impl App {
    fn title(&self) -> String {
        "Sola Monitor".into()
    }

    fn theme(&self) -> Theme {
        Theme::custom(
            String::from("sola-monitor"),
            iced::theme::Palette {
                background: hex("#0d1117"),
                text: hex("#c9d1d9"),
                primary: hex("#58a6ff"),
                success: hex("#3fb950"),
                warning: hex("#d29922"),
                danger: hex("#f85149"),
            },
        )
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::BusMessage(message) => {
                // Intercept our own MenuAction("quit") for Cmd+Q.
                // We decode via Topic::parse (postcard) since the
                // payload is not JSON on the wire.
                let our_quit = match Topic::parse(&message) {
                    Some(Topic::MenuAction(MenuActionPayload { app_id, action_id })) => {
                        app_id == APP_ID && action_id == "quit"
                    }
                    _ => false,
                };

                let seq = self.messages.len() as u64 + self.pause_buffer.len() as u64;
                let entry = Entry::from_message(&message, seq);
                let mut appended_live = false;
                if entry.is_sticky {
                    self.sticky
                        .insert(entry.topic.clone(), Entry::from_message(&message, seq));
                }
                if self.paused {
                    self.pause_buffer.push(entry);
                } else {
                    self.messages.push(entry);
                    appended_live = true;
                    if self.messages.len() > MAX_MESSAGES {
                        let drop = self.messages.len() - MAX_MESSAGES;
                        self.messages.drain(0..drop);
                        if let Some(sel) = self.selected_seq {
                            if (sel as usize) < drop {
                                self.selected_seq = None;
                            }
                        }
                    }
                }

                if our_quit {
                    return iced::exit();
                }

                // Auto-scroll to the bottom whenever a new message
                // lands in the live list. Snapping is unconditional
                // — we don't try to detect "user scrolled away to
                // read history" yet. If that becomes annoying, gate
                // this on a `tail_following: bool` toggled by the
                // scrollable's `on_scroll` callback.
                if appended_live {
                    return operation::snap_to(
                        messages_scroll_id(),
                        RelativeOffset::END,
                    );
                }
            }
            Msg::FilterChanged(s) => self.filter = s,
            Msg::TogglePause => {
                self.paused = !self.paused;
                if !self.paused {
                    let drained: Vec<_> = self.pause_buffer.drain(..).collect();
                    self.messages.extend(drained);
                    if self.messages.len() > MAX_MESSAGES {
                        let drop = self.messages.len() - MAX_MESSAGES;
                        self.messages.drain(0..drop);
                    }
                }
            }
            Msg::Clear => {
                self.messages.clear();
                self.pause_buffer.clear();
                self.selected_seq = None;
            }
            Msg::ToggleSelect(seq) => {
                self.selected_seq = if self.selected_seq == Some(seq) {
                    None
                } else {
                    Some(seq)
                };
            }
            Msg::ToggleSelectSticky(topic) => {
                self.selected_sticky_topic = if self.selected_sticky_topic.as_ref() == Some(&topic)
                {
                    None
                } else {
                    Some(topic)
                };
            }
            Msg::TopicFilterChanged(t) => {
                self.topic_filter = if t == FILTER_ALL { None } else { Some(t) };
            }
            Msg::DividerPress => {
                self.dragging_divider = true;
                // Capture the anchor at press time. `last_cursor_x`
                // is continuously tracked, so it's already current
                // when the press fires.
                if let Some(x) = self.last_cursor_x {
                    self.drag_anchor = Some((x, self.sidebar_w));
                }
            }
            Msg::CursorMoved(x) => {
                self.last_cursor_x = Some(x);
                if self.dragging_divider {
                    if let Some((anchor_x, anchor_w)) = self.drag_anchor {
                        // Absolute, anchor-relative: sidebar grows by
                        // however far the cursor has moved LEFT from
                        // the anchor (negative delta), shrinks if it
                        // moved right. Clamping doesn't accumulate
                        // drift — the next frame recomputes from the
                        // anchor and the cursor re-syncs as soon as
                        // it returns to a non-clamped position.
                        let desired = anchor_w + (anchor_x - x);
                        self.sidebar_w = desired.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX);
                    }
                }
            }
            Msg::CursorReleased => {
                if self.dragging_divider {
                    self.dragging_divider = false;
                    self.drag_anchor = None;
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        let toolbar = self.view_toolbar();
        let messages = self.view_messages();
        let sidebar = self.view_sidebar();

        // 8px-wide column separator. Interaction is `ResizingColumn`
        // (not `ResizingHorizontally`) — it's a column-divider drag,
        // and it maps via winit→sctk to `Shape::ColResize` whose XDG
        // cursor name is `col-resize`. The generic `ew-resize` name
        // that `ResizingHorizontally` would request is absent from
        // most themes (McMojave included), and wlroots silently
        // substitutes default when a cursor name isn't found.
        let divider = mouse_area(
            container(
                Space::new()
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .style(divider_style)
            .width(Length::Fixed(8.0))
            .height(Length::Fill),
        )
        .interaction(mouse::Interaction::ResizingColumn)
        .on_press(Msg::DividerPress);

        let main = row![
            container(messages).width(Length::Fill).height(Length::Fill),
            divider,
            container(sidebar)
                .width(Length::Fixed(self.sidebar_w))
                .height(Length::Fill),
        ]
        .height(Length::Fill);

        let body: Element<'_, Msg> = column![toolbar, main].into();

        // iced has no pointer-capture API. While dragging the divider,
        // hit-testing still runs every frame against the current widget
        // tree — so when the cursor races ahead of the lagging divider,
        // it crosses into a sibling that returns a different (or no)
        // cursor shape and the cursor flickers between resize and
        // default. AppKit / browsers solve this by routing all pointer
        // events to the widget that received mouse-down until release;
        // in iced the equivalent is a transparent overlay that
        // unconditionally declares the desired cursor for the duration
        // of the drag. Stack evaluates mouse_interaction top-down and
        // returns the first non-None, so the overlay wins.
        if self.dragging_divider {
            stack![
                body,
                mouse_area(
                    Space::new()
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .interaction(mouse::Interaction::ResizingColumn),
            ]
            .into()
        } else {
            body
        }
    }

    fn view_toolbar(&self) -> Element<'_, Msg> {
        let filter = text_input("filter…", &self.filter)
            .on_input(Msg::FilterChanged)
            .font(F_NORMAL)
            .padding(Padding::new(4.0).left(8.0).right(8.0))
            .size(13);

        let topic_options = topic_filter_options();
        let topic_selected = self
            .topic_filter
            .clone()
            .unwrap_or_else(|| FILTER_ALL.to_string());
        let topic_picker = pick_list(
            topic_options,
            Some(topic_selected),
            Msg::TopicFilterChanged,
        )
        .font(F_CONDENSED)
        .text_size(12)
        .padding(Padding::new(4.0).left(8.0).right(8.0));

        let pause_label = if self.paused {
            format!("Resume ({})", self.pause_buffer.len())
        } else {
            "Pause".into()
        };

        let toolbar_row = row![
            Space::new().width(Length::Fill),
            filter,
            topic_picker,
            button(text(pause_label).font(F_CONDENSED).size(12))
                .on_press(Msg::TogglePause),
            button(text("Clear").font(F_CONDENSED).size(12)).on_press(Msg::Clear),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        container(toolbar_row)
            .padding(Padding::new(8.0))
            .style(toolbar_style)
            .width(Length::Fill)
            .into()
    }

    fn view_messages(&self) -> Element<'_, Msg> {
        let header = row![
            text("time")
                .font(F_CONDENSED)
                .size(HEADER_FONT_PX)
                .width(Length::Fixed(80.0)),
            text("topic")
                .font(F_CONDENSED)
                .size(HEADER_FONT_PX)
                .width(Length::Fixed(200.0)),
            text("source")
                .font(F_CONDENSED)
                .size(HEADER_FONT_PX)
                .width(Length::Fixed(120.0)),
            text("payload")
                .font(F_CONDENSED)
                .size(HEADER_FONT_PX)
                .width(Length::Fill),
        ]
        .spacing(8);

        let header = container(header)
            .padding(Padding::new(4.0).left(12.0).right(12.0))
            .style(header_style)
            .width(Length::Fill);

        let filter_lower = self.filter.to_lowercase();
        let mut rows: Vec<Element<'_, Msg>> = Vec::new();
        // Oldest first (top of list). The scrollable auto-snaps to
        // the bottom on each new message via a `Task::scroll_to` in
        // `update`, so the user sees newest content without losing
        // a natural chronological reading order.
        for entry in self.messages.iter() {
            if let Some(t) = &self.topic_filter {
                if &entry.topic != t {
                    continue;
                }
            }
            if !filter_lower.is_empty() {
                let hay = format!(
                    "{} {} {}",
                    entry.topic, entry.source, entry.payload_preview
                )
                .to_lowercase();
                if !hay.contains(&filter_lower) {
                    continue;
                }
            }
            let selected = self.selected_seq == Some(entry.seq);
            let t = format_clock(entry.timestamp);

            let one_line = row![
                text(t)
                    .font(F_MONO)
                    .size(ROW_FONT_PX)
                    .width(Length::Fixed(80.0)),
                text(&entry.topic)
                    .font(F_NORMAL)
                    .size(ROW_FONT_PX)
                    .width(Length::Fixed(200.0))
                    .wrapping(Wrapping::None),
                text(&entry.source)
                    .font(F_NORMAL)
                    .size(ROW_FONT_PX)
                    .width(Length::Fixed(120.0))
                    .wrapping(Wrapping::None),
                // Single-line highlighted JSON preview — same colorizer
                // as the expanded form, just with wrapping disabled so
                // long payloads clip at the cell edge instead of
                // breaking row alignment.
                preview_payload(&entry.payload_preview),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Start);

            let row_content: Element<'_, Msg> = if selected && !entry.payload_pretty.is_empty()
            {
                column![one_line, expanded_payload(&entry.payload_pretty)]
                    .spacing(4)
                    .into()
            } else {
                one_line.into()
            };

            let body = container(row_content)
                .padding(Padding::new(2.0).left(12.0).right(12.0))
                .width(Length::Fill)
                .style(if selected {
                    selected_row_style
                } else {
                    plain_row_style
                });

            let seq = entry.seq;
            rows.push(
                mouse_area(body)
                    .interaction(mouse::Interaction::Pointer)
                    .on_press(Msg::ToggleSelect(seq))
                    .into(),
            );
        }

        let body = scrollable(column(rows).spacing(0))
            .id(messages_scroll_id())
            .height(Length::Fill)
            .width(Length::Fill);

        column![header, body].into()
    }

    fn view_sidebar(&self) -> Element<'_, Msg> {
        let header = container(
            text("Sticky State")
                .font(F_CONDENSED)
                .size(HEADER_FONT_PX),
        )
        .padding(Padding::new(4.0).left(12.0).right(12.0))
        .style(header_style)
        .width(Length::Fill);

        let mut rows: Vec<Element<'_, Msg>> = Vec::new();
        for entry in self.sticky.values() {
            let selected = self
                .selected_sticky_topic
                .as_deref()
                .map(|t| t == entry.topic.as_str())
                .unwrap_or(false);

            // Topic + source on a single line, matching the kit's
            // StickyPanel layout. No inline JSON preview — clicking
            // expands the full pretty-printed payload below.
            let one_line = row![
                text(&entry.topic)
                    .font(F_NORMAL)
                    .size(ROW_FONT_PX)
                    .width(Length::Fill)
                    .wrapping(Wrapping::None),
                text(&entry.source)
                    .font(F_NORMAL)
                    .size(ROW_FONT_PX)
                    .wrapping(Wrapping::None),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Start);

            let row_content: Element<'_, Msg> = if selected && !entry.payload_pretty.is_empty()
            {
                column![one_line, expanded_payload(&entry.payload_pretty)]
                    .spacing(4)
                    .into()
            } else {
                one_line.into()
            };

            let body = container(row_content)
                .padding(Padding::new(4.0).left(12.0).right(12.0))
                .width(Length::Fill)
                .style(if selected {
                    selected_row_style
                } else {
                    plain_row_style
                });

            let topic = entry.topic.clone();
            rows.push(
                mouse_area(body)
                    .interaction(mouse::Interaction::Pointer)
                    .on_press(Msg::ToggleSelectSticky(topic))
                    .into(),
            );
        }

        let body = scrollable(column(rows).spacing(0))
            .height(Length::Fill)
            .width(Length::Fill);

        column![header, body].into()
    }

    fn subscription(&self) -> Subscription<Msg> {
        // Always-on global event listener so DividerPress can be
        // followed by CursorMoved / CursorReleased without a tick
        // delay from re-evaluating subscription. iced's update path
        // bails on CursorMoved when not dragging, so the cost is
        // a no-op call per cursor sample — fine for a debug UI.
        Subscription::batch([
            Subscription::run(bus_stream),
            event::listen_with(|event, _, _| match event {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::CursorMoved(position.x))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::CursorReleased)
                }
                _ => None,
            }),
        ])
    }
}

fn bus_stream() -> impl Stream<Item = Msg> {
    stream::channel(64, |mut output: iced::futures::channel::mpsc::Sender<Msg>| async move {
        // The bus connection + subscription are already done in main()
        // before iced takes the thread — here we just spin a poller
        // that forwards messages into the subscription channel.
        let (tx, mut rx) = iced::futures::channel::mpsc::unbounded::<Message>();
        std::thread::spawn(move || {
            loop {
                {
                    let bus = bus().lock().expect("bus poisoned");
                    bus.drain_notify();
                    while let Some(msg) = bus.try_recv() {
                        if tx.unbounded_send(msg).is_err() {
                            return;
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
        });
        use iced::futures::StreamExt;
        while let Some(msg) = rx.next().await {
            if output.send(Msg::BusMessage(Arc::new(msg))).await.is_err() {
                break;
            }
        }
    })
}

// ── Styling helpers ────────────────────────────────────────────────

fn hex(s: &str) -> Color {
    let s = s.trim_start_matches('#');
    let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(0);
    Color::from_rgb8(r, g, b)
}

fn toolbar_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(hex("#161b22"))),
        ..container::Style::default()
    }
}

fn header_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(hex("#161b22"))),
        text_color: Some(hex("#8b949e")),
        ..container::Style::default()
    }
}

fn divider_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(hex("#21262d"))),
        ..container::Style::default()
    }
}

fn plain_row_style(_: &Theme) -> container::Style {
    container::Style::default()
}

fn selected_row_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(hex("#1c2129"))),
        ..container::Style::default()
    }
}

/// Render a pretty-printed JSON payload as a syntax-highlighted
/// rich_text widget. Mono font, GitHub-dark-ish color palette
/// (keys blue, strings green, numbers red-orange, booleans/null
/// orange, punctuation muted gray).
fn expanded_payload(json: &str) -> Element<'static, Msg> {
    rich_text(highlight_json(json))
        .font(F_MONO)
        .size(SELECTED_PAYLOAD_FONT_PX)
        .on_link_click(iced::never)
        .into()
}

/// Single-line highlighted preview of a JSON payload. Width fills
/// the parent cell; `wrapping(None)` clips at the edge so the row
/// stays one line tall and column alignment stays intact.
fn preview_payload(json: &str) -> Element<'static, Msg> {
    if json.is_empty() {
        return text("—").size(ROW_FONT_PX).color(hex("#8b949e")).into();
    }
    rich_text(highlight_json(json))
        .font(F_MONO)
        .size(ROW_FONT_PX)
        .wrapping(Wrapping::None)
        .width(Length::Fill)
        .on_link_click(iced::never)
        .into()
}

/// Static list of options for the topic filter dropdown.
/// `FILTER_ALL` is the sentinel "no filter" value at index 0; the
/// rest are the topic-kind names sorted alphabetically so the user
/// can scan them quickly.
fn topic_filter_options() -> Vec<String> {
    let mut v: Vec<String> = std::iter::once(FILTER_ALL.to_string())
        .chain(TopicKind::ALL.iter().map(|k| k.as_str().to_string()))
        .collect();
    // Sentinel stays at index 0; sort the rest.
    let tail = &mut v[1..];
    tail.sort();
    v
}

/// Single-pass JSON tokenizer that emits colored `Span`s.
/// Best-effort — assumes the input is already valid JSON (we built
/// it via `serde_json::to_string_pretty`). Malformed input would
/// produce visually-wrong spans but won't panic.
///
/// Key vs string distinction: after closing a string, peek ahead
/// past whitespace to see whether the next non-whitespace char is
/// `:` — keys get the blue color, value strings get green.
fn highlight_json(src: &str) -> Vec<Span<'static, Never>> {
    // The `span()` helper is generic over Link and won't infer
    // `Link = Never` from the Vec context alone; a tiny shim
    // pins the type so the call sites stay clean.
    fn colored(text: String, c: &str) -> Span<'static, Never> {
        span(text).color(hex(c))
    }
    const C_PUNCT: &str = "#8b949e";
    const C_KEY: &str = "#79c0ff";
    const C_STRING: &str = "#7ee787";
    const C_NUMBER: &str = "#ff7b72";
    const C_LITERAL: &str = "#ffa657";
    const C_DEFAULT: &str = "#c9d1d9";

    let mut out: Vec<Span<'static, Never>> = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'{' | b'}' | b'[' | b']' | b',' | b':' => {
                out.push(colored(String::from(b as char), C_PUNCT));
                i += 1;
            }
            b'"' => {
                // Read until next unescaped quote.
                let start = i;
                i += 1;
                let mut escaped = false;
                while i < bytes.len() {
                    let c = bytes[i];
                    i += 1;
                    if escaped {
                        escaped = false;
                    } else if c == b'\\' {
                        escaped = true;
                    } else if c == b'"' {
                        break;
                    }
                }
                let s_bytes = &bytes[start..i];
                let s = String::from_utf8_lossy(s_bytes).into_owned();
                // Peek past whitespace to see if this is a key.
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let is_key = j < bytes.len() && bytes[j] == b':';
                let color = if is_key { C_KEY } else { C_STRING };
                out.push(colored(s, color));
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && matches!(
                        bytes[i],
                        b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-'
                    )
                {
                    i += 1;
                }
                let s = String::from_utf8_lossy(&bytes[start..i]).into_owned();
                out.push(colored(s, C_NUMBER));
            }
            b't' | b'f' | b'n' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let s = String::from_utf8_lossy(&bytes[start..i]).into_owned();
                out.push(colored(s, C_LITERAL));
            }
            _ => {
                // Whitespace and stragglers — keep as default color.
                let start = i;
                while i < bytes.len()
                    && !matches!(
                        bytes[i],
                        b'{' | b'}'
                            | b'['
                            | b']'
                            | b','
                            | b':'
                            | b'"'
                            | b'-'
                            | b'0'..=b'9'
                            | b't'
                            | b'f'
                            | b'n'
                    )
                {
                    i += 1;
                }
                if i > start {
                    let s = String::from_utf8_lossy(&bytes[start..i]).into_owned();
                    out.push(colored(s, C_DEFAULT));
                }
            }
        }
    }
    out
}

/// HH:MM:SS.mmm clock from a unix-seconds float. Matches the existing
/// monitor's time column shape so columns line up visually.
fn format_clock(unix_secs: f64) -> String {
    let total_ms = (unix_secs * 1000.0) as i64;
    let seconds_today = (total_ms / 1000) % 86400;
    let ms = total_ms % 1000;
    let h = seconds_today / 3600;
    let m = (seconds_today % 3600) / 60;
    let s = seconds_today % 60;
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}
