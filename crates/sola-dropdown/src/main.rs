//! The drop-down terminal over an app that fills the screen
//! (`Topic::DropDown`, `solactl dropdown`). The shell starts it with the
//! app's key, as `sola-dropdown -- sh -c <command>`, places its window
//! across the top of the screen over the app and gives it the keyboard; the
//! next press closes it. One window, the command run in a pty sized to it,
//! its background letting the app show through. Closing it hangs up the
//! command; the command ending closes it.

use std::sync::Arc;

use iced::widget::{Space, canvas};
use iced::{Element, Event, Length, Subscription, Task, Theme, event, keyboard, window};
use sola_bus::topics::{Topic, TopicKind};
use sola_kit::app::{
    BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup,
    window_settings_transparent,
};
use sola_kit::fonts;
use sola_kit::theme::{Atoms, atoms_from_bus_theme, default_theme};
use sola_terminal::emulator::{self, Emulator, Listener};
use sola_terminal::pty::{self, PtyBackend};
use sola_terminal::term_view::{self, CellMetrics, Palette};
use sola_terminal::{extkeys, input};

const APP_ID: &str = "sola-dropdown";
/// Its window's title, which the shell knows it by, and its pane's id.
const TITLE: &str = "dropdown";
/// Its background's opacity: the app under it shows through.
const OPACITY: f32 = 0.88;
/// The terminal's font size, as Projects'.
const FONT_PX: f32 = 15.0;

fn main() -> iced::Result {
    let argv: Vec<String> = std::env::args().skip_while(|a| a != "--").skip(1).collect();
    startup(APP_ID);
    if argv.is_empty() {
        tracing::error!("no command: sola-dropdown -- <command>…");
        std::process::exit(2);
    }
    BusSetup::new(APP_ID).app_kinds(&[TopicKind::Theme]).install();
    iced::application(move || App::new(argv.clone()), App::update, App::view)
        .title(|_: &App| TITLE.to_string())
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings_transparent(APP_ID))
        .run()
}

struct Pane {
    emulator: Emulator,
    backend: PtyBackend,
    cache: canvas::Cache,
    grid: (u16, u16),
}

struct App {
    argv: Vec<String>,
    /// Started at the window's first size, so the command starts at it.
    pane: Option<Pane>,
    palette: Palette,
    metrics: CellMetrics,
    theme: Theme,
    keyboard_mods: keyboard::Modifiers,
    keys_held_mods: keyboard::Modifiers,
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<sola_bus::Message>),
    Window(window::Event),
    Input(Event),
    /// The pty wrote: draw again.
    Output,
    /// The command ended.
    Exited,
    /// The selection or the scrollback moved: draw again.
    Redraw,
    /// Bytes for the command (the wheel, the mouse).
    ToPty(Vec<u8>),
    Ignore,
}

impl App {
    fn new(argv: Vec<String>) -> Self {
        Self {
            argv,
            pane: None,
            palette: palette(&Atoms::default()),
            metrics: CellMetrics::for_font(FONT_PX, fonts::mono_metrics()),
            theme: default_theme(),
            keyboard_mods: keyboard::Modifiers::empty(),
            keys_held_mods: keyboard::Modifiers::empty(),
        }
    }

    fn theme(&self) -> Theme {
        sola_kit::theme_for(sola_kit::Chrome::Tiled, &self.theme)
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            emulator::output_subscription().map(|_| Msg::Output),
            emulator::exit_subscription().map(|_| Msg::Exited),
            window::events().map(|(_, e)| Msg::Window(e)),
            event::listen_with(|ev, _, _| match &ev {
                Event::Keyboard(_) => Some(Msg::Input(ev.clone())),
                _ => None,
            }),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => {
                if is_self_quit(&m, APP_ID) {
                    return iced::exit();
                }
                if apply_theme_update(&m, &mut self.theme) {
                    if let Some(Topic::Theme(bus)) = Topic::parse(&m) {
                        self.palette = palette(&atoms_from_bus_theme(&bus));
                        self.metrics = CellMetrics::for_font(FONT_PX, fonts::mono_metrics());
                        if let Some(p) = &self.pane {
                            p.cache.clear();
                        }
                    }
                }
            }
            Msg::Window(window::Event::Resized(size)) => self.sized(size),
            Msg::Window(window::Event::CloseRequested) => return iced::exit(),
            Msg::Window(_) | Msg::Ignore => {}
            Msg::Input(event) => self.key(event),
            Msg::Output | Msg::Redraw => {
                if let Some(p) = &self.pane {
                    p.cache.clear();
                }
            }
            Msg::Exited => {
                tracing::info!("the command ended");
                return iced::exit();
            }
            Msg::ToPty(bytes) => {
                if let Some(p) = &self.pane {
                    p.backend.write(&bytes);
                    p.cache.clear();
                }
            }
        }
        Task::none()
    }

    /// The window has its size: start the command at it, or resize it.
    fn sized(&mut self, size: iced::Size) {
        let (cols, rows) = term_view::cols_rows_for(size, self.metrics);
        if let Some(p) = &mut self.pane {
            if p.grid != (cols, rows) {
                p.emulator.resize(cols, rows);
                p.backend.resize_client(cols, rows);
                p.backend.sigwinch();
                p.grid = (cols, rows);
                p.cache.clear();
            }
            return;
        }
        let listener = Listener::new(
            TITLE.to_string(),
            pty::pty_write_sender(),
            emulator::notify_sender(),
            emulator::title_sender(),
        );
        let emulator = Emulator::new(cols, rows, listener);
        let backend = PtyBackend::spawn_command(
            TITLE,
            &self.argv,
            cols,
            rows,
            emulator.term(),
            emulator.cursor_snap(),
            emulator::notify_sender(),
            emulator::exit_sender(),
        );
        match backend {
            Ok(backend) => {
                tracing::info!(cols, rows, command = ?self.argv, "drop-down terminal");
                self.pane = Some(Pane {
                    emulator,
                    backend,
                    cache: canvas::Cache::default(),
                    grid: (cols, rows),
                });
            }
            Err(e) => {
                tracing::error!(%e, command = ?self.argv, "drop-down terminal: the command did not start");
                std::process::exit(1);
            }
        }
    }

    /// A key: to the command, or Super+V / Super+C (paste, copy).
    fn key(&mut self, event: Event) {
        let Event::Keyboard(event) = event else {
            return;
        };
        let (key, modified_key, physical_key, modifiers, location, text, repeat) = match event {
            keyboard::Event::ModifiersChanged(mods) => {
                self.keyboard_mods = mods;
                return;
            }
            keyboard::Event::KeyReleased { key, physical_key, .. } => {
                self.held(&key, &physical_key, false);
                return;
            }
            keyboard::Event::KeyPressed {
                key,
                modified_key,
                physical_key,
                modifiers,
                location,
                text,
                repeat,
                ..
            } => (key, modified_key, physical_key, modifiers, location, text, repeat),
        };
        self.held(&key, &physical_key, true);
        if modifier_bit(&key, &physical_key).is_some() {
            return;
        }
        let Some(p) = &self.pane else {
            return;
        };
        let modifiers = input::merge_modifiers(modifiers, self.keyboard_mods, self.keys_held_mods);
        let mut mode = { *p.emulator.term().lock().mode() };
        if modifiers.logo() {
            match key.as_ref() {
                keyboard::Key::Character("v") => {
                    if let Some(text) = sola_kit::clipboard::read_string() {
                        p.backend.write(&input::paste(&text, mode));
                    }
                }
                keyboard::Key::Character("c") => {
                    let selected = p.emulator.term().lock().selection_to_string();
                    if let Some(text) = selected.filter(|t| !t.is_empty()) {
                        if let Err(e) = sola_kit::clipboard::write_text(text) {
                            tracing::warn!(%e, "copy");
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        let modify_other_keys = extkeys::level(TITLE) >= 1;
        if modify_other_keys {
            mode |= alacritty_terminal::term::TermMode::DISAMBIGUATE_ESC_CODES;
        }
        let enter = keyboard::Key::Named(keyboard::key::Named::Enter);
        let modified = if key == enter || modified_key == enter { enter } else { modified_key };
        let Some(bytes) = input::resolve_bytes(&input::KeyInput {
            key: &key,
            modified_key: &modified,
            mods: input::Mods::from(modifiers),
            mode,
            location,
            text: text.as_deref(),
            repeat,
            modify_other_keys,
        }) else {
            return;
        };
        {
            let term = p.emulator.term();
            let mut guard = term.lock();
            if guard.grid().display_offset() != 0 {
                guard.scroll_display(alacritty_terminal::grid::Scroll::Bottom);
            }
        }
        p.backend.write(&bytes);
    }

    fn held(&mut self, key: &keyboard::Key, physical: &keyboard::key::Physical, pressed: bool) {
        if let Some(bit) = modifier_bit(key, physical) {
            if pressed {
                self.keys_held_mods |= bit;
            } else {
                self.keys_held_mods &= !bit;
            }
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        let Some(p) = &self.pane else {
            return Space::new().width(Length::Fill).height(Length::Fill).into();
        };
        let view = term_view::TermView {
            term: p.emulator.term(),
            cursor_snap: p.emulator.cursor_snap(),
            selection_track: p.emulator.selection_track(),
            cache: &p.cache,
            palette: &self.palette,
            metrics: self.metrics,
            cursor_on: true,
            active: true,
            on_select: Msg::Redraw,
            on_scroll: Msg::Redraw,
            // Nothing opens over the app: it would come up behind it.
            on_open_url: Box::new(|_| Msg::Ignore),
            on_wheel_pty: Box::new(Msg::ToPty),
            on_mouse_pty: Box::new(Msg::ToPty),
            on_desk_menu: Box::new(|_| Msg::Ignore),
        };
        canvas(view).width(Length::Fill).height(Length::Fill).into()
    }
}

/// The terminal's colours from the theme, its background see-through.
fn palette(atoms: &Atoms) -> Palette {
    let mut palette = Palette::from_kit_theme(atoms);
    palette.bg.a = OPACITY;
    palette
}

fn modifier_bit(
    key: &keyboard::Key,
    physical: &keyboard::key::Physical,
) -> Option<keyboard::Modifiers> {
    use keyboard::key::{Code, Named, Physical};
    if let keyboard::Key::Named(n) = key {
        match n {
            Named::Shift => return Some(keyboard::Modifiers::SHIFT),
            Named::Control => return Some(keyboard::Modifiers::CTRL),
            Named::Alt => return Some(keyboard::Modifiers::ALT),
            Named::Super | Named::Meta => return Some(keyboard::Modifiers::LOGO),
            _ => {}
        }
    }
    match physical {
        Physical::Code(Code::ShiftLeft | Code::ShiftRight) => Some(keyboard::Modifiers::SHIFT),
        Physical::Code(Code::ControlLeft | Code::ControlRight) => Some(keyboard::Modifiers::CTRL),
        Physical::Code(Code::AltLeft | Code::AltRight) => Some(keyboard::Modifiers::ALT),
        Physical::Code(Code::SuperLeft | Code::SuperRight) => Some(keyboard::Modifiers::LOGO),
        _ => None,
    }
}
