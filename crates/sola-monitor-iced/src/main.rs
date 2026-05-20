//! Iced port of sola-monitor — parallel evaluation crate for the
//! UI Frameworks investigation (see `docs/vault/UI Frameworks.md`).
//!
//! This first cut is intentionally minimal: connect to sola-bus,
//! subscribe to the topic firehose, render incoming topics in a
//! scrolling list. The goal is to validate the bus → iced bridge
//! and the round-trip with the shell (xdg_toplevel sized via
//! `Topic::Frame`, menu via `Topic::SetAppMenu`, etc.) before
//! growing the UI to match what the kit-based sola-monitor shows.

use std::sync::Arc;
use std::sync::Mutex;

use iced::futures::SinkExt;
use iced::futures::Stream;
use iced::stream;
use iced::widget::{column, container, scrollable, text};
use iced::{Element, Length, Subscription, Task, Theme};

use sola_bus::topics::TopicKind;
use sola_bus::{BusClient, Message};

const APP_ID: &str = "sola-monitor-iced";

fn main() -> iced::Result {
    sola_core::log::init(APP_ID);
    tracing::info!("{APP_ID} starting");

    // Auto-discover sola-river's wayland socket and set WAYLAND_DISPLAY
    // before any iced/winit/sctk code runs. Makes the app launchable
    // from a remote / non-graphical shell (where the inherited env
    // doesn't have WAYLAND_DISPLAY set), matching how sola-kit apps
    // bootstrap. 10s timeout is generous — sola is normally already
    // up when the user launches an app.
    let wayland_display = sola_core::env::activate_wayland_session(10_000);
    tracing::info!(socket = %wayland_display, "wayland socket resolved");

    iced::application(App::default, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .run()
}

#[derive(Default)]
struct App {
    /// Most recent bus messages, oldest first. Capped so the UI
    /// stays responsive — sola can produce a lot of topics.
    messages: Vec<TopicLine>,
}

struct TopicLine {
    source: String,
    topic: String,
}

#[derive(Debug, Clone)]
enum Msg {
    BusMessage(Arc<Message>),
    BusConnected,
}

impl App {
    fn title(&self) -> String {
        "Sola Monitor (iced)".into()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::BusConnected => {
                tracing::info!("bus connected, awaiting messages");
            }
            Msg::BusMessage(message) => {
                self.messages.push(TopicLine {
                    source: message.source.clone(),
                    topic: message.topic.clone(),
                });
                // Hard cap so the firehose can't unbounded-grow during
                // a long session.
                const MAX_LINES: usize = 5_000;
                if self.messages.len() > MAX_LINES {
                    let drop = self.messages.len() - MAX_LINES;
                    self.messages.drain(0..drop);
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        let lines = self
            .messages
            .iter()
            .map(|line| text(format!("{:>16}  {}", line.source, line.topic)).size(13).into())
            .collect::<Vec<_>>();

        let body = scrollable(column(lines).spacing(2).padding(12))
            .height(Length::Fill)
            .width(Length::Fill);

        container(body).into()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::run(bus_stream)
    }
}

/// Async stream that owns the BusClient on a blocking-friendly task
/// and yields messages to the iced runtime.
///
/// `BusClient` is poll/blocking — we keep it on a dedicated tokio
/// blocking task, drain its notify pipe, and forward each message
/// through a channel that the iced subscription consumes. The Arc
/// wrap on `Message` keeps the clone cheap when iced's runtime
/// passes it back to `update`.
fn bus_stream() -> impl Stream<Item = Msg> {
    stream::channel(64, |mut output: iced::futures::channel::mpsc::Sender<Msg>| async move {
        let _ = output.send(Msg::BusConnected).await;

        let bus = Arc::new(Mutex::new(BusClient::new()));
        {
            let mut b = bus.lock().expect("bus poisoned");
            b.connect_blocking(std::time::Duration::from_millis(250));
            if let Err(e) = b.subscribe(&[
                TopicKind::Windows,
                TopicKind::Application,
                TopicKind::Composition,
                TopicKind::Focus,
                TopicKind::Frame,
                TopicKind::OutputGeometry,
                TopicKind::Theme,
                TopicKind::Chord,
                TopicKind::MouseClicked,
                TopicKind::MouseEntered,
                TopicKind::MouseLeft,
                TopicKind::LaunchApp,
                TopicKind::CloseApp,
                TopicKind::SetAppMenu,
                TopicKind::MenuAction,
            ]) {
                tracing::warn!("bus subscribe failed: {e}");
            }
        }

        // Pump in a blocking task and forward via the channel. The
        // bus notify pipe wakes us when traffic arrives; we drain
        // every available message per wake to amortize the cost.
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
                // Cheap idle backoff. The drain_notify above is
                // edge-triggered — without this the loop spins.
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
