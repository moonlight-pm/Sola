//! The window: the picture, and everything said to the other machine.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use iced::futures::SinkExt as _;
use iced::futures::StreamExt as _;
use iced::keyboard::{self, key::Physical};
use iced::widget::{center, container, shader, text};
use iced::{Color, Element, Event, Length, Subscription, Task, Theme, event, stream, window};

use sola_bus::Message;
use sola_bus::topics::Topic;
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::theme::default_theme;
use sola_kvm::protocol::Packet;

use crate::APP_ID;
use crate::inhibit::{self, Inhibitor};
use crate::net::{self, Display, Outgoing};
use crate::picture::Picture;
use crate::pictures::Pictures;

const ESCAPE_EVDEV: u32 = 1;

/// How the other machine is, in words for the window.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Connecting,
    Showing,
    Lost(String),
    Refused(String),
}

pub struct App {
    /// `None`: no machine to show, and the status says why.
    host: Option<String>,
    port: u16,
    status: Status,
    display: Option<Display>,
    pictures: Arc<Pictures>,
    outgoing: Option<Sender<Outgoing>>,
    /// Keys the other machine has been told are down.
    held: BTreeSet<u32>,
    /// False once Ctrl+Alt+Esc has handed keyboard and pointer back.
    captured: bool,
    inhibitor: Option<Arc<Inhibitor>>,
    window_id: Option<window::Id>,
    theme: Theme,
    float: sola_kit::FloatState,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    Net(net::Event),
    Key {
        physical: Physical,
        pressed: bool,
        repeat: bool,
        modifiers: keyboard::Modifiers,
    },
    Pointer(Packet),
    Capture,
    Unfocused,
    Inhibited(Option<Arc<Inhibitor>>),
    WindowReady(Option<window::Id>),
    TitleDrag,
    TitleResize(window::Direction),
    TitleClose,
}

impl App {
    pub fn boot(target: Result<(String, u16), String>) -> (Self, Task<Msg>) {
        let (host, port, status) = match target {
            Ok((host, port)) => (Some(host), port, Status::Connecting),
            Err(why) => (None, 0, Status::Refused(why)),
        };
        (
            Self {
                host,
                port,
                status,
                display: None,
                pictures: Arc::default(),
                outgoing: None,
                held: BTreeSet::new(),
                captured: true,
                inhibitor: None,
                window_id: None,
                theme: default_theme(),
                float: sola_kit::FloatState::new(APP_ID),
            },
            sola_kit::window_ready_task(Msg::WindowReady),
        )
    }

    pub fn title(&self) -> String {
        self.name().to_string()
    }

    fn name(&self) -> &str {
        self.host.as_deref().unwrap_or("Remote Desktop")
    }

    pub fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.chrome_any(), &self.theme)
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        let link = self.host.clone().map_or_else(Subscription::none, |host| {
            Subscription::run_with(
                Link {
                    host,
                    port: self.port,
                    pictures: Arc::clone(&self.pictures),
                },
                run_link,
            )
        });
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            link.map(Msg::Net),
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    physical_key,
                    modifiers,
                    repeat,
                    ..
                }) => Some(Msg::Key {
                    physical: physical_key,
                    pressed: true,
                    repeat,
                    modifiers,
                }),
                Event::Keyboard(keyboard::Event::KeyReleased {
                    physical_key, modifiers, ..
                }) => Some(Msg::Key {
                    physical: physical_key,
                    pressed: false,
                    repeat: false,
                    modifiers,
                }),
                Event::Window(window::Event::Unfocused) => Some(Msg::Unfocused),
                _ => None,
            }),
        ])
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => {
                self.float.update(&message);
                apply_theme_update(&message, &mut self.theme);
                if is_self_quit(&message, APP_ID) {
                    return iced::exit();
                }
                if let Some(Topic::MenuAction(p)) = Topic::parse(&message) {
                    if p.app_id == APP_ID && p.action_id == "quit" {
                        return iced::exit();
                    }
                }
            }
            Msg::Net(event) => return self.on_net(event),
            Msg::Key {
                physical,
                pressed,
                repeat,
                modifiers,
            } => return self.on_key(&physical, pressed, repeat, modifiers),
            Msg::Pointer(packet) => self.send(packet),
            Msg::Capture => {
                self.captured = true;
                return self.inhibit();
            }
            Msg::Unfocused => self.release_keys(),
            Msg::Inhibited(inhibitor) => {
                // A release that landed while this was on its way wins.
                if self.captured {
                    self.inhibitor = inhibitor;
                }
            }
            Msg::WindowReady(id) => {
                self.window_id = id;
                return self.inhibit();
            }
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(direction) => return sola_kit::drag_resize(self.window_id, direction),
            Msg::TitleClose => sola_kit::close_app(APP_ID),
        }
        Task::none()
    }

    fn on_net(&mut self, event: net::Event) -> Task<Msg> {
        match event {
            net::Event::Ready(outgoing) => self.outgoing = Some(outgoing),
            net::Event::Connected(display) => {
                self.display = Some(display);
                self.status = Status::Showing;
            }
            // The message alone brings the redraw that uploads it.
            net::Event::Picture => self.pictures.drawn(),
            net::Event::Lost(why) => {
                self.status = Status::Lost(why);
                self.held.clear();
            }
            net::Event::Refused(why) => self.status = Status::Refused(why),
        }
        Task::none()
    }

    fn on_key(&mut self, physical: &Physical, pressed: bool, repeat: bool, modifiers: keyboard::Modifiers) -> Task<Msg> {
        if !self.captured {
            return Task::none();
        }
        let Some(code) = sola_kit::keys::evdev_of(physical) else {
            return Task::none();
        };
        if pressed && code == ESCAPE_EVDEV && modifiers.control() && modifiers.alt() {
            return self.release();
        }
        if pressed {
            self.held.insert(code);
        } else if !self.held.remove(&code) {
            // Pressed elsewhere and let go here: the other machine never
            // heard it go down.
            return Task::none();
        }
        let state = match (pressed, repeat) {
            (false, _) => 0,
            (true, false) => 1,
            (true, true) => 2,
        };
        self.send(Packet::Key { keycode: code, pressed: state });
        Task::none()
    }

    /// Ctrl+Alt+Esc: keyboard, pointer and the desk's shortcuts come back.
    /// A click on the picture takes them again.
    fn release(&mut self) -> Task<Msg> {
        self.captured = false;
        self.inhibitor = None;
        self.held.clear();
        self.send(Packet::Leave);
        Task::none()
    }

    fn release_keys(&mut self) {
        for keycode in std::mem::take(&mut self.held) {
            self.send(Packet::Key { keycode, pressed: 0 });
        }
    }

    fn inhibit(&self) -> Task<Msg> {
        let Some(id) = self.window_id else {
            return Task::none();
        };
        window::run(id, |window| inhibit::acquire(window).map(Arc::new)).map(Msg::Inhibited)
    }

    fn send(&self, packet: Packet) {
        if let Some(outgoing) = &self.outgoing {
            let _ = outgoing.send(Outgoing::Packet(packet));
        }
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let note = match &self.status {
            Status::Showing => None,
            Status::Connecting => Some(format!("Connecting to {}…", self.name())),
            Status::Lost(why) => Some(format!("Lost {}: {why}. Connecting again…", self.name())),
            Status::Refused(why) => Some(why.clone()),
        };
        let body: Element<'_, Msg> = match note {
            Some(note) => center(text(note)).into(),
            None => shader(Picture {
                pictures: Arc::clone(&self.pictures),
                points: self.display.map(|d| d.points),
                captured: self.captured,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        };
        let body = container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::background(Color::BLACK));
        sola_kit::window_frame(
            self.float.chrome_any(),
            self.name(),
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            body.into(),
        )
    }
}

/// The subscription's identity: one link per host.
struct Link {
    host: String,
    port: u16,
    pictures: Arc<Pictures>,
}

impl std::hash::Hash for Link {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.host.hash(state);
        self.port.hash(state);
    }
}

fn run_link(link: &Link) -> impl iced::futures::Stream<Item = net::Event> + use<> {
    let (host, port, pictures) = (link.host.clone(), link.port, Arc::clone(&link.pictures));
    stream::channel(64, async move |mut output: iced::futures::channel::mpsc::Sender<net::Event>| {
        let (tx, mut rx) = iced::futures::channel::mpsc::unbounded();
        net::start(host, port, pictures, tx);
        while let Some(event) = rx.next().await {
            if output.send(event).await.is_err() {
                return;
            }
        }
    })
}
