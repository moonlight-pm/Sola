//! Minimal kit chrome + CEF page. Not sola-browser.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::event;
use iced::keyboard;
use iced::widget::{Shader, Space, column, container, mouse_area, row, stack, text};
use iced::{Alignment, Element, Event, Length, Subscription, Task, Theme};

use sola_bus::Message;
use sola_bus::topics::{Application, FocusTarget, Topic, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, window_settings_transparent,
};
use sola_kit::components::button as kit_button;
use sola_kit::components::card;
use sola_kit::components::style::{SPACE_MD, SPACE_SM};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use sola_browser::app::{VIEW_H, VIEW_W};
use sola_browser::cef::page_ime::page_ime;
use sola_browser::engine::{Cmd, EditCmd, Engine, FrameSlot, TabId};
use sola_browser::run::frame_subscription;
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
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
    PagePasted(Option<String>),
    NotifyAllow,
    NotifyBlock,
    MediaAllow,
    MediaBlock,
    Ignore,
}

pub fn run(app_id: &'static str, spec: Application) -> iced::Result {
    BusSetup::new(app_id)
        .subscribe(TopicKind::ALL)
        .app_menu(&spec.label, [("quit", "Quit", KeyCode::Q.meta())])
        .app_menu("Edit", sola_browser::integration::EDIT_MENU_ITEMS)
        .install();

    let url = spec.url.clone().unwrap_or_default();
    let engine = CefEngine::spawn(app_id, &url, VIEW_W, VIEW_H);
    let cmd_tx = engine.cmd_sender();
    let cursor = engine.cursor_handle();
    let active = engine.active_tab_handle();
    let slot = Arc::new(FrameSlot::<CefEngine> {
        pending: Mutex::new(None),
        cmd_tx: cmd_tx.clone(),
        last_size: Mutex::new((VIEW_W, VIEW_H)),
        cursor,
        paint_tab: AtomicU64::new(u64::MAX),
        need_park_prime: Mutex::new(HashSet::new()),
        drop_paint_tabs: Mutex::new(Vec::new()),
        parked_frames: Mutex::new(HashMap::new()),
        blank_content: AtomicBool::new(false),
        redraw_queued: AtomicBool::new(false),
        pumping: AtomicBool::new(false),
        last_frame_ms: AtomicU64::new(0),
        ime: engine.ime_handle(),
    });
    let paint = TabId(active.load(Ordering::Relaxed));
    if paint.0 != 0 && paint.0 != u64::MAX {
        slot.present_tab(paint);
    }

    let engine_cell = std::cell::Cell::new(Some(engine));
    let slot_cell = std::cell::Cell::new(Some(slot));
    let spec_cell = std::cell::Cell::new(Some(spec));

    let mut settings = window_settings_transparent(app_id);
    settings.size = iced::Size::new(1100.0, 760.0);

    iced::application(
        move || {
            let engine = engine_cell.take().expect("wrapper init once");
            let slot = slot_cell.take().expect("wrapper init once");
            let spec = spec_cell.take().expect("wrapper init once");
            let app = App::new(app_id, spec, engine, slot);
            (app, sola_kit::window_ready_task(Msg::WindowReady))
        },
        App::update,
        App::view,
    )
    .title(App::title)
    .subscription(App::subscription)
    .theme(App::theme)
    .default_font(fonts::ui())
    .window(settings)
    .run()
}

struct App {
    app_id: &'static str,
    label: String,
    start_url: String,
    engine: CefEngine,
    slot: Arc<FrameSlot<CefEngine>>,
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
    pending_notify: Option<sola_browser::notify::IpcPerm>,
    pending_media: Option<sola_browser::media::IpcMedia>,
}

impl App {
    fn new(
        app_id: &'static str,
        spec: Application,
        engine: CefEngine,
        slot: Arc<FrameSlot<CefEngine>>,
    ) -> Self {
        Self {
            app_id,
            label: spec.label,
            start_url: spec.url.clone().unwrap_or_default(),
            engine,
            slot,
            theme: default_theme(),
            float: sola_kit::FloatState::new(app_id),
            window_id: None,
            pending_notify: None,
            pending_media: None,
        }
    }

    fn title(&self) -> String {
        self.label.clone()
    }

    fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            frame_subscription::<CefEngine>(
                self.engine.frames(),
                self.slot.clone(),
                self.engine.active_tab_handle(),
            )
            .map(map_browser_msg),
            bus_subscription().map(Msg::Bus),
            iced::time::every(Duration::from_millis(250)).map(|_| Msg::Tick),
            event::listen_with(|event, _status, _| {
                match event {
                    Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                        sola_browser::input::store_modifiers(m);
                    }
                    Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                        sola_browser::input::store_modifiers(modifiers);
                        if sola_browser::input::is_super_key(&key) {
                            sola_browser::input::note_super_key(true);
                        }
                    }
                    Event::Keyboard(keyboard::Event::KeyReleased { key, modifiers, .. }) => {
                        sola_browser::input::store_modifiers(modifiers);
                        if sola_browser::input::is_super_key(&key) {
                            sola_browser::input::note_super_key(false);
                        }
                    }
                    _ => {}
                }
                None
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
                        return self.raise();
                    }
                    _ => {}
                }
            }
            Msg::NewFrame => {
                self.sync_engine_tab();
                self.slot.redraw_queued.store(false, Ordering::Release);
                self.slot.pumping.store(true, Ordering::Release);
            }
            Msg::WebViewFocused => {
                let _ = self.slot.cmd_tx.send(Cmd::Focus(true));
            }
            Msg::Tick => {
                self.sync_engine_tab();
                self.drain_notify_ipc();
                self.drain_outbound_links();
                let clip = self.take_page_clipboard();
                if instance::try_recv_activate() {
                    return Task::batch([clip, self.raise()]);
                }
                return clip;
            }
            Msg::WindowReady(id) => self.window_id = id,
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => {
                sola_kit::close_app(self.app_id);
            }
            Msg::PagePasted(text) => {
                let Some(s) = sola_browser::util::usable_clipboard_text(text) else {
                    return Task::none();
                };
                let _ = self.slot.cmd_tx.send(Cmd::PasteText(s.clone()));
                return iced::clipboard::write(s);
            }
            Msg::NotifyAllow => self.resolve_notify_permission("granted"),
            Msg::NotifyBlock => self.resolve_notify_permission("denied"),
            Msg::MediaAllow => self.resolve_media_permission("granted"),
            Msg::MediaBlock => self.resolve_media_permission("denied"),
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
        // Same pipe as sola-browser: iced owns the Wayland seat. CEF
        // `paste()` after a chrome read hits an empty clipboard.
        match cmd {
            EditCmd::Paste => iced::clipboard::read().map(Msg::PagePasted),
            EditCmd::Copy | EditCmd::Cut => {
                let _ = self.slot.cmd_tx.send(Cmd::EvaluateJs(
                    sola_browser::paste_js::copy_selection_script(),
                ));
                if cmd == EditCmd::Cut {
                    let _ = self.slot.cmd_tx.send(Cmd::Edit(EditCmd::Cut));
                }
                Task::none()
            }
            EditCmd::SelectAll => {
                let _ = self.slot.cmd_tx.send(Cmd::Edit(EditCmd::SelectAll));
                Task::none()
            }
            EditCmd::Undo | EditCmd::Redo => Task::none(),
        }
    }

    fn sync_engine_tab(&mut self) {
        let id = self.engine.active_tab_handle().load(Ordering::Relaxed);
        if id == 0 || id == u64::MAX {
            return;
        }
        if self.slot.paint_tab.load(Ordering::Relaxed) != id {
            tracing::info!(tab = id, "wrapper: follow CEF tab (popup/huddle)");
            self.slot.present_tab(TabId(id));
        }
    }

    fn drain_outbound_links(&mut self) {
        let urls: Vec<String> = self
            .engine
            .background_tabs_handle()
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        for url in urls {
            match links::classify(&self.start_url, &url) {
                links::LinkAction::Browser => links::open_in_browser(&url),
                links::LinkAction::InApp => {
                    tracing::debug!(%url, "wrapper: same-site popup stays in-app");
                }
                links::LinkAction::Ignore => {
                    tracing::debug!(%url, "wrapper: ignore popup url");
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
                        let _ = self.slot.cmd_tx.send(Cmd::EvaluateJs(
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
            sola_browser::media::send_resolve(&self.slot.cmd_tx, &m, known == "granted");
            return;
        }
        if let Some(pending) = self.pending_media.as_mut() {
            if sola_browser::notify::canon_origin(&pending.origin)
                == sola_browser::notify::canon_origin(&m.origin)
            {
                sola_browser::media::merge(pending, &m);
                return;
            }
            sola_browser::media::send_resolve(&self.slot.cmd_tx, &m, false);
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
        sola_browser::media::send_resolve(&self.slot.cmd_tx, &perm, result == "granted");
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
        let _ = self.slot.cmd_tx.send(Cmd::NotifyPermission {
            prompt_id: perm.req_id,
            granted,
        });
        let _ = self
            .slot
            .cmd_tx
            .send(Cmd::EvaluateJs(sola_browser::notify::resolve_script(
                perm.req_id,
                result,
            )));
    }

    fn raise(&self) -> Task<Msg> {
        if let Some(window_id) = self.float.any_window_id() {
            if let Ok(mut b) = bus().lock() {
                let _ = b.emit(Topic::Focus(FocusTarget { window_id }));
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        let webview = Shader::new(CefEngine::make_program(self.slot.clone()))
            .width(Length::Fill)
            .height(Length::Fill);
        let webview = page_ime(webview, self.slot.clone(), true);
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

        let content: Element<'_, Msg> = if self.pending_media.is_some() {
            stack![content, self.view_media_permission()].into()
        } else if self.pending_notify.is_some() {
            stack![content, self.view_notify_permission()].into()
        } else {
            content
        };

        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            self.label.as_str(),
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
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
