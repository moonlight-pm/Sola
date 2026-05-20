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
    Space, button, column, container, mouse_area, row, scrollable, text, text_input,
};
use iced::widget::Id as ScrollId;
use iced::widget::operation;
use iced::widget::scrollable::RelativeOffset;
use iced::{Color, Element, Length, Padding, Subscription, Task, Theme};

use sola_bus::topics::{
    AppMenuPayload, MenuActionPayload, MenuDefinition, MenuItem, Topic, TopicKind,
};
use sola_bus::{BusClient, Message};
use sola_core::KeyCode;

const APP_ID: &str = "sola-monitor-iced";
const MAX_MESSAGES: usize = 5_000;
const SIDEBAR_W: f32 = 280.0;
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
    iced::application(App::default, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .window(iced::window::Settings {
            decorations: false,
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: APP_ID.into(),
                ..Default::default()
            },
            ..iced::window::Settings::default()
        })
        .run()
}

fn messages_scroll_id() -> ScrollId {
    ScrollId::new("messages")
}

#[derive(Default)]
struct App {
    messages: Vec<Entry>,
    sticky: BTreeMap<String, Entry>,
    filter: String,
    paused: bool,
    pause_buffer: Vec<Entry>,
    /// Currently expanded row, by sequence number. `None` collapses
    /// every payload back to a single-line preview.
    selected_seq: Option<u64>,
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
    TogglePause,
    Clear,
    ToggleSelect(u64),
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
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        let toolbar = self.view_toolbar();
        let messages = self.view_messages();
        let sidebar = self.view_sidebar();

        let main = row![
            container(messages).width(Length::Fill).height(Length::Fill),
            container(
                Space::new()
                    .width(Length::Fixed(1.0))
                    .height(Length::Fill)
            )
            .style(divider_style),
            container(sidebar)
                .width(Length::Fixed(SIDEBAR_W))
                .height(Length::Fill),
        ]
        .height(Length::Fill);

        column![toolbar, main].into()
    }

    fn view_toolbar(&self) -> Element<'_, Msg> {
        let filter = text_input("filter…", &self.filter)
            .on_input(Msg::FilterChanged)
            .padding(Padding::new(4.0).left(8.0).right(8.0))
            .size(13);

        let pause_label = if self.paused {
            format!("Resume ({})", self.pause_buffer.len())
        } else {
            "Pause".into()
        };

        let toolbar_row = row![
            text("monitor").size(13),
            Space::new().width(Length::Fill),
            filter,
            button(text(pause_label).size(12)).on_press(Msg::TogglePause),
            button(text("Clear").size(12)).on_press(Msg::Clear),
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
            text("time").size(HEADER_FONT_PX).width(Length::Fixed(80.0)),
            text("topic")
                .size(HEADER_FONT_PX)
                .width(Length::Fixed(200.0)),
            text("source")
                .size(HEADER_FONT_PX)
                .width(Length::Fixed(120.0)),
            text("payload").size(HEADER_FONT_PX).width(Length::Fill),
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

            // Payload cell: preview when collapsed, full pretty JSON
            // wrapping across lines when selected.
            let payload_cell: Element<'_, Msg> = if selected && !entry.payload_pretty.is_empty()
            {
                text(&entry.payload_pretty)
                    .size(SELECTED_PAYLOAD_FONT_PX)
                    .width(Length::Fill)
                    .into()
            } else {
                text(&entry.payload_preview)
                    .size(ROW_FONT_PX)
                    .width(Length::Fill)
                    .into()
            };

            let line = row![
                text(t).size(ROW_FONT_PX).width(Length::Fixed(80.0)),
                text(&entry.topic)
                    .size(ROW_FONT_PX)
                    .width(Length::Fixed(200.0)),
                text(&entry.source)
                    .size(ROW_FONT_PX)
                    .width(Length::Fixed(120.0)),
                payload_cell,
            ]
            .spacing(8)
            .align_y(iced::Alignment::Start);

            let body = container(line)
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
        let header = container(text("sticky").size(HEADER_FONT_PX))
            .padding(Padding::new(4.0).left(12.0).right(12.0))
            .style(header_style)
            .width(Length::Fill);

        let mut rows: Vec<Element<'_, Msg>> = Vec::new();
        for entry in self.sticky.values() {
            let body_text = if entry.payload_preview.is_empty() {
                "<no payload>"
            } else {
                &entry.payload_preview
            };
            let block = column![
                text(&entry.topic).size(11),
                text(body_text).size(ROW_FONT_PX).color(hex("#8b949e")),
            ]
            .spacing(2);
            rows.push(
                container(block)
                    .padding(Padding::new(6.0).left(12.0).right(12.0))
                    .width(Length::Fill)
                    .into(),
            );
        }

        let body = scrollable(column(rows).spacing(0))
            .height(Length::Fill)
            .width(Length::Fill);

        column![header, body].into()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::run(bus_stream)
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
