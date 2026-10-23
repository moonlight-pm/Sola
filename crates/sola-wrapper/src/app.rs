//! Minimal kit chrome + CEF page. Not sola-browser.
//!
//! A window per page (Next): the site's page is the app's window; each
//! popup it opens (a huddle, a same-site `window.open`) gets a window of
//! its own — a tab in the shell's bar — instead of replacing the view.
//! Closing a popup's window closes the popup; closing the site's quits.

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use iced::event;
use iced::keyboard;
use iced::widget::{Shader, Space, column, container, mouse_area, row, stack, text};
use iced::{Alignment, Element, Event, Length, Subscription, Task, Theme, window};

use sola_bus::Message;
use sola_bus::topics::{Application, Topic, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit};
use sola_kit::components::button as kit_button;
use sola_kit::components::card;
use sola_kit::components::style::{SPACE_MD, SPACE_SM};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use sola_browser::app::{VIEW_H, VIEW_W};
use sola_browser::cef::page_ime::page_ime;
use sola_browser::engine::{Cmd, EditCmd, Engine, FrameSlot, TabId, TabInfo};
use sola_browser::page_windows::PageWindows;
use sola_browser::run::frame_subscription_slots;
use sola_browser::{CefEngine, Msg as BrowserMsg};

use crate::instance;
use crate::links;
use crate::menu;

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    NewFrame,
    WebViewFocused,
    Tick,
    /// A page window's focus and close events.
    Window(window::Id, window::Event),
    TitleDrag(window::Id),
    TitleResize(window::Id, window::Direction),
    TitleClose(window::Id),
    PagePasted(Option<String>),
    PageOffer(sola_kit::clipboard::Offer),
    NotifyAllow,
    NotifyBlock,
    MediaAllow,
    MediaBlock,
    JsDialogOk,
    JsDialogCancel,
    JsDialogInput(String),
    Ignore,
}

pub fn run(app_id: &'static str, spec: Application) -> iced::Result {
    BusSetup::new(app_id)
        .subscribe(TopicKind::ALL)
        .app_menu(&spec.label, [("quit", "Quit", KeyCode::Q.meta())])
        .app_menu("Edit", sola_browser::integration::EDIT_MENU_ITEMS)
        .install();

    let url = spec.url.clone().unwrap_or_default();
    // The engine opens the site at once and names its tab: the main page.
    let engine = CefEngine::spawn(app_id, &url, VIEW_W, VIEW_H);
    let main = TabId(engine.active_tab_handle().load(Ordering::Relaxed));
    // Page windows take their command channel, cursor and IME from this.
    let template = Arc::new(FrameSlot::<CefEngine>::new(
        engine.cmd_sender(),
        engine.cursor_handle(),
        engine.ime_handle(),
        VIEW_W,
        VIEW_H,
    ));

    let engine_cell = std::cell::Cell::new(Some(engine));
    let template_cell = std::cell::Cell::new(Some(template));
    let spec_cell = std::cell::Cell::new(Some(spec));

    iced::daemon(
        move || {
            let engine = engine_cell.take().expect("wrapper init once");
            let template = template_cell.take().expect("wrapper init once");
            let spec = spec_cell.take().expect("wrapper init once");
            App::new(app_id, spec, engine, template, main)
        },
        App::update,
        App::view,
    )
    .title(App::title)
    .subscription(App::subscription)
    .theme(App::theme)
    .default_font(fonts::ui())
    .run()
}

struct App {
    app_id: &'static str,
    label: String,
    start_url: String,
    engine: CefEngine,
    /// A window per page: the site's, and one per popup.
    windows: PageWindows<CefEngine>,
    /// The site's own page. Its window is the app's: closing it quits.
    main: TabId,
    /// The engine has listed the main page (until then it may be missing
    /// from the list without having closed).
    main_listed: bool,
    /// The engine's pages, as last read.
    tabs: Vec<TabInfo>,
    /// Pages whose window we closed, until the engine stops listing them.
    closed: HashSet<TabId>,
    /// The page last brought up (the engine's active page).
    shown_active: TabId,
    theme: Theme,
    float: sola_kit::FloatState,
    pending_notify: Option<sola_browser::notify::IpcPerm>,
    pending_media: Option<sola_browser::media::IpcMedia>,
    pending_js_dialog: Option<sola_browser::js_dialog::Ipc>,
    js_dialog_queue: std::collections::VecDeque<sola_browser::js_dialog::Ipc>,
    js_dialog_prompt: String,
}

impl App {
    /// Open the site's window at once (its page is loading).
    fn new(
        app_id: &'static str,
        spec: Application,
        engine: CefEngine,
        template: Arc<FrameSlot<CefEngine>>,
        main: TabId,
    ) -> (Self, Task<Msg>) {
        let mut windows = PageWindows::new(app_id, template);
        let open = windows.open(app_id, main, "main", false).discard();
        let app = Self {
            app_id,
            label: spec.label,
            start_url: spec.url.clone().unwrap_or_default(),
            engine,
            windows,
            main,
            main_listed: false,
            tabs: Vec::new(),
            closed: HashSet::new(),
            shown_active: main,
            theme: default_theme(),
            float: sola_kit::FloatState::new(app_id),
            pending_notify: None,
            pending_media: None,
            pending_js_dialog: None,
            js_dialog_queue: std::collections::VecDeque::new(),
            js_dialog_prompt: String::new(),
        };
        (app, open)
    }

    /// A window's title: the site's name for its own window, the page's
    /// title for a popup (the shell's bar row).
    fn title(&self, window: window::Id) -> String {
        let tab = self.windows.tab_of(window);
        if tab == Some(self.main) {
            return self.label.clone();
        }
        tab.and_then(|id| self.tabs.iter().find(|t| t.id == id))
            .map(|t| t.title.trim())
            .filter(|t| !t.is_empty())
            .map_or_else(|| self.label.clone(), str::to_string)
    }

    fn theme(&self, window: window::Id) -> Theme {
        let floating = self
            .windows
            .river_id(window)
            .is_some_and(|id| self.float.is_window_floating(id));
        sola_kit::theme_for(floating, &self.theme)
    }

    fn chrome(&self, window: window::Id) -> sola_kit::Chrome {
        self.windows
            .river_id(window)
            .map_or(sola_kit::Chrome::Tiled, |id| self.float.chrome(id))
    }

    /// Engine commands go through the shared channel.
    fn send(&self, cmd: Cmd<CefEngine>) {
        let _ = self.engine.cmd_sender().send(cmd);
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            frame_subscription_slots::<CefEngine>(
                self.engine.frames(),
                self.windows.slots(),
                self.engine.active_tab_handle(),
            )
            .map(map_browser_msg),
            bus_subscription().map(Msg::Bus),
            iced::time::every(Duration::from_millis(250)).map(|_| Msg::Tick),
            event::listen_with(|event, _status, id| {
                if let Event::Window(
                    e @ (window::Event::Focused
                    | window::Event::CloseRequested
                    | window::Event::Closed),
                ) = event
                {
                    return Some(Msg::Window(id, e));
                }
                match &event {
                    Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                        sola_browser::input::store_modifiers(*m);
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                        sola_browser::input::store_modifiers(*modifiers);
                        if sola_browser::input::is_super_key(key) {
                            sola_browser::input::note_super_key(true);
                        }
                    }
                    Event::Keyboard(keyboard::Event::KeyReleased { key, modifiers, .. }) => {
                        sola_browser::input::store_modifiers(*modifiers);
                        if sola_browser::input::is_super_key(key) {
                            sola_browser::input::note_super_key(false);
                        }
                    }
                    _ => {}
                }
                match event {
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Escape),
                        ..
                    }) if sola_browser::js_dialog::is_open() => {
                        Some(if sola_browser::js_dialog::is_alert() {
                            Msg::JsDialogOk
                        } else {
                            Msg::JsDialogCancel
                        })
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Enter),
                        ..
                    }) if sola_browser::js_dialog::is_open()
                        && !sola_browser::js_dialog::is_prompt() =>
                    {
                        Some(Msg::JsDialogOk)
                    }
                    _ => None,
                }
            }),
        ])
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => {
                self.float.update(&message);
                apply_theme_update(&message, &mut self.theme);
                if is_self_quit(&message, self.app_id) {
                    return iced::exit();
                }
                match Topic::parse(&message) {
                    Some(Topic::MenuAction(m)) if m.app_id == self.app_id => {
                        return self.run_menu_action(&m.action_id);
                    }
                    Some(Topic::Chord(c)) => {
                        sola_browser::input::apply_super_chord(true, c.keysym);
                    }
                    Some(Topic::ChordReleased(c)) => {
                        sola_browser::input::apply_super_chord(false, c.keysym);
                    }
                    Some(Topic::NotificationActivate(a)) if a.app_id == self.app_id => {
                        self.raise();
                    }
                    _ => {}
                }
            }
            Msg::NewFrame => {
                // Every window redraws after an update: serve each page
                // window's slot that queued a kick.
                for slot in self.windows.slots().lock().unwrap().iter() {
                    if slot.redraw_queued.swap(false, Ordering::AcqRel) {
                        slot.pumping.store(true, Ordering::Release);
                    }
                }
                return self.sync_windows();
            }
            Msg::WebViewFocused => self.send(Cmd::Focus(true)),
            Msg::Tick => {
                let sync = self.sync_windows();
                self.drain_notify_ipc();
                let js_dlg = self.drain_js_dialogs();
                self.drain_outbound_links();
                let clip = self.take_page_clipboard();
                if instance::try_recv_activate() {
                    self.raise();
                }
                return Task::batch([sync, clip, js_dlg]);
            }
            Msg::Window(window, event) => return self.on_window(window, event),
            Msg::TitleDrag(window) => return window::drag(window),
            Msg::TitleResize(window, dir) => return window::drag_resize(window, dir),
            Msg::TitleClose(window) => return self.close_window(window),
            Msg::PageOffer(offer) => {
                use sola_kit::clipboard::Offer;
                return match offer {
                    Offer::Empty => iced::clipboard::read().map(Msg::PagePasted),
                    other => {
                        sola_browser::page_paste::send(&self.engine.cmd_sender(), other);
                        Task::none()
                    }
                };
            }
            Msg::PagePasted(text) => {
                let Some(s) = sola_browser::util::usable_clipboard_text(text) else {
                    return Task::none();
                };
                self.send(Cmd::PasteText(s.clone()));
                return iced::clipboard::write(s);
            }
            Msg::NotifyAllow => self.resolve_notify_permission("granted"),
            Msg::NotifyBlock => self.resolve_notify_permission("denied"),
            Msg::MediaAllow => self.resolve_media_permission("granted"),
            Msg::MediaBlock => self.resolve_media_permission("denied"),
            Msg::JsDialogOk => return self.resolve_js_dialog(true),
            Msg::JsDialogCancel => return self.resolve_js_dialog(false),
            Msg::JsDialogInput(s) => {
                self.js_dialog_prompt = s;
            }
            Msg::Ignore => {}
        }
        Task::none()
    }

    fn run_menu_action(&mut self, action_id: &str) -> Task<Msg> {
        let Some(cmd) = menu::edit_cmd(action_id) else {
            return Task::none();
        };
        tracing::info!(action_id, ?cmd, "edit menu");
        self.run_edit(cmd)
    }

    fn run_edit(&mut self, cmd: EditCmd) -> Task<Msg> {
        // Same pipe as sola-browser: data-control first (image or text).
        // CEF `paste()` after a chrome read hits an empty clipboard.
        match cmd {
            EditCmd::Paste => sola_browser::page_paste::read_task().map(Msg::PageOffer),
            EditCmd::Copy | EditCmd::Cut => {
                self.send(Cmd::EvaluateJs(sola_browser::paste_js::copy_selection_script()));
                if cmd == EditCmd::Cut {
                    self.send(Cmd::Edit(EditCmd::Cut));
                }
                Task::none()
            }
            EditCmd::SelectAll => {
                self.send(Cmd::Edit(EditCmd::SelectAll));
                Task::none()
            }
            EditCmd::Undo | EditCmd::Redo => Task::none(),
        }
    }

    /// Keep a window per page: a popup the page opened (a huddle, a
    /// same-site `window.open`) gets its window in front; a popup that
    /// closed (`window.close`) loses its; the site's page closing ends the
    /// app. The engine paints the pages on screen. A page the engine
    /// brings up from outside its window (a popup opening) is shown.
    fn sync_windows(&mut self) -> Task<Msg> {
        self.tabs = self.engine.tabs_handle().lock().unwrap().clone();
        let listed: HashSet<TabId> = self.tabs.iter().map(|t| t.id).collect();
        self.closed.retain(|id| listed.contains(id));
        let mut tasks = Vec::new();
        for id in &listed {
            if *id == self.main {
                self.main_listed = true;
            } else if !self.windows.has(*id) && !self.closed.contains(id) {
                tracing::info!(tab = id.0, "wrapper: popup gets a window");
                let key = format!("popup-{}", id.0);
                tasks.push(self.windows.open(self.app_id, *id, &key, false).discard());
            }
        }
        for id in self.windows.open_tabs() {
            if id != self.main && !listed.contains(&id) {
                tracing::info!(tab = id.0, "wrapper: popup closed");
                tasks.push(self.windows.close(id).discard());
            }
        }
        if self.main_listed && !listed.contains(&self.main) {
            tracing::info!("wrapper: the site's page closed; quitting");
            return iced::exit();
        }
        for (id, shown) in self.windows.take_shown_changes() {
            self.send(Cmd::ShowPage { id, shown });
        }
        let active = TabId(self.engine.active_tab_handle().load(Ordering::Relaxed));
        if active != self.shown_active && self.windows.has(active) {
            self.shown_active = active;
            self.windows.show(active);
        }
        Task::batch(tasks)
    }

    /// A page window's focus makes its page the active one (keys, edits,
    /// dialogs); its close button or request closes its page.
    fn on_window(&mut self, window: window::Id, event: window::Event) -> Task<Msg> {
        match event {
            window::Event::Focused => {
                self.windows.focused = Some(window);
                if let Some(tab) = self.windows.tab_of(window) {
                    self.activate(tab);
                }
                Task::none()
            }
            window::Event::CloseRequested => self.close_window(window),
            window::Event::Closed => {
                if self.windows.closed(window) == Some(self.main) {
                    return iced::exit();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// Page `tab` takes the keys: the engine's active page.
    fn activate(&mut self, tab: TabId) {
        let active = self.engine.active_tab_handle();
        if active.load(Ordering::Relaxed) == tab.0 {
            return;
        }
        active.store(tab.0, Ordering::Relaxed);
        self.shown_active = tab;
        self.send(Cmd::SetActiveTab(tab));
    }

    /// Close `window`: a popup's closes the popup (the page may say
    /// goodbye — a huddle leaves); the site's quits the app.
    fn close_window(&mut self, window: window::Id) -> Task<Msg> {
        let Some(tab) = self.windows.tab_of(window) else {
            return window::close(window);
        };
        if tab == self.main {
            return iced::exit();
        }
        self.closed.insert(tab);
        self.send(Cmd::CloseTab(tab));
        if self.engine.active_tab_handle().load(Ordering::Relaxed) == tab.0 {
            self.activate(self.main);
        }
        self.windows.close(tab).discard()
    }

    fn drain_outbound_links(&mut self) {
        let urls: Vec<sola_browser::ChromeTabRequest> = self
            .engine
            .background_tabs_handle()
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        for req in urls {
            match links::classify(&self.start_url, &req.url) {
                links::LinkAction::Browser => links::open_in_browser(&req.url),
                links::LinkAction::InApp => {
                    tracing::debug!(url = %req.url, "wrapper: same-site popup stays in-app");
                }
                links::LinkAction::Ignore => {
                    tracing::debug!(url = %req.url, "wrapper: ignore popup url");
                }
            }
        }
    }

    fn take_page_clipboard(&mut self) -> Task<Msg> {
        let Some(text) = self
            .engine
            .clipboard_handle()
            .lock()
            .unwrap()
            .take()
            .and_then(|t| sola_browser::util::usable_clipboard_text(Some(t)))
        else {
            return Task::none();
        };
        tracing::debug!(len = text.len(), "page copy → system clipboard");
        iced::clipboard::write(text)
    }

    fn drain_notify_ipc(&mut self) {
        let evs: Vec<sola_browser::notify::Ipc> = self
            .engine
            .notifications_handle()
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        let profile = sola_browser::profiles::active().id;
        for ev in evs {
            match ev {
                sola_browser::notify::Ipc::Show(show) => {
                    let perm = sola_browser::notify::permission_for(&profile, &show.origin);
                    if perm != "granted" {
                        tracing::info!(
                            origin = %show.origin,
                            %perm,
                            "notify: drop show (origin not granted)"
                        );
                        continue;
                    }
                    tracing::info!(
                        origin = %show.origin,
                        title = %show.title,
                        app_id = self.app_id,
                        "notify: emit AppNotification"
                    );
                    if let Ok(mut bus) = bus().lock() {
                        let _ = bus.emit(Topic::AppNotification(sola_browser::notify::to_bus_for(
                            self.app_id,
                            &show,
                        )));
                    }
                }
                sola_browser::notify::Ipc::Perm(perm) => {
                    tracing::info!(
                        origin = %perm.origin,
                        prompt_id = perm.req_id,
                        "notification permission request"
                    );
                    let known = sola_browser::notify::permission_for(&profile, &perm.origin);
                    if known != "default" {
                        self.send(Cmd::EvaluateJs(
                            sola_browser::notify::resolve_script(perm.req_id, &known),
                        ));
                        continue;
                    }
                    if self.pending_notify.is_none() {
                        self.pending_notify = Some(perm);
                    }
                }
                sola_browser::notify::Ipc::Media(m) => self.on_media_ipc(m),
            }
        }
    }

    fn on_media_ipc(&mut self, m: sola_browser::media::IpcMedia) {
        let profile = sola_browser::profiles::active().id;
        let known = sola_browser::media::permission_for(&profile, &m.origin);
        if known != "default" {
            sola_browser::media::send_resolve(&self.engine.cmd_sender(), &m, known == "granted");
            return;
        }
        if let Some(pending) = self.pending_media.as_mut() {
            if sola_browser::notify::canon_origin(&pending.origin)
                == sola_browser::notify::canon_origin(&m.origin)
            {
                sola_browser::media::merge(pending, &m);
                return;
            }
            sola_browser::media::send_resolve(&self.engine.cmd_sender(), &m, false);
            return;
        }
        tracing::info!(
            origin = %m.origin,
            audio = m.audio,
            video = m.video,
            "media permission request"
        );
        self.pending_media = Some(m);
    }

    fn resolve_media_permission(&mut self, result: &str) {
        let Some(perm) = self.pending_media.take() else {
            return;
        };
        let profile = sola_browser::profiles::active().id;
        if let Err(e) = sola_browser::media::set_permission(&profile, &perm.origin, result) {
            tracing::warn!(error = %e, "media: persist permission failed");
        }
        sola_browser::media::send_resolve(&self.engine.cmd_sender(), &perm, result == "granted");
    }

    fn drain_js_dialogs(&mut self) -> Task<Msg> {
        let evs: Vec<sola_browser::js_dialog::Event> = self
            .engine
            .js_dialogs_handle()
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        let mut focus = Task::none();
        for ev in evs {
            match ev {
                sola_browser::js_dialog::Event::Open(dlg) => {
                    tracing::info!(
                        id = dlg.id,
                        origin = %dlg.origin,
                        kind = ?dlg.kind,
                        "js dialog"
                    );
                    if self.pending_js_dialog.is_none() {
                        focus = self.show_js_dialog(dlg);
                    } else {
                        self.js_dialog_queue.push_back(dlg);
                    }
                }
                sola_browser::js_dialog::Event::Reset { ids } => {
                    let drop = |d: &sola_browser::js_dialog::Ipc| ids.contains(&d.id);
                    self.js_dialog_queue.retain(|d| !drop(d));
                    if self.pending_js_dialog.as_ref().is_some_and(drop) {
                        self.pending_js_dialog = None;
                        self.js_dialog_prompt.clear();
                        if let Some(next) = self.js_dialog_queue.pop_front() {
                            focus = self.show_js_dialog(next);
                        } else {
                            sola_browser::js_dialog::set_open(false);
                        }
                    }
                }
            }
        }
        focus
    }

    fn show_js_dialog(&mut self, dlg: sola_browser::js_dialog::Ipc) -> Task<Msg> {
        sola_browser::js_dialog::set_kind(Some(dlg.kind));
        self.js_dialog_prompt = dlg.default_prompt.clone();
        let prompt = dlg.kind == sola_browser::js_dialog::Kind::Prompt;
        // The dialog shows over its page's window: bring that up.
        if dlg.tab_id != 0 {
            self.windows.show(TabId(dlg.tab_id));
        }
        self.pending_js_dialog = Some(dlg);
        if prompt {
            Task::batch([
                iced::widget::operation::focus(sola_browser::js_dialog::prompt_input_id()),
                iced::advanced::widget::operate(
                    iced::advanced::widget::operation::text_input::select_all::<Msg>(
                        sola_browser::js_dialog::prompt_input_id(),
                    ),
                ),
            ])
        } else {
            iced::advanced::widget::operate(
                iced::advanced::widget::operation::focusable::unfocus::<Msg>(),
            )
        }
    }

    fn resolve_js_dialog(&mut self, success: bool) -> Task<Msg> {
        let Some(dlg) = self.pending_js_dialog.take() else {
            sola_browser::js_dialog::set_open(false);
            return Task::none();
        };
        let input = if dlg.kind == sola_browser::js_dialog::Kind::Prompt {
            std::mem::take(&mut self.js_dialog_prompt)
        } else {
            String::new()
        };
        self.send(Cmd::JsDialog {
            id: dlg.id,
            success,
            input,
        });
        if let Some(next) = self.js_dialog_queue.pop_front() {
            self.show_js_dialog(next)
        } else {
            sola_browser::js_dialog::set_open(false);
            Task::none()
        }
    }

    fn view_js_dialog(&self) -> Element<'_, Msg> {
        let Some(dlg) = self.pending_js_dialog.as_ref() else {
            return Space::new()
                .width(Length::Shrink)
                .height(Length::Shrink)
                .into();
        };
        sola_browser::js_dialog::overlay(
            dlg,
            &self.js_dialog_prompt,
            Msg::JsDialogOk,
            Msg::JsDialogCancel,
            Msg::JsDialogInput,
        )
    }

    fn resolve_notify_permission(&mut self, result: &str) {
        let Some(perm) = self.pending_notify.take() else {
            return;
        };
        let profile = sola_browser::profiles::active().id;
        if let Err(e) = sola_browser::notify::set_permission(&profile, &perm.origin, result) {
            tracing::warn!(error = %e, "notify: persist permission failed");
        }
        let granted = result == "granted";
        self.send(Cmd::NotifyPermission {
            prompt_id: perm.req_id,
            granted,
        });
        self.send(Cmd::EvaluateJs(sola_browser::notify::resolve_script(
            perm.req_id,
            result,
        )));
    }

    /// A second launch or a notification click: bring the site's window
    /// up.
    fn raise(&self) {
        self.windows.show(self.main);
    }

    /// The window a dialog shows in: its page's (a JS dialog names it),
    /// else the active page's.
    fn dialog_tab(&self) -> TabId {
        self.pending_js_dialog
            .as_ref()
            .map(|d| TabId(d.tab_id))
            .filter(|t| t.0 != 0 && self.windows.has(*t))
            .unwrap_or_else(|| TabId(self.engine.active_tab_handle().load(Ordering::Relaxed)))
    }

    fn view(&self, window: window::Id) -> Element<'_, Msg> {
        let tab = self.windows.tab_of(window);
        let Some(slot) = tab.and_then(|t| self.windows.slot_of(t)) else {
            return Space::new().width(Length::Fill).height(Length::Fill).into();
        };
        let dialogs_here = tab == Some(self.dialog_tab())
            || (tab == Some(self.main) && !self.windows.has(self.dialog_tab()));
        let webview = Shader::new(CefEngine::make_program(slot.clone()))
            .width(Length::Fill)
            .height(Length::Fill);
        let webview = page_ime(
            webview,
            slot,
            !(dialogs_here && self.pending_js_dialog.is_some()),
        );
        let webview = Element::from(webview).map(map_browser_msg);

        let canvas = self.theme.extended_palette().background.base.color;
        let canvas = iced::Color { a: 1.0, ..canvas };
        let page: Element<'_, Msg> = container(webview)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_t: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(canvas)),
                ..iced::widget::container::Style::default()
            })
            .into();

        let content: Element<'_, Msg> = column![page]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let content: Element<'_, Msg> = if !dialogs_here {
            content
        } else if self.pending_js_dialog.is_some() {
            stack![content, self.view_js_dialog()].into()
        } else if self.pending_media.is_some() {
            stack![content, self.view_media_permission()].into()
        } else if self.pending_notify.is_some() {
            stack![content, self.view_notify_permission()].into()
        } else {
            content
        };

        sola_kit::window_frame(
            self.chrome(window),
            self.title(window),
            Msg::TitleDrag(window),
            Msg::TitleClose(window),
            move |dir| Msg::TitleResize(window, dir),
            content,
        )
    }

    fn view_notify_permission(&self) -> Element<'_, Msg> {
        let Some(perm) = self.pending_notify.as_ref() else {
            return Space::new()
                .width(Length::Shrink)
                .height(Length::Shrink)
                .into();
        };
        let host = sola_browser::notify::host_of(&perm.origin);
        let title = text("Notifications")
            .size(15)
            .font(sola_kit::fonts::ui_medium());
        let hint = text(format!("{host} wants to show notifications."))
            .size(12)
            .style(|theme: &iced::Theme| {
                let t = theme.extended_palette().background.base.text;
                iced::widget::text::Style {
                    color: Some(iced::Color { a: 0.72, ..t }),
                }
            });
        let actions = row![
            kit_button::labeled("Allow", kit_button::primary).on_press(Msg::NotifyAllow),
            kit_button::labeled("Block", kit_button::ghost).on_press(Msg::NotifyBlock),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);
        let body = column![title, hint, actions]
            .spacing(SPACE_SM)
            .width(Length::Fixed(300.0));
        let panel =
            card::modal(container(body).padding(SPACE_MD + SPACE_SM)).width(Length::Fixed(340.0));
        let backdrop = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_t| {
                iced::widget::container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.0, 0.0, 0.0, 0.22,
                    ))),
                    ..iced::widget::container::Style::default()
                }
            }),
        )
        .on_press(Msg::NotifyBlock);
        let centered = container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center);
        stack![backdrop, centered].into()
    }

    fn view_media_permission(&self) -> Element<'_, Msg> {
        let Some(perm) = self.pending_media.as_ref() else {
            return Space::new()
                .width(Length::Shrink)
                .height(Length::Shrink)
                .into();
        };
        let (title_s, hint_s) = sola_browser::media::copy(perm);
        let title = text(title_s).size(15).font(sola_kit::fonts::ui_medium());
        let hint = text(hint_s).size(12).style(|theme: &iced::Theme| {
            let t = theme.extended_palette().background.base.text;
            iced::widget::text::Style {
                color: Some(iced::Color { a: 0.72, ..t }),
            }
        });
        let actions = row![
            kit_button::labeled("Allow", kit_button::primary).on_press(Msg::MediaAllow),
            kit_button::labeled("Block", kit_button::ghost).on_press(Msg::MediaBlock),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);
        let body = column![title, hint, actions]
            .spacing(SPACE_SM)
            .width(Length::Fixed(300.0));
        let panel =
            card::modal(container(body).padding(SPACE_MD + SPACE_SM)).width(Length::Fixed(340.0));
        let backdrop = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_t| {
                iced::widget::container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgba(
                        0.0, 0.0, 0.0, 0.22,
                    ))),
                    ..iced::widget::container::Style::default()
                }
            }),
        )
        .on_press(Msg::MediaBlock);
        let centered = container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center);
        stack![backdrop, centered].into()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.engine.shutdown();
    }
}

fn map_browser_msg(m: BrowserMsg) -> Msg {
    match m {
        BrowserMsg::NewFrame => Msg::NewFrame,
        BrowserMsg::WebViewFocused => Msg::WebViewFocused,
        _ => Msg::Ignore,
    }
}
