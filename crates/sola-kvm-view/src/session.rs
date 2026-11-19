//! One connection's window: the picture, and everything said to the other
//! machine. Several can be open, one machine each.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;

use iced::futures::SinkExt as _;
use iced::futures::StreamExt as _;
use iced::keyboard::{self, key::Physical};
use iced::widget::{Stack, center, container, shader, text};
use iced::{Alignment, Color, Element, Length, Size, Subscription, Task, stream, window};

use sola_core::state_file::StateFile;
use sola_kit::fonts;
use sola_kvm::protocol::Packet;
use sola_kvm::view_wire;

use crate::APP_ID;
use crate::inhibit::{self, Inhibitor};
use crate::menu;
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

pub struct Session {
    pub host: String,
    port: u16,
    window: window::Id,
    status: Status,
    display: Option<Display>,
    pictures: Arc<Pictures>,
    outgoing: Option<Sender<Outgoing>>,
    /// Keys the other machine has been told are down.
    held: BTreeSet<u32>,
    /// False once Ctrl+Alt+Esc has handed keyboard and pointer back.
    captured: bool,
    /// Where the other machine's pointer was sent, in its points; `None`
    /// until a move since connecting.
    pointer: Option<(i32, i32)>,
    /// The picture's area, which the host's screen is made to fit; `None`
    /// until laid out, and nothing connects before.
    area: Option<Size>,
    scale: f32,
    focused: bool,
    /// View → Statistics: the window's corner says how the pictures come.
    pub statistics: bool,
    /// View → Mirror: the other machine's own screen, not one of the
    /// window's size. Kept across starts, per machine.
    mirror: bool,
    /// The other machine has a screen of its own on to mirror.
    can_mirror: bool,
    /// The other machine can make a screen of the window's size (a Mac);
    /// a Linux host cannot.
    can_extend: bool,
    /// The pictures are its own screen, as it says: it mirrors when it
    /// could not extend. `None` until it says.
    mirroring: Option<bool>,
    inhibitor: Option<Arc<Inhibitor>>,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Net(net::Event),
    Key {
        physical: Physical,
        pressed: bool,
        repeat: bool,
        modifiers: keyboard::Modifiers,
    },
    Pointer(Packet),
    Area(Size),
    Scale(f32),
    Capture,
    Focused,
    Unfocused,
    Inhibited(Option<Arc<Inhibitor>>),
    TitleDrag,
    TitleResize(window::Direction),
    TitleClose,
}

impl Session {
    pub fn new(host: String, port: u16, window: window::Id) -> Self {
        let mirror = saved_mirror(&host);
        let pictures = Arc::new(Pictures::default());
        pictures.mirror.store(mirror, Ordering::Relaxed);
        Self {
            host,
            port,
            window,
            status: Status::Connecting,
            display: None,
            pictures,
            outgoing: None,
            held: BTreeSet::new(),
            captured: true,
            pointer: None,
            area: None,
            scale: 1.0,
            focused: false,
            statistics: false,
            mirror,
            can_mirror: true,
            can_extend: true,
            mirroring: None,
            inhibitor: None,
        }
    }

    /// The window is up: take the desk's shortcuts, and learn its scale.
    pub fn opened(&self) -> Task<Msg> {
        Task::batch([self.inhibit(), window::scale_factor(self.window).map(Msg::Scale)])
    }

    /// The link to the other machine, once the picture is laid out (its
    /// size goes first). It ends when the window does.
    pub fn subscription(&self) -> Subscription<Msg> {
        if self.area.is_none() {
            return Subscription::none();
        }
        Subscription::run_with(
            Link {
                host: self.host.clone(),
                port: self.port,
                pictures: Arc::clone(&self.pictures),
            },
            run_link,
        )
        .map(Msg::Net)
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Net(event) => self.on_net(event),
            Msg::Key {
                physical,
                pressed,
                repeat,
                modifiers,
            } => return self.on_key(&physical, pressed, repeat, modifiers),
            Msg::Pointer(packet) => {
                if let Packet::Motion { x, y } = packet {
                    self.pointer = Some((x, y));
                }
                self.send(packet);
            }
            Msg::Area(area) => {
                self.area = Some(area);
                self.want_size();
            }
            Msg::Scale(scale) => {
                self.scale = scale;
                self.want_size();
            }
            Msg::Capture => {
                self.captured = true;
                self.clipboard_to_host();
                return self.inhibit();
            }
            Msg::Focused => {
                self.focused = true;
                self.clipboard_to_host();
            }
            Msg::Unfocused => {
                self.focused = false;
                self.release_keys();
                self.clipboard_from_host();
            }
            Msg::Inhibited(inhibitor) => {
                // A release that landed while this was on its way wins.
                if self.captured {
                    self.inhibitor = inhibitor;
                }
            }
            Msg::TitleDrag => return sola_kit::drag(Some(self.window)),
            Msg::TitleResize(direction) => return sola_kit::drag_resize(Some(self.window), direction),
            // The app closes the window (it ends the connection).
            Msg::TitleClose => {}
        }
        Task::none()
    }

    fn on_net(&mut self, event: net::Event) {
        match event {
            net::Event::Ready(outgoing) => self.outgoing = Some(outgoing),
            net::Event::Connected(display) => {
                self.display = Some(display);
                self.pointer = None;
                // Said again after each Display.
                self.mirroring = None;
                self.status = Status::Showing;
                self.clipboard_to_host();
            }
            // The message alone brings the redraw that uploads it.
            net::Event::Picture => self.pictures.drawn(),
            net::Event::Cursor => {}
            net::Event::CanMirror(can) => self.can_mirror = can,
            net::Event::CanExtend(can) => self.can_extend = can,
            net::Event::Mirroring(mirroring) => self.mirroring = Some(mirroring),
            net::Event::Lost(why) => {
                self.status = Status::Lost(why);
                self.held.clear();
            }
            net::Event::Refused(why) => self.status = Status::Refused(why),
        }
    }

    fn on_key(&mut self, physical: &Physical, pressed: bool, repeat: bool, modifiers: keyboard::Modifiers) -> Task<Msg> {
        if !self.captured {
            return Task::none();
        }
        let Some(code) = sola_kit::keys::evdev_of(physical) else {
            return Task::none();
        };
        if pressed && code == ESCAPE_EVDEV && modifiers.control() && modifiers.alt() {
            self.release();
            return Task::none();
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
    fn release(&mut self) {
        self.clipboard_from_host();
        self.captured = false;
        self.inhibitor = None;
        self.held.clear();
        self.send(Packet::Leave);
    }

    /// Coming in: what was copied here is there to paste.
    fn clipboard_to_host(&self) {
        if self.captured && self.focused {
            self.send_out(Outgoing::PushClipboard);
        }
    }

    /// Going out: what was copied there is here to paste.
    fn clipboard_from_host(&self) {
        if self.captured {
            self.send_out(Outgoing::PullClipboard);
        }
    }

    /// Mirror the other machine's screen, or extend it, now and from now on.
    pub fn set_mirror(&mut self, mirror: bool) {
        self.mirror = mirror;
        let mut saved = mirror_file().load::<BTreeMap<String, bool>>().unwrap_or_default();
        saved.insert(self.host.clone(), mirror);
        mirror_file().save(&saved);
        self.pictures.mirror.store(mirror, Ordering::Relaxed);
        self.send_out(Outgoing::Mirror(mirror));
    }

    /// The View menu as this window has it.
    pub fn menu(&self) -> menu::View {
        menu::View {
            statistics: self.statistics,
            mirrored: self.mirroring.unwrap_or(self.mirror && self.can_mirror),
            can_mirror: self.can_mirror,
            can_extend: self.can_extend,
        }
    }

    /// The host's screen at the picture's area, when it can.
    fn want_size(&self) {
        let Some(area) = self.area else {
            return;
        };
        let size = view_wire::Size {
            width: area.width as u32,
            height: area.height as u32,
            scale_percent: (self.scale * 100.0).round() as u16,
        };
        if self.pictures.want(size) {
            self.send_out(Outgoing::Size(size));
        }
    }

    fn release_keys(&mut self) {
        for keycode in std::mem::take(&mut self.held) {
            self.send(Packet::Key { keycode, pressed: 0 });
        }
    }

    fn inhibit(&self) -> Task<Msg> {
        window::run(self.window, |window| inhibit::acquire(window).map(Arc::new)).map(Msg::Inhibited)
    }

    fn send(&self, packet: Packet) {
        self.send_out(Outgoing::Packet(packet));
    }

    fn send_out(&self, out: Outgoing) {
        if let Some(outgoing) = &self.outgoing {
            let _ = outgoing.send(out);
        }
    }

    pub fn view(&self, chrome: sola_kit::Chrome) -> Element<'_, Msg> {
        let note = match &self.status {
            Status::Showing => None,
            Status::Connecting => Some(format!("Connecting to {}…", self.host)),
            Status::Lost(why) => Some(format!("Lost {}: {why}. Connecting again…", self.host)),
            Status::Refused(why) => Some(why.clone()),
        };
        // The picture is always laid out: its area is the size the host's
        // screen is made, said before connecting.
        let picture = shader(Picture {
            pictures: Arc::clone(&self.pictures),
            points: self.display.map(|d| d.points),
            pointer: self.pointer,
            captured: self.captured,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let over = match note {
            Some(note) => Some(center(panel(text(note).color(Color::WHITE))).into()),
            None => self.statistics.then(|| self.statistics_panel()),
        };
        let body = Stack::new().push(picture).extend(over);
        let body = container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::background(Color::BLACK));
        sola_kit::window_frame(
            chrome,
            self.host.clone(),
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            body.into(),
        )
    }

    /// The last second of pictures, in a corner over the picture.
    fn statistics_panel(&self) -> Element<'_, Msg> {
        let words = match self.pictures.stats.last() {
            (_, Some(second)) => second.to_string(),
            (_, None) => "Waiting for a second of pictures…".into(),
        };
        let panel = panel(text(words).font(fonts::mono()).size(12).color(Color::WHITE));
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(8)
            .align_x(Alignment::End)
            .align_y(Alignment::Start)
            .into()
    }
}

fn mirror_file() -> StateFile {
    StateFile::new(APP_ID, "mirror.json")
}

/// View → Mirror as last chosen for `host`; Extend before anything is.
fn saved_mirror(host: &str) -> bool {
    mirror_file()
        .load::<BTreeMap<String, bool>>()
        .and_then(|saved| saved.get(host).copied())
        .unwrap_or(false)
}

/// Words over the picture, on a dark ground.
fn panel<'a>(words: impl Into<Element<'a, Msg>>) -> container::Container<'a, Msg> {
    container(words)
        .padding(8)
        .style(|_| container::background(Color::from_rgba(0.0, 0.0, 0.0, 0.7)))
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

/// The link's events, until iced drops the stream (the window closed):
/// then its threads stop with it.
fn run_link(link: &Link) -> impl iced::futures::Stream<Item = net::Event> + use<> {
    let (host, port, pictures) = (link.host.clone(), link.port, Arc::clone(&link.pictures));
    stream::channel(64, async move |mut output: iced::futures::channel::mpsc::Sender<net::Event>| {
        let (tx, mut rx) = iced::futures::channel::mpsc::unbounded();
        let _running = net::start(
            host,
            port,
            pictures,
            Arc::new(move |event| {
                let _ = tx.unbounded_send(event);
            }),
            None,
        );
        while let Some(event) = rx.next().await {
            if output.send(event).await.is_err() {
                return;
            }
        }
    })
}
