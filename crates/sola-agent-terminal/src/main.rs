//! sola-agent-terminal — project / workspace rail + agent-aware PTYs.
//!
//! Status chrome + Grok hooks: reserved marks, hook socket, process-tree
//! presence, OSC 9999. Demo rows remain for scan. Spawn and `sat` later.

use std::sync::Arc;
use std::time::Duration;

use iced::widget::{canvas, container, row};
use iced::{Element, Event, Length, Subscription, Task, Theme};
use iced::{event, keyboard};

use sola_bus::topics::{Topic, TopicKind};
use sola_bus::Message;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, startup,
    window_settings_transparent,
};
use sola_kit::fonts;
use sola_kit::theme::{Atoms, atoms_from_bus_theme, default_theme};
use sola_terminal::emulator::{self, Emulator, Listener};
use sola_terminal::input::{self, Mods};
use sola_terminal::pty::PtyBackend;
use sola_terminal::state::PaneRuntime;
use sola_terminal::term_view::{self, CellMetrics, Palette};
use sola_terminal::{extkeys, links, tmux};

mod hooks;
mod menu;
mod presence;
mod sidebar;
mod status;
mod workspace;

const APP_ID: &str = "sola-agent-terminal";
const WINDOW_TITLE: &str = "Workspaces";

const TMUX_SOCKET: &str = "sola-at";
const TMUX_UNIT: &str = "sola-at-tmux.service";
const TMUX_PREFIX: &str = "sat-";

const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

fn main() -> iced::Result {
    startup(APP_ID);

    // Must precede any tmux helper — do not share sola-terminal's server.
    tmux::configure(TMUX_SOCKET, TMUX_UNIT, TMUX_PREFIX);
    tmux::cleanup_stale_socket();
    tmux::kill_orphaned_clients();
    tmux::ensure_server_running();
    tmux::reload_config();

    BusSetup::new(APP_ID)
        .subscribe(&[
            TopicKind::Theme,
            TopicKind::MenuAction,
            TopicKind::CloseApp,
            TopicKind::Windows,
            TopicKind::WindowFloating,
        ])
        .install();

    if let Ok(mut client) = bus().lock() {
        if let Err(e) = client.emit(Topic::SetAppMenu(menu::app_menu())) {
            tracing::warn!("app-menu publish failed: {e:?}");
        }
    }

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::mono())
        .window(window_settings_transparent(APP_ID))
        .run()
}

struct App {
    project: workspace::Project,
    workspaces: Vec<workspace::Workspace>,
    selected: String,
    pane_id: String,
    runtime: Option<PaneRuntime>,
    theme: Theme,
    palette: Palette,
    sidebar: sidebar::SidebarState,
    window_size: iced::Size,
    metrics: CellMetrics,
    cursor_on: bool,
    keyboard_mods: keyboard::Modifiers,
    keys_held_mods: keyboard::Modifiers,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
    pane_status: status::PaneStatus,
    hook_sock: String,
    /// Previous pane id if we renamed an orphan tmux session onto `ws-main`.
    adopted_from: Option<String>,
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    PtyOutput(String),
    PtyExit(String),
    Title(String, String),
    Input(iced::Event),
    Resized(iced::Size),
    SelectionChanged,
    Scrolled(String),
    OpenUrl(String),
    WheelToPty(String, Vec<u8>),
    #[allow(dead_code)]
    Pasted(Option<String>),
    BlinkTick,
    SidebarDragStart,
    CursorMoved(f32, f32),
    CursorReleased,
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
    SelectWorkspace(String),
    StatusTick,
    Hook(hooks::Incoming),
    Osc(String, sola_terminal::osc9999::OscStatus),
    PresenceTick,
    Noop,
}

impl App {
    fn boot() -> (Self, Task<Msg>) {
        let hook_paths = hooks::start();
        let _ = sola_terminal::osc9999::sender();
        let (project, workspaces) = workspace::seed();
        let selected = workspace::live(&workspaces)
            .map(|w| w.id.clone())
            .unwrap_or_else(|| workspace::LIVE_ID.into());
        let adopted_from = workspace::adopt_orphan_session();
        let pane_id = selected.clone();
        let pane_status = status::hydrate(&selected).unwrap_or_default();
        let mut app = Self {
            project,
            workspaces,
            selected,
            pane_id: pane_id.clone(),
            runtime: None,
            theme: default_theme(),
            palette: Palette::from_kit_theme(&Atoms::default()),
            sidebar: sidebar::SidebarState::default(),
            window_size: iced::Size::new(1100.0, 720.0),
            metrics: CellMetrics::for_font(15.0, fonts::mono_metrics()),
            cursor_on: true,
            keyboard_mods: keyboard::Modifiers::empty(),
            keys_held_mods: keyboard::Modifiers::empty(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            pane_status,
            hook_sock: hook_paths.socket_path.to_string_lossy().into_owned(),
            adopted_from,
        };
        app.sync_live_row();
        let attach = app.attach_pane();
        (
            app,
            Task::batch([sola_kit::window_ready_task(Msg::WindowReady), attach]),
        )
    }

    fn title(&self) -> String {
        WINDOW_TITLE.into()
    }

    fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            emulator::output_subscription().map(Msg::PtyOutput),
            emulator::exit_subscription().map(Msg::PtyExit),
            emulator::title_subscription().map(|(id, t)| Msg::Title(id, t)),
            hooks::subscription().map(Msg::Hook),
            sola_terminal::osc9999::subscription()
                .map(|(id, payload)| Msg::Osc(id, payload)),
            iced::time::every(Duration::from_secs(1)).map(|_| Msg::PresenceTick),
            event::listen_with(|ev, status, _| match &ev {
                Event::Keyboard(_) => Some(Msg::Input(ev)),
                _ if matches!(status, iced::event::Status::Ignored) => Some(Msg::Input(ev)),
                _ => None,
            }),
            iced::window::resize_events().map(|(_id, size)| Msg::Resized(size)),
            iced::time::every(Duration::from_millis(530)).map(|_| Msg::BlinkTick),
            if self
                .workspaces
                .iter()
                .any(|w| w.status == status::AgentStatus::Working)
            {
                iced::window::frames().map(|_| Msg::StatusTick)
            } else {
                Subscription::none()
            },
            event::listen_with(|ev, _, _| match ev {
                Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Msg::CursorMoved(position.x, position.y))
                }
                Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                    Some(Msg::CursorReleased)
                }
                _ => None,
            }),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            Msg::WindowReady(id) => {
                self.window_id = id;
                Task::none()
            }
            Msg::TitleDrag => sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => {
                sola_kit::close_app(APP_ID);
                Task::none()
            }
            Msg::Noop => Task::none(),
            Msg::StatusTick => Task::none(),
            Msg::Hook(incoming) => {
                if self.pane_is(&incoming.pane_id) {
                    self.pane_status.apply_hook(&incoming);
                    status::persist(&self.selected, &self.pane_status);
                    self.sync_live_row();
                }
                Task::none()
            }
            Msg::Osc(id, payload) => {
                if self.pane_is(&id) {
                    self.pane_status.apply_osc(&payload);
                    status::persist(&self.selected, &self.pane_status);
                    self.sync_live_row();
                }
                Task::none()
            }
            Msg::PresenceTick => {
                let tmux_session = tmux::session_name(&self.pane_id);
                let who = presence::scan_session(&tmux_session);
                self.pane_status.apply_presence(who);
                self.sync_live_row();
                Task::none()
            }
            Msg::SelectWorkspace(id) => {
                if self
                    .workspaces
                    .iter()
                    .any(|w| w.id == id && !w.demo)
                {
                    self.selected = id;
                }
                Task::none()
            }
            Msg::PtyOutput(id) => {
                if id == self.pane_id {
                    if let Some(rt) = &self.runtime {
                        rt.cache.clear();
                    }
                }
                Task::none()
            }
            Msg::PtyExit(id) => {
                if id == self.pane_id {
                    tracing::info!("pane PTY exited");
                }
                Task::none()
            }
            Msg::Title(id, title) => {
                if id == self.pane_id {
                    tracing::debug!(%title, "pane title");
                }
                Task::none()
            }
            Msg::BlinkTick => {
                self.cursor_on = !self.cursor_on;
                Task::none()
            }
            Msg::Input(event) => self.on_input(event),
            Msg::Resized(size) => {
                self.window_size = size;
                self.resize_pane();
                Task::none()
            }
            Msg::SelectionChanged => {
                if let Some(rt) = &self.runtime {
                    rt.cache.clear();
                }
                Task::none()
            }
            Msg::Scrolled(id) => {
                if id == self.pane_id {
                    if let Some(rt) = &self.runtime {
                        rt.cache.clear();
                    }
                }
                Task::none()
            }
            Msg::OpenUrl(uri) => {
                links::open_url(&uri);
                Task::none()
            }
            Msg::WheelToPty(_id, bytes) => {
                if let Some(rt) = &self.runtime {
                    rt.backend.write(&bytes);
                }
                Task::none()
            }
            Msg::Pasted(text) => {
                if let (Some(text), Some(rt)) = (text, &self.runtime) {
                    let mode = { *rt.emulator.term().lock().mode() };
                    rt.backend.write(&input::paste(&text, mode));
                }
                Task::none()
            }
            Msg::SidebarDragStart => {
                self.sidebar.dragging_divider = true;
                self.sidebar.drag_anchor = None;
                Task::none()
            }
            Msg::CursorMoved(x, _y) => {
                if self.sidebar.dragging_divider {
                    if let Some((anchor_x, anchor_w)) = self.sidebar.drag_anchor {
                        self.sidebar.width =
                            sola_kit::components::panel_dragged_width(anchor_x, anchor_w, x);
                        self.resize_pane();
                    } else {
                        self.sidebar.drag_anchor = Some((x, self.sidebar.width));
                    }
                }
                Task::none()
            }
            Msg::CursorReleased => {
                self.sidebar.dragging_divider = false;
                self.sidebar.drag_anchor = None;
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        let pane: Element<'_, Msg> = match &self.runtime {
            Some(rt) => {
                let view = term_view::TermView {
                    term: rt.emulator.term(),
                    cursor_snap: rt.emulator.cursor_snap(),
                    cache: &rt.cache,
                    palette: &self.palette,
                    metrics: self.metrics,
                    cursor_on: self.cursor_on,
                    active: true,
                    on_select: Msg::SelectionChanged,
                    on_scroll: Msg::Scrolled(self.pane_id.clone()),
                    on_open_url: Box::new(Msg::OpenUrl),
                    on_wheel_pty: Box::new({
                        let pid = self.pane_id.clone();
                        move |bytes| Msg::WheelToPty(pid.clone(), bytes)
                    }),
                };
                canvas(view).width(Length::Fill).height(Length::Fill).into()
            }
            None => container(
                sola_kit::components::text::body("no pane")
                    .style(sola_kit::components::text::muted),
            )
            .padding(sola_kit::components::style::SPACE_MD)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        };

        let body: Element<'_, Msg> = row![
            sidebar::view(
                &self.sidebar,
                &self.project,
                &self.workspaces,
                &self.selected,
                &self.theme,
                self.palette.bg,
            ),
            pane,
        ]
        .into();

        let bg = self.palette.bg;
        let framed = container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(bg.into()),
                ..container::Style::default()
            });

        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            WINDOW_TITLE,
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            framed.into(),
        )
    }

    fn on_bus(&mut self, m: &Message) -> Task<Msg> {
        self.float.update(m);
        if apply_theme_update(m, &mut self.theme) {
            if let Some(Topic::Theme(bus)) = Topic::parse(m) {
                self.palette = Palette::from_kit_theme(&atoms_from_bus_theme(&bus));
                self.metrics =
                    CellMetrics::for_font(self.metrics.font_size, fonts::mono_metrics());
                self.resize_pane();
            }
            return Task::none();
        }
        if is_self_quit(m, APP_ID) {
            return iced::exit();
        }
        Task::none()
    }

    fn pane_size(&self) -> iced::Size {
        let chrome = self.sidebar.width + sola_kit::components::DIVIDER_HIT_PX;
        iced::Size::new(
            (self.window_size.width - chrome).max(0.0),
            self.window_size.height,
        )
    }

    fn cols_rows(&self) -> (u16, u16) {
        let (c, r) = term_view::cols_rows_for(self.pane_size(), self.metrics);
        (c.max(2), r.max(1))
    }

    fn attach_pane(&mut self) -> Task<Msg> {
        let (cols, rows) = self.cols_rows();
        let cols = if cols == 0 { DEFAULT_COLS } else { cols };
        let rows = if rows == 0 { DEFAULT_ROWS } else { rows };
        let tmux_session = tmux::session_name(&self.pane_id);
        let cwd = workspace::live(&self.workspaces)
            .map(|w| w.path.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into());

        let listener = Listener::new(
            self.pane_id.clone(),
            sola_terminal::pty::pty_write_sender(),
            emulator::notify_sender(),
            emulator::title_sender(),
        );
        let em = Emulator::new(cols, rows, listener);
        let term = em.term();
        let cursor = em.cursor_snap();

        let pane_id = self.pane_id.clone();
        let hook_sock = self.hook_sock.clone();
        let env = [
            ("SOLA_PANE_ID", pane_id.as_str()),
            ("SOLA_AT_HOOKS_SOCK", hook_sock.as_str()),
        ];
        let backend = match PtyBackend::spawn_or_attach_with_env(
            &self.pane_id,
            &tmux_session,
            cols,
            rows,
            Some(&cwd),
            term,
            cursor,
            emulator::notify_sender(),
            emulator::exit_sender(),
            &env,
        ) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("spawn_or_attach failed: {e}");
                return Task::none();
            }
        };
        backend.resize(cols, rows);
        backend.sigwinch();
        self.runtime = Some(PaneRuntime {
            emulator: em,
            backend,
            cache: canvas::Cache::default(),
        });
        Task::none()
    }

    fn pane_is(&self, id: &str) -> bool {
        id == self.pane_id || self.adopted_from.as_deref() == Some(id)
    }

    fn sync_live_row(&mut self) {
        if let Some(ws) = self.workspaces.iter_mut().find(|w| !w.demo) {
            ws.status = self.pane_status.status;
            ws.agent = self.pane_status.agent.clone();
        }
    }

    fn resize_pane(&mut self) {
        let Some(rt) = &self.runtime else {
            return;
        };
        let (cols, rows) = self.cols_rows();
        rt.emulator.resize(cols, rows);
        rt.backend.resize(cols, rows);
        rt.backend.sigwinch();
        rt.cache.clear();
    }

    fn on_input(&mut self, event: iced::Event) -> Task<Msg> {
        if let iced::Event::Keyboard(keyboard::Event::ModifiersChanged(mods)) = event {
            self.keyboard_mods = mods;
            return Task::none();
        }
        if let iced::Event::Keyboard(keyboard::Event::KeyReleased {
            key,
            physical_key,
            ..
        }) = &event
        {
            self.apply_modifier_key(key, physical_key, false);
            return Task::none();
        }

        let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modified_key,
            physical_key,
            modifiers,
            location,
            text,
            repeat,
            ..
        }) = event
        else {
            return Task::none();
        };

        self.apply_modifier_key(&key, &physical_key, true);
        if modifier_key_bit(&key, &physical_key).is_some() {
            return Task::none();
        }

        let modifiers = modifiers | self.keyboard_mods | self.keys_held_mods;
        self.keyboard_mods = modifiers;
        if modifiers.logo() {
            return Task::none();
        }

        let Some(rt) = &self.runtime else {
            return Task::none();
        };
        let mut mode = { *rt.emulator.term().lock().mode() };
        if extkeys::level(&self.pane_id) >= 1 {
            mode |= alacritty_terminal::term::TermMode::DISAMBIGUATE_ESC_CODES;
        }
        let mods = Mods::from(modifiers);
        let enter_key = match (&key, &modified_key) {
            (keyboard::Key::Named(keyboard::key::Named::Enter), _)
            | (_, keyboard::Key::Named(keyboard::key::Named::Enter)) => {
                keyboard::Key::Named(keyboard::key::Named::Enter)
            }
            _ => modified_key.clone(),
        };
        let Some(bytes) = input::resolve_bytes(&input::KeyInput {
            key: &key,
            modified_key: &enter_key,
            mods,
            mode,
            location,
            text: text.as_deref(),
            repeat,
            modify_other_keys: extkeys::level(&self.pane_id) >= 1,
        }) else {
            return Task::none();
        };
        {
            let term = rt.emulator.term();
            let mut guard = term.lock();
            if guard.grid().display_offset() != 0 {
                guard.scroll_display(alacritty_terminal::grid::Scroll::Bottom);
            }
        }
        rt.backend.write(&bytes);
        Task::none()
    }

    fn apply_modifier_key(
        &mut self,
        key: &keyboard::Key,
        physical: &keyboard::key::Physical,
        pressed: bool,
    ) {
        let Some(bit) = modifier_key_bit(key, physical) else {
            return;
        };
        if pressed {
            self.keys_held_mods = self.keys_held_mods | bit;
        } else {
            self.keys_held_mods = self.keys_held_mods & !bit;
        }
    }
}

fn modifier_key_bit(
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
