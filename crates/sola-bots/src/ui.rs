//! Iced Bots window: list + long dialog. Talks HTTP/SSE to sola-botsd.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::event;
use iced::futures::Stream;
use iced::keyboard;
use iced::keyboard::key::Named as NamedKey;
use iced::mouse;
use iced::widget::Id as ScrollId;
use iced::widget::operation;
use iced::widget::scrollable::Viewport;
use iced::widget::text_editor;
use iced::widget::{
    Space, column, container, row, scrollable, stack, text_editor as text_editor_widget,
};
use iced::{Background, Border, Color, Element, Event, Length, Padding, Subscription, Task, Theme};
use serde_json::Value;
use sola_bus::Message;
use sola_bus::topics::{SplitDir, Topic};
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::components::prose::{parse_markdown, parse_plain, prose_selectable};
use sola_kit::components::style::{HAIRLINE_A, RADIUS_LG, SPACE_LG, SPACE_SM, SPACE_XL, mix_white};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{
    DividerColors, SidebarIndicator, SidebarItem, SidebarSection, button as kit_btn, readable,
    split_with,
};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

pub const APP_ID: &str = "sola-bots";

const SIDEBAR_W_MIN: f32 = 180.0;
const SIDEBAR_W_MAX: f32 = 420.0;
const HEADER_H: f32 = 44.0;
const THREAD_RATIO_MIN: f32 = 0.35;
const THREAD_RATIO_MAX: f32 = 0.92;

pub struct App {
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
    window_w: f32,
    window_h: f32,
    bots: Vec<BotRow>,
    selected: Option<String>,
    turns: Vec<(String, String)>,
    body_sel: Option<String>,
    prose_select_all: u64,
    status: String,
    compose: text_editor::Content,
    new_name: String,
    sending: bool,
    sidebar_w: f32,
    /// Fraction of the dialog column (below the header) owned by the thread.
    thread_ratio: f32,
    dragging_sidebar: bool,
    dragging_compose: bool,
    thread_follow: bool,
    thread_unseen: bool,
    thread_fp: String,
}

#[derive(Clone)]
struct BotRow {
    id: String,
    name: String,
    status: String,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleClose,
    TitleResize(iced::window::Direction),
    Sse(SseFrame),
    Listed(Result<Value, String>),
    Transcript(Result<Value, String>),
    Sent(Result<Value, String>),
    OpenUrl(String),
    Created(Result<Value, String>),
    Removed(Result<Value, String>),
    Select(String),
    Rm(String),
    ComposeAction(text_editor::Action),
    Send,
    NewName(String),
    Create,
    SidebarPress,
    ComposePress,
    Pointer(f32, f32),
    PointerUp,
    WindowResized(f32, f32),
    Pasted(Option<String>),
    KeySend,
    Cancel,
    Cancelled(Result<Value, String>),
    Scrolled(Viewport),
    JumpLatest,
    BodySelect(Option<String>),
}

#[derive(Debug, Clone)]
pub enum SseFrame {
    Snapshot(Value),
    Bots(Value),
    Transcript(Value),
    Delta { id: String, text: String },
    Removed { id: String },
    Error(String),
}

impl Default for App {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            window_w: 900.0,
            window_h: 700.0,
            bots: Vec::new(),
            selected: None,
            turns: Vec::new(),
            body_sel: None,
            prose_select_all: 0,
            status: String::new(),
            compose: text_editor::Content::new(),
            new_name: String::new(),
            sending: false,
            sidebar_w: 220.0,
            thread_ratio: 0.78,
            dragging_sidebar: false,
            dragging_compose: false,
            thread_follow: true,
            thread_unseen: false,
            thread_fp: String::new(),
        }
    }
}

impl App {
    pub fn boot() -> (Self, Task<Msg>) {
        (
            Self::default(),
            Task::batch([
                sola_kit::window_ready_task(Msg::WindowReady),
                refresh_list(),
            ]),
        )
    }

    pub fn title(&self) -> String {
        "Bots".into()
    }

    pub fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            iced::Subscription::run(sse_stream).map(Msg::Sse),
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                    if matches!(key, iced::keyboard::Key::Named(NamedKey::Escape)) =>
                {
                    Some(Msg::Cancel)
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::Pointer(position.x, position.y))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::PointerUp)
                }
                Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Msg::WindowResized(size.width, size.height))
                }
                _ => None,
            }),
        ])
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => {
                apply_theme_update(&m, &mut self.theme);
                self.float.update(&m);
                if is_self_quit(&m, APP_ID) {
                    sola_kit::close_app(APP_ID);
                    return iced::exit();
                }
                if let Some(Topic::MenuAction(p)) = Topic::parse(&m) {
                    if p.app_id == APP_ID {
                        return self.on_menu(&p.action_id);
                    }
                }
                Task::none()
            }
            Msg::WindowReady(id) => {
                self.window_id = id;
                match id {
                    Some(id) => {
                        iced::window::size(id).map(|s| Msg::WindowResized(s.width, s.height))
                    }
                    None => Task::none(),
                }
            }
            Msg::TitleDrag => sola_kit::drag(self.window_id),
            Msg::TitleClose => {
                sola_kit::close_app(APP_ID);
                iced::exit()
            }
            Msg::TitleResize(d) => sola_kit::drag_resize(self.window_id, d),
            Msg::Sse(frame) => self.apply_sse(frame),
            Msg::Listed(Ok(v)) => self.apply_bots(&v),
            Msg::Listed(Err(e)) => {
                self.status = e;
                Task::none()
            }
            Msg::OpenUrl(url) => {
                let _ = sola_core::open_url(&url);
                Task::none()
            }
            Msg::Transcript(Ok(v)) => self.apply_transcript(&v),
            Msg::Transcript(Err(e)) => {
                if e.starts_with("unknown bot") {
                    if let Some(id) = &self.selected {
                        if !e.contains(id) {
                            return Task::none();
                        }
                    }
                    self.status.clear();
                    return Task::none();
                }
                self.status = e;
                Task::none()
            }
            Msg::Select(id) => {
                self.selected = Some(id.clone());
                self.turns.clear();
                self.body_sel = None;
                self.status.clear();
                self.thread_follow = true;
                self.thread_unseen = false;
                self.thread_fp.clear();
                refresh_transcript(id)
            }
            Msg::Cancel => {
                let Some(id) = self.live_selected() else {
                    return Task::none();
                };
                if !self.sending {
                    return Task::none();
                }
                rest(
                    move || {
                        crate::client::post(&format!("/bots/{id}/cancel"), &serde_json::json!({}))
                    },
                    Msg::Cancelled,
                )
            }
            Msg::Cancelled(Ok(_)) => {
                self.status.clear();
                Task::none()
            }
            Msg::Cancelled(Err(e)) => {
                self.status = e;
                Task::none()
            }
            Msg::Scrolled(vp) => {
                let y = vp.relative_offset().y;
                let at_end = !y.is_finite() || y >= 0.97;
                if at_end {
                    self.thread_follow = true;
                    self.thread_unseen = false;
                } else {
                    self.thread_follow = false;
                }
                Task::none()
            }
            Msg::JumpLatest => {
                self.thread_follow = true;
                self.thread_unseen = false;
                snap_thread()
            }
            Msg::BodySelect(sel) => {
                self.body_sel = sel;
                Task::none()
            }
            Msg::ComposeAction(action) => {
                self.compose.perform(action);
                Task::none()
            }
            Msg::KeySend | Msg::Send => self.send_compose(),
            Msg::Sent(Ok(_)) => {
                self.status.clear();
                Task::none()
            }
            Msg::Sent(Err(e)) => {
                self.sending = false;
                self.status = e;
                Task::none()
            }
            Msg::NewName(s) => {
                self.new_name = s;
                Task::none()
            }
            Msg::Create => {
                let name = self.new_name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                self.new_name.clear();
                rest(
                    move || crate::client::post("/bots", &serde_json::json!({ "name": name })),
                    Msg::Created,
                )
            }
            Msg::Created(Ok(v)) => {
                if let Some(id) = v.get("id").and_then(|i| i.as_str()) {
                    self.selected = Some(id.to_string());
                    self.status.clear();
                    self.turns.clear();
                    self.body_sel = None;
                }
                Task::batch([refresh_list(), {
                    if let Some(id) = &self.selected {
                        refresh_transcript(id.clone())
                    } else {
                        Task::none()
                    }
                }])
            }
            Msg::Created(Err(e)) => {
                self.status = e;
                Task::none()
            }
            Msg::Rm(id) => rest(
                {
                    let id = id.clone();
                    move || crate::client::delete(&format!("/bots/{id}"))
                },
                Msg::Removed,
            ),
            Msg::Removed(Ok(v)) => {
                let gone = v.get("id").and_then(|i| i.as_str());
                if let Some(id) = gone {
                    self.bots.retain(|b| b.id != id);
                    if self.selected.as_deref() == Some(id) {
                        self.selected = None;
                        self.turns.clear();
                        self.body_sel = None;
                        self.status.clear();
                    }
                }
                Task::none()
            }
            Msg::Removed(Err(e)) => {
                self.status = e;
                Task::none()
            }
            Msg::SidebarPress => {
                self.dragging_sidebar = true;
                Task::none()
            }
            Msg::ComposePress => {
                self.dragging_compose = true;
                Task::none()
            }
            Msg::Pointer(x, y) => {
                if self.dragging_sidebar && self.window_w > 1.0 {
                    self.sidebar_w = x.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX);
                }
                if self.dragging_compose && self.window_h > 1.0 {
                    let usable = (self.window_h - HEADER_H).max(1.0);
                    let thread =
                        (y - HEADER_H).clamp(usable * THREAD_RATIO_MIN, usable * THREAD_RATIO_MAX);
                    self.thread_ratio = (thread / usable).clamp(THREAD_RATIO_MIN, THREAD_RATIO_MAX);
                }
                Task::none()
            }
            Msg::PointerUp => {
                self.dragging_sidebar = false;
                self.dragging_compose = false;
                Task::none()
            }
            Msg::WindowResized(w, h) => {
                self.window_w = w;
                self.window_h = h;
                Task::none()
            }
            Msg::Pasted(text) => {
                if let Some(t) = text {
                    self.compose
                        .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                            Arc::new(t),
                        )));
                }
                Task::none()
            }
        }
    }

    fn apply_sse(&mut self, frame: SseFrame) -> Task<Msg> {
        match frame {
            SseFrame::Snapshot(v) => {
                self.status.clear();
                self.apply_poll(Ok(v))
            }
            SseFrame::Bots(v) => {
                self.status.clear();
                self.apply_bots(&v)
            }
            SseFrame::Transcript(v) => self.apply_transcript(&v),
            SseFrame::Delta { id, text } => self.apply_delta(&id, &text),
            SseFrame::Removed { id } => {
                self.bots.retain(|b| b.id != id);
                if self.selected.as_deref() == Some(id.as_str()) {
                    self.selected = None;
                    self.turns.clear();
                    self.body_sel = None;
                    self.status.clear();
                }
                Task::none()
            }
            SseFrame::Error(e) => {
                self.status = e;
                Task::none()
            }
        }
    }

    fn apply_delta(&mut self, id: &str, text: &str) -> Task<Msg> {
        if let Some(row) = self.bots.iter_mut().find(|b| b.id == id) {
            row.status = "working".into();
        }
        if self.selected.as_deref() != Some(id) {
            return Task::none();
        }
        match self.turns.last_mut() {
            Some((role, body)) if role == "assistant" => body.push_str(text),
            _ => self.turns.push(("assistant".into(), text.to_string())),
        }
        self.sending = true;
        self.on_thread_changed()
    }

    fn apply_poll(&mut self, r: Result<Value, String>) -> Task<Msg> {
        match r {
            Err(e) => {
                self.status = e;
                Task::none()
            }
            Ok(v) => {
                let t = self.apply_bots(&v);
                if let Some(tr) = v.get("transcript").filter(|t| !t.is_null()) {
                    let follow = self.apply_transcript(tr);
                    return Task::batch([t, follow]);
                }
                t
            }
        }
    }

    fn apply_bots(&mut self, v: &Value) -> Task<Msg> {
        self.bots = v
            .get("bots")
            .and_then(|b| b.as_array())
            .into_iter()
            .flatten()
            .filter_map(|b| {
                Some(BotRow {
                    id: b.get("id")?.as_str()?.to_string(),
                    name: b.get("name")?.as_str()?.to_string(),
                    status: b
                        .get("status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("idle")
                        .to_string(),
                })
            })
            .collect();
        if let Some(id) = &self.selected {
            if !self.bots.iter().any(|b| &b.id == id) {
                self.selected = None;
                self.turns.clear();
                self.body_sel = None;
                if self.status.starts_with("unknown bot") {
                    self.status.clear();
                }
            }
        }
        if self.selected.is_none() {
            if let Some(first) = self.bots.first() {
                self.selected = Some(first.id.clone());
                return refresh_transcript(first.id.clone());
            }
        }
        Task::none()
    }

    fn apply_transcript(&mut self, v: &Value) -> Task<Msg> {
        let tid = v.get("id").and_then(|i| i.as_str());
        if let (Some(sel), Some(tid)) = (self.selected.as_deref(), tid) {
            if sel != tid {
                return Task::none();
            }
        }
        self.turns = v
            .get("turns")
            .and_then(|t| t.as_array())
            .into_iter()
            .flatten()
            .filter_map(|t| {
                Some((
                    t.get("role")?.as_str()?.to_string(),
                    t.get("text")?.as_str()?.to_string(),
                ))
            })
            .collect();
        if let Some(st) = v.get("status").and_then(|s| s.as_str()) {
            self.sending = st == "working";
        }
        let err = v.get("error").and_then(|e| e.as_str()).unwrap_or("");
        if err.is_empty() || err.starts_with("unknown bot") {
            if self.status.starts_with("unknown bot") || err.is_empty() {
                self.status.clear();
            }
        } else {
            self.status = err.to_string();
        }
        self.on_thread_changed()
    }

    fn live_selected(&self) -> Option<String> {
        let id = self.selected.as_ref()?;
        self.bots.iter().any(|b| &b.id == id).then(|| id.clone())
    }

    fn send_compose(&mut self) -> Task<Msg> {
        let Some(id) = self.live_selected() else {
            return Task::none();
        };
        let text = self.compose.text();
        let text = text.trim().to_string();
        if text.is_empty() || self.sending {
            return Task::none();
        }
        self.compose = text_editor::Content::new();
        self.sending = true;
        self.status.clear();
        self.thread_follow = true;
        self.thread_unseen = false;
        Task::batch([
            rest(
                move || {
                    crate::client::post(
                        &format!("/bots/{id}/send"),
                        &serde_json::json!({ "text": text }),
                    )
                },
                Msg::Sent,
            ),
            snap_thread(),
        ])
    }

    fn on_thread_changed(&mut self) -> Task<Msg> {
        let fp: String = self.turns.iter().map(|(r, t)| format!("{r}:{t}")).collect();
        if fp == self.thread_fp {
            return Task::none();
        }
        self.thread_fp = fp;
        if self.thread_follow {
            snap_thread()
        } else {
            self.thread_unseen = true;
            Task::none()
        }
    }

    fn on_menu(&mut self, action: &str) -> Task<Msg> {
        match action {
            "cut" => {
                let Some(sel) = self.compose.selection() else {
                    return Task::none();
                };
                if sel.is_empty() {
                    return Task::none();
                }
                self.compose
                    .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                iced::clipboard::write(sel)
            }
            "copy" => {
                if let Some(t) = self.body_sel.as_ref().filter(|s| !s.is_empty()) {
                    return iced::clipboard::write(t.clone());
                }
                if let Some(sel) = self.compose.selection() {
                    if !sel.is_empty() {
                        return iced::clipboard::write(sel);
                    }
                }
                let t = self
                    .turns
                    .iter()
                    .map(|(r, b)| format!("{r}: {b}"))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if t.is_empty() {
                    Task::none()
                } else {
                    iced::clipboard::write(t)
                }
            }
            "paste" => iced::clipboard::read().map(Msg::Pasted),
            "select_all" => {
                self.prose_select_all = self.prose_select_all.saturating_add(1);
                self.compose.perform(text_editor::Action::SelectAll);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let pal = self.theme.extended_palette();
        let chrome = pal.background.weak.color;
        let canvas = pal.background.base.color;
        let line = pal.background.strong.color.scale_alpha(HAIRLINE_A);
        let win = self.window_w.max(1.0);
        let sidebar_ratio = (self.sidebar_w / win).clamp(0.12, 0.45);
        let body = split_with(
            SplitDir::Vertical,
            self.view_rail(),
            sidebar_ratio,
            Msg::SidebarPress,
            self.view_dialog(),
            DividerColors {
                a: chrome,
                line,
                b: canvas,
            },
        );
        let canvas = container(body).width(Length::Fill).height(Length::Fill);
        sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            "Bots",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            canvas.into(),
        )
    }

    fn selected_name(&self) -> Option<&str> {
        let id = self.selected.as_deref()?;
        self.bots
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.name.as_str())
    }

    fn view_rail(&self) -> Element<'_, Msg> {
        let items: Vec<_> = self
            .bots
            .iter()
            .map(|b| {
                let mut item = SidebarItem::new(b.name.clone(), Msg::Select(b.id.clone()));
                item.active = self.selected.as_deref() == Some(b.id.as_str());
                item.indicator = Some(match b.status.as_str() {
                    "working" => SidebarIndicator::Working,
                    "error" => SidebarIndicator::Waiting,
                    _ => SidebarIndicator::Idle,
                });
                item.id = Some(b.id.clone());
                item.on_close = Some(Msg::Rm(b.id.clone()));
                item
            })
            .collect();
        let list = sola_kit::components::sidebar(vec![SidebarSection::unlabeled(items)])
            .width(Length::Fill)
            .height(Length::Fill);
        let new_row = row![
            text_input("Name", &self.new_name)
                .on_input(Msg::NewName)
                .on_submit(Msg::Create)
                .width(Length::Fill),
            kit_btn::labeled_sm("Add", kit_btn::primary).on_press(Msg::Create),
        ]
        .spacing(SPACE_SM)
        .padding(Padding::from([SPACE_LG, SPACE_LG]));
        column![list, new_row]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_dialog(&self) -> Element<'_, Msg> {
        let pal = self.theme.extended_palette();
        let canvas = pal.background.base.color;
        let chrome = pal.background.weak.color;
        let line = pal.background.strong.color.scale_alpha(HAIRLINE_A);
        let title = self.selected_name().unwrap_or("Bots");
        let header = container(kit_text::subheading(title))
            .width(Length::Fill)
            .padding(Padding::from([SPACE_LG, SPACE_XL]));

        let body = split_with(
            SplitDir::Horizontal,
            self.view_thread(),
            self.thread_ratio,
            Msg::ComposePress,
            self.view_composer(),
            DividerColors {
                a: canvas,
                line,
                b: chrome,
            },
        );

        column![header, sola_kit::components::horizontal_divider(), body,]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_thread(&self) -> Element<'_, Msg> {
        let mut msgs = column![].spacing(SPACE_XL);
        if self.bots.is_empty() {
            msgs = msgs.push(
                kit_text::body("Name a bot in the rail to start a dialog.").style(kit_text::muted),
            );
        }
        for (role, body) in &self.turns {
            if body.trim().is_empty() {
                continue;
            }
            if role == "user" {
                msgs = msgs.push(user_line(body.clone(), &self.theme, self.prose_select_all));
            } else {
                msgs = msgs.push(bot_line(body, &self.theme, self.prose_select_all));
            }
        }
        let thinking = self.sending
            && self
                .turns
                .iter()
                .rev()
                .find(|(r, _)| r == "assistant")
                .is_none_or(|(_, t)| t.trim().is_empty());
        if thinking {
            msgs = msgs.push(kit_text::body("Thinking…").style(kit_text::muted));
        }
        if !self.status.is_empty() {
            msgs = msgs.push(kit_text::body(&self.status).style(kit_text::danger));
        }
        let scroller = scrollable(
            readable(msgs.padding(Padding::from([SPACE_LG, SPACE_XL])), 680.0)
                .width(Length::Fill)
                .padding(Padding::from([0.0, SPACE_XL])),
        )
        .id(thread_scroll_id())
        .on_scroll(Msg::Scrolled)
        .height(Length::Fill);
        if self.thread_unseen {
            let chip = container(
                kit_btn::labeled_sm("New output", kit_btn::primary).on_press(Msg::JumpLatest),
            )
            .width(Length::Fill)
            .padding(SPACE_LG)
            .align_bottom(Length::Fill)
            .center_x(Length::Fill);
            stack![scroller, chip].into()
        } else {
            scroller.into()
        }
    }

    fn view_composer(&self) -> Element<'_, Msg> {
        let editor = text_editor_widget(&self.compose)
            .placeholder("Message — Enter to send, Shift+Enter for a new line")
            .height(Length::Fill)
            .padding(SPACE_LG)
            .font(fonts::ui())
            .size(13)
            .style(compose_style)
            .key_binding(|press| compose_key(press))
            .on_action(Msg::ComposeAction);
        let action = if self.sending {
            kit_btn::labeled("Stop", kit_btn::danger).on_press(Msg::Cancel)
        } else {
            kit_btn::labeled("Send", kit_btn::primary).on_press(Msg::Send)
        };
        let bar = row![Space::new().width(Length::Fill), action]
            .padding(Padding::from([SPACE_SM, SPACE_XL]));
        column![editor, bar]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn compose_key(press: text_editor::KeyPress) -> Option<text_editor::Binding<Msg>> {
    use iced::keyboard::Key;
    use iced::keyboard::key::Named;
    if matches!(press.modified_key.as_ref(), Key::Named(Named::Enter)) {
        if press.modifiers.shift() {
            return Some(text_editor::Binding::Enter);
        }
        return Some(text_editor::Binding::Custom(Msg::KeySend));
    }
    text_editor::Binding::from_key_press(press)
}

fn compose_style(theme: &Theme, _status: text_editor::Status) -> text_editor::Style {
    let p = theme.extended_palette();
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_LG.into(),
        },
        placeholder: Color {
            a: 0.75,
            ..p.secondary.base.color
        },
        value: p.background.base.text,
        selection: mix_white(p.background.weaker.color, 0.16),
    }
}

fn user_line<'a>(body: String, theme: &'a Theme, select_all: u64) -> Element<'a, Msg> {
    let inner = prose_selectable(
        parse_plain(&body),
        theme,
        select_all,
        Msg::OpenUrl,
        Msg::BodySelect,
    );
    let bubble = container(inner)
        .padding(SPACE_LG)
        .max_width(480.0)
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            container::Style {
                background: Some(Background::Color(p.background.weak.color)),
                border: Border {
                    radius: RADIUS_LG.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                ..container::Style::default()
            }
        });
    row![Space::new().width(Length::Fill), bubble]
        .width(Length::Fill)
        .into()
}

fn bot_line<'a>(body: &'a str, theme: &'a Theme, select_all: u64) -> Element<'a, Msg> {
    prose_selectable(
        parse_markdown(body),
        theme,
        select_all,
        Msg::OpenUrl,
        Msg::BodySelect,
    )
}

fn thread_scroll_id() -> ScrollId {
    ScrollId::new("bots-thread")
}

fn snap_thread() -> Task<Msg> {
    operation::snap_to_end(thread_scroll_id())
}

fn refresh_list() -> Task<Msg> {
    rest(|| crate::client::get("/bots"), Msg::Listed)
}

fn refresh_transcript(id: String) -> Task<Msg> {
    rest(
        move || crate::client::get(&format!("/bots/{id}/transcript")),
        Msg::Transcript,
    )
}

fn rest(
    work: impl FnOnce() -> Result<Value, String> + Send + 'static,
    map: fn(Result<Value, String>) -> Msg,
) -> Task<Msg> {
    Task::perform(
        async move {
            std::thread::spawn(work)
                .join()
                .unwrap_or_else(|_| Err("http thread panicked".into()))
        },
        map,
    )
}

static SSE_TX: Mutex<Option<iced::futures::channel::mpsc::UnboundedSender<SseFrame>>> =
    Mutex::new(None);
static SSE_STARTED: AtomicBool = AtomicBool::new(false);

fn sse_stream() -> impl Stream<Item = SseFrame> {
    let (tx, rx) = iced::futures::channel::mpsc::unbounded();
    match SSE_TX.lock() {
        Ok(mut slot) => *slot = Some(tx),
        Err(poisoned) => *poisoned.into_inner() = Some(tx),
    }
    ensure_sse_thread();
    rx
}

fn ensure_sse_thread() {
    if SSE_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::Builder::new()
        .name("bots-sse".into())
        .spawn(|| {
            let mut backoff = Duration::from_millis(400);
            loop {
                let result = crate::client::read_events(|event, data| {
                    let frame = match event {
                        "snapshot" => SseFrame::Snapshot(data),
                        "bots" => SseFrame::Bots(data),
                        "transcript" => SseFrame::Transcript(data),
                        "delta" => SseFrame::Delta {
                            id: data
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            text: data
                                .get("text")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        },
                        "removed" => SseFrame::Removed {
                            id: data
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        },
                        _ => return true,
                    };
                    let mut slot = SSE_TX.lock().unwrap_or_else(|p| p.into_inner());
                    if let Some(tx) = slot.as_ref() {
                        if tx.unbounded_send(frame).is_err() {
                            *slot = None;
                            return false;
                        }
                    }
                    true
                });
                if let Err(e) = result {
                    let slot = SSE_TX.lock().unwrap_or_else(|p| p.into_inner());
                    if let Some(tx) = slot.as_ref() {
                        let _ = tx.unbounded_send(SseFrame::Error(e));
                    }
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(Duration::from_secs(8));
                } else {
                    backoff = Duration::from_millis(400);
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        })
        .ok();
}
