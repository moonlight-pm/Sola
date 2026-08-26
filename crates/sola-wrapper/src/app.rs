//! Minimal kit chrome + CEF page. Not sola-browser.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::event;
use iced::keyboard;
use iced::widget::{Shader, column, container};
use iced::{Element, Event, Length, Subscription, Task, Theme};

use sola_bus::Message;
use sola_bus::topics::{Application, FocusTarget, Topic, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus, bus_subscription, is_self_quit, window_settings_transparent,
};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use sola_browser::app::{VIEW_H, VIEW_W};
use sola_browser::cef::page_ime::page_ime;
use sola_browser::engine::{Cmd, EditCmd, Engine, FrameSlot, TabId};
use sola_browser::run::frame_subscription;
use sola_browser::{CefEngine, Msg as BrowserMsg};

use crate::edit;
use crate::instance;

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
    /// Result of `iced::clipboard::read` for paste into page content.
    PagePasted(Option<String>),
    Ignore,
}

pub fn run(app_id: &'static str, spec: Application) -> iced::Result {
    BusSetup::new(app_id)
        .subscribe(TopicKind::ALL)
        .app_menu(&spec.label, [("quit", "Quit", KeyCode::Q.meta())])
        .app_menu_more("Edit", edit::MENU_ITEMS)
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
    engine: CefEngine,
    slot: Arc<FrameSlot<CefEngine>>,
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
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
            engine,
            slot,
            theme: default_theme(),
            float: sola_kit::FloatState::new(app_id),
            window_id: None,
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
            chrome_drain_subscription(),
            event::listen_with(|event, _status, _| {
                if let Event::Keyboard(keyboard::Event::ModifiersChanged(m)) = event {
                    sola_browser::input::store_modifiers(m);
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
                if let Some(Topic::MenuAction(m)) = Topic::parse(&message) {
                    if m.app_id == self.app_id {
                        return self.on_edit_action(&m.action_id);
                    }
                }
            }
            Msg::NewFrame => {
                self.slot.redraw_queued.store(false, Ordering::Release);
                self.slot.pumping.store(true, Ordering::Release);
            }
            Msg::WebViewFocused => {
                let _ = self.slot.cmd_tx.send(Cmd::Focus(true));
            }
            Msg::Tick => {
                sola_browser::chrome_wake::take_queued();
                let clip = self.take_page_clipboard();
                if instance::try_recv_activate() {
                    return Task::batch([clip, self.raise()]);
                }
                return clip;
            }
            Msg::PagePasted(text) => {
                let Some(s) = sola_browser::util::usable_clipboard_text(text) else {
                    return Task::none();
                };
                let _ = self.slot.cmd_tx.send(Cmd::PasteText(s.clone()));
                return iced::clipboard::write(s);
            }
            Msg::WindowReady(id) => self.window_id = id,
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => {
                sola_kit::close_app(self.app_id);
            }
            Msg::Ignore => {}
        }
        Task::none()
    }

    fn on_edit_action(&self, action_id: &str) -> Task<Msg> {
        let Some(cmd) = edit::edit_cmd_for_action(action_id) else {
            return Task::none();
        };
        tracing::debug!(?cmd, "edit → page");
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
                let _ = self.slot.cmd_tx.send(Cmd::Edit(cmd));
                Task::none()
            }
            EditCmd::Undo | EditCmd::Redo => Task::none(),
        }
    }

    fn take_page_clipboard(&self) -> Task<Msg> {
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

        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            self.label.as_str(),
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            content,
        )
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.engine.shutdown();
    }
}

fn chrome_drain_subscription() -> Subscription<Msg> {
    Subscription::run(chrome_drain_stream)
}

fn chrome_drain_stream() -> impl iced::futures::Stream<Item = Msg> {
    use iced::futures::StreamExt;
    let (tx, rx) = iced::futures::channel::mpsc::unbounded();
    sola_browser::chrome_wake::install_tx(tx);
    rx.map(|()| Msg::Tick)
}

fn map_browser_msg(m: BrowserMsg) -> Msg {
    match m {
        BrowserMsg::NewFrame => Msg::NewFrame,
        BrowserMsg::WebViewFocused => Msg::WebViewFocused,
        _ => Msg::Ignore,
    }
}
