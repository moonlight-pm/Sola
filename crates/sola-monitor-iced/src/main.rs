//! Iced port of sola-monitor. Visually parallels the existing
//! kit-based monitor: top toolbar (filter + pause + clear), main row
//! split between a scrollable messages list and a sticky-topics
//! sidebar.
//!
//! Window chrome is off — sola-shell frames + decorates every app
//! itself via its menubar; the app surface is just content. Same
//! convention every other sola app follows.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use iced::futures::SinkExt;
use iced::futures::Stream;
use iced::stream;
use iced::widget::{
    Space, button, column, container, row, scrollable, text, text_input,
};
use iced::{Color, Element, Length, Padding, Subscription, Task, Theme};

use sola_bus::topics::TopicKind;
use sola_bus::{BusClient, Message};

const APP_ID: &str = "sola-monitor-iced";
const MAX_MESSAGES: usize = 5_000;
const SIDEBAR_W: f32 = 280.0;
const ROW_FONT_PX: f32 = 12.0;
const HEADER_FONT_PX: f32 = 11.0;

fn main() -> iced::Result {
    sola_core::log::init(APP_ID);
    tracing::info!("{APP_ID} starting");

    let wayland_display = sola_core::env::activate_wayland_session(10_000);
    tracing::info!(socket = %wayland_display, "wayland socket resolved");

    iced::application(App::default, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .decorations(false)
        .run()
}

#[derive(Default)]
struct App {
    messages: Vec<Entry>,
    sticky: BTreeMap<String, Entry>,
    filter: String,
    paused: bool,
    pause_buffer: Vec<Entry>,
}

struct Entry {
    seq: u64,
    timestamp: f64,
    topic: String,
    source: String,
    payload_preview: String,
    is_sticky: bool,
}

impl Entry {
    fn from_message(msg: &Message, seq: u64) -> Self {
        let kind = TopicKind::from_str(&msg.topic);
        let is_sticky = kind.map(|k| k.behavior().is_sticky()).unwrap_or(false);
        let payload_preview = match &msg.payload {
            None => String::new(),
            Some(bytes) if bytes.is_empty() => String::new(),
            Some(bytes) => match serde_json::from_slice::<serde_json::Value>(bytes) {
                Ok(v) => {
                    let s = v.to_string();
                    if s.len() > 240 {
                        format!("{}…", &s[..240])
                    } else {
                        s
                    }
                }
                Err(_) => format!("<{} bytes>", bytes.len()),
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
                let seq = self.messages.len() as u64 + self.pause_buffer.len() as u64;
                let entry = Entry::from_message(&message, seq);
                if entry.is_sticky {
                    self.sticky.insert(entry.topic.clone(), Entry::from_message(&message, seq));
                }
                if self.paused {
                    self.pause_buffer.push(entry);
                } else {
                    self.messages.push(entry);
                    if self.messages.len() > MAX_MESSAGES {
                        let drop = self.messages.len() - MAX_MESSAGES;
                        self.messages.drain(0..drop);
                    }
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
            container(Space::new().width(Length::Fixed(1.0)).height(Length::Fill)).style(divider_style),
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
            text("topic").size(HEADER_FONT_PX).width(Length::Fixed(200.0)),
            text("source").size(HEADER_FONT_PX).width(Length::Fixed(120.0)),
            text("payload").size(HEADER_FONT_PX).width(Length::Fill),
        ]
        .spacing(8);

        let header = container(header)
            .padding(Padding::new(4.0).left(12.0).right(12.0))
            .style(header_style)
            .width(Length::Fill);

        let filter_lower = self.filter.to_lowercase();
        let mut rows: Vec<Element<'_, Msg>> = Vec::new();
        for entry in self.messages.iter().rev() {
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
            let t = format_clock(entry.timestamp);
            let line = row![
                text(t).size(ROW_FONT_PX).width(Length::Fixed(80.0)),
                text(&entry.topic)
                    .size(ROW_FONT_PX)
                    .width(Length::Fixed(200.0)),
                text(&entry.source)
                    .size(ROW_FONT_PX)
                    .width(Length::Fixed(120.0)),
                text(&entry.payload_preview)
                    .size(ROW_FONT_PX)
                    .width(Length::Fill),
            ]
            .spacing(8);
            let _ = entry.seq;
            rows.push(
                container(line)
                    .padding(Padding::new(2.0).left(12.0).right(12.0))
                    .width(Length::Fill)
                    .into(),
            );
        }

        let body = scrollable(column(rows).spacing(0))
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
            let block = column![
                text(&entry.topic).size(11),
                text(if entry.payload_preview.is_empty() {
                    "<no payload>"
                } else {
                    &entry.payload_preview
                })
                .size(ROW_FONT_PX)
                .color(hex("#8b949e")),
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
        let bus = Arc::new(Mutex::new(BusClient::new()));
        {
            let mut b = bus.lock().expect("bus poisoned");
            b.connect_blocking(std::time::Duration::from_millis(250));
            // subscribe_all isn't part of the wire-level API; subscribe
            // to the full known set instead. New kinds added later
            // require updating this list (or lifting a subscribe_all
            // helper into BusClient).
            if let Err(e) = b.subscribe(TopicKind::ALL) {
                tracing::warn!("bus subscribe failed: {e}");
            }
        }
        let bus_for_task = bus.clone();
        let (tx, mut rx) = iced::futures::channel::mpsc::unbounded::<Message>();
        std::thread::spawn(move || {
            loop {
                {
                    let bus = bus_for_task.lock().expect("bus poisoned");
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
        border: iced::Border {
            color: hex("#21262d"),
            width: 0.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn header_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(hex("#161b22"))),
        text_color: Some(hex("#8b949e")),
        border: iced::Border {
            color: hex("#21262d"),
            width: 0.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn divider_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(hex("#21262d"))),
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
