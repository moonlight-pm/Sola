//! The window: conversations beside the open one, drawn as Mail is. It
//! reads the store itself and asks the engine for everything else.

pub mod bridge;
mod list;
mod style;
mod thread;
mod when;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{Id as WidgetId, Space, column, container, image, row, text_editor};
use iced::{Element, Event as IcedEvent, Length, Subscription, Task, Theme, event, keyboard, window};
use sola_bus::Message;
use sola_bus::topics::{AppNotification, Topic};
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::components::file_picker::{FilePicker, Message as PickerMsg, Outcome};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::{self, MenuEdit, MenuEdited};
use sola_kit::theme::default_theme;

use crate::engine::{Command, Event};
use crate::store::chats::{Bubble, Chat, Id, Recipient};
use bridge::{store, tell};

pub const APP_ID: &str = "sola-signal";
/// Messages of a thread shown at first, and added by "Earlier messages".
const PAGE: u32 = 200;
/// How long "is typing" stays without news.
const TYPING: Duration = Duration::from_secs(6);
/// How often what changed is read again from the store.
const SETTLE: Duration = Duration::from_millis(120);
/// The name this device goes by on the phone's list of linked devices.
const DEVICE_NAME: &str = "Sola";

#[derive(Debug, Clone)]
pub enum Msg {
    WindowReady(Option<window::Id>),
    WindowFocus(bool),
    TitleDrag,
    TitleClose,
    TitleResize(window::Direction),
    Bus(Arc<Message>),
    Engine(Event),
    /// Read again from the store what the engine said changed.
    Settle,
    Escape,

    Search(String),
    Open(Id),
    /// Open the conversation with someone, making it if there is none.
    OpenPerson(Id),
    Draft(text_editor::Action),
    Send,
    /// Pick a message (or put it down): its actions show under it.
    Pick(Id),
    React(Id, String),
    Reply(Id),
    Edit(Id),
    Copy(Id),
    DeleteHere(Id),
    Retract(Id),
    Retry(Id),
    /// Leave the reply or the edit under way.
    Cancel,
    Attach,
    Picker(PickerMsg),
    Unattach(usize),
    Fetch(Id),
    OpenFile(String),
    OpenLink(String),
    Earlier,
    Relink,
    Pasted(Option<String>),
    MenuEdited(MenuEdit, Option<MenuEdited>),
}

/// Whether there is an account, as far as the window knows.
enum Linked {
    Yes,
    /// Not linked: the code to scan once the engine has it, or why not.
    No { code: Option<image::Handle>, failed: Option<String> },
}

enum Wire {
    Connecting,
    Online,
    Offline,
}

pub struct App {
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<window::Id>,
    focused: bool,
    linked: Linked,
    wire: Wire,
    /// Our own row among the people.
    me: Id,
    chats: Vec<Chat>,
    people: Vec<Recipient>,
    search: String,
    open: Option<Id>,
    thread: Vec<Bubble>,
    limit: u32,
    draft: text_editor::Content,
    replying: Option<Id>,
    editing: Option<Id>,
    files: Vec<PathBuf>,
    picked: Option<Id>,
    /// Who is typing, where, and when we last heard so.
    typing: Option<(Id, String, Instant)>,
    stale_list: bool,
    stale_thread: bool,
    picker: Option<FilePicker>,
    last_dir: Option<PathBuf>,
}

fn editor_id() -> WidgetId {
    WidgetId::new("signal-composer")
}

fn thread_id() -> WidgetId {
    WidgetId::new("signal-thread")
}

fn search_id() -> WidgetId {
    WidgetId::new("signal-search")
}

impl App {
    pub fn boot() -> (Self, Task<Msg>) {
        let mut app = Self {
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            focused: true,
            linked: Linked::No { code: None, failed: None },
            wire: Wire::Connecting,
            me: 0,
            chats: Vec::new(),
            people: Vec::new(),
            search: String::new(),
            open: None,
            thread: Vec::new(),
            limit: PAGE,
            draft: text_editor::Content::new(),
            replying: None,
            editing: None,
            files: Vec::new(),
            picked: None,
            typing: None,
            stale_list: false,
            stale_thread: false,
            picker: None,
            last_dir: None,
        };
        app.take_account();
        (app, sola_kit::window_ready_task(Msg::WindowReady))
    }

    /// Read who we are from the store, if anyone.
    fn take_account(&mut self) {
        match store().account() {
            Ok(Some(account)) => {
                self.linked = Linked::Yes;
                self.me = store().recipient_for(account.aci).unwrap_or(0);
                self.read_list();
            }
            Ok(None) => {}
            Err(e) => tracing::error!("reading the account: {e:#}"),
        }
    }

    pub fn title(&self) -> String {
        match self.chats.iter().filter(|chat| !chat.muted).map(|chat| chat.unread).sum::<u32>() {
            0 => "Signal".into(),
            unread => format!("Signal · {unread}"),
        }
    }

    fn frame_title(&self) -> String {
        match self.open_chat() {
            Some(chat) => chat.title.clone(),
            None => "Signal".into(),
        }
    }

    pub fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.chrome_any(), &self.theme)
    }

    fn open_chat(&self) -> Option<&Chat> {
        self.open.and_then(|open| self.chats.iter().find(|chat| chat.id == open))
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        let settle = if self.stale_list || self.stale_thread || self.typing.is_some() {
            iced::time::every(SETTLE).map(|_| Msg::Settle)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            bridge::events().map(Msg::Engine),
            if self.picker.is_some() { FilePicker::keys().map(Msg::Picker) } else { Subscription::none() },
            event::listen_with(|event, _status, _id| match event {
                IcedEvent::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => Some(Msg::Escape),
                IcedEvent::Window(window::Event::Focused) => Some(Msg::WindowFocus(true)),
                IcedEvent::Window(window::Event::Unfocused) => Some(Msg::WindowFocus(false)),
                _ => None,
            }),
            settle,
        ])
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::WindowReady(id) => self.window_id = id,
            Msg::WindowFocus(focused) => {
                self.focused = focused;
                if focused {
                    self.mark_read();
                }
            }
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(direction) => return sola_kit::drag_resize(self.window_id, direction),
            Msg::TitleClose => sola_kit::close_app(APP_ID),
            Msg::Bus(message) => return self.on_bus(&message),
            Msg::Engine(event) => return self.on_engine(event),
            Msg::Settle => {
                if std::mem::take(&mut self.stale_list) {
                    self.read_list();
                }
                if std::mem::take(&mut self.stale_thread) {
                    self.read_thread();
                }
                if self.typing.as_ref().is_some_and(|(_, _, at)| at.elapsed() > TYPING) {
                    self.typing = None;
                }
            }
            Msg::Escape => {
                if self.picker.is_some() {
                    return Task::none();
                }
                if self.replying.is_some() || self.editing.is_some() {
                    return self.update(Msg::Cancel);
                }
                self.picked = None;
            }

            Msg::Search(query) => {
                self.search = query;
                if !self.search.is_empty() && self.people.is_empty() {
                    self.people = store().recipients().unwrap_or_default();
                }
            }
            Msg::Open(conversation) => return self.open_conversation(conversation),
            Msg::OpenPerson(person) => {
                let opened = store().conversation_with(person).and_then(|conversation| {
                    store().open_conversation(conversation, crate::store::now())?;
                    Ok(conversation)
                });
                match opened {
                    Ok(conversation) => {
                        self.search.clear();
                        self.read_list();
                        return self.open_conversation(conversation);
                    }
                    Err(e) => tracing::warn!("opening a conversation: {e:#}"),
                }
            }
            Msg::Draft(action) => self.draft.perform(action),
            Msg::Send => self.send(),
            Msg::Pick(message) => self.picked = (self.picked != Some(message)).then_some(message),
            Msg::React(message, emoji) => {
                let mine = self
                    .thread
                    .iter()
                    .find(|bubble| bubble.id == message)
                    .is_some_and(|bubble| bubble.reactions.iter().any(|(e, _, mine)| *mine && *e == emoji));
                tell(Command::React { message, emoji: (!mine).then_some(emoji) });
                self.picked = None;
            }
            Msg::Reply(message) => {
                self.replying = Some(message);
                self.editing = None;
                self.picked = None;
                return iced::widget::operation::focus(editor_id());
            }
            Msg::Edit(message) => {
                if let Some(bubble) = self.thread.iter().find(|bubble| bubble.id == message) {
                    self.draft = text_editor::Content::with_text(&bubble.body);
                    self.editing = Some(message);
                    self.replying = None;
                    self.picked = None;
                    return iced::widget::operation::focus(editor_id());
                }
            }
            Msg::Copy(message) => {
                self.picked = None;
                if let Some(bubble) = self.thread.iter().find(|bubble| bubble.id == message) {
                    return iced::clipboard::write(bubble.body.clone());
                }
            }
            Msg::DeleteHere(message) => {
                self.picked = None;
                tell(Command::Delete { message });
            }
            Msg::Retract(message) => {
                self.picked = None;
                tell(Command::Retract { message });
            }
            Msg::Retry(message) => tell(Command::Retry { message }),
            Msg::Cancel => {
                if self.editing.take().is_some() {
                    self.draft = text_editor::Content::new();
                }
                self.replying = None;
            }
            Msg::Attach => {
                let mut picker = FilePicker::open().multiple().title("Attach files").action("Attach");
                if let Some(dir) = self.last_dir.clone().or_else(|| std::env::var_os("HOME").map(PathBuf::from)) {
                    picker = picker.start_dir(dir);
                }
                self.picker = Some(picker);
            }
            Msg::Picker(msg) => return self.on_picker(msg),
            Msg::Unattach(index) => {
                if index < self.files.len() {
                    self.files.remove(index);
                }
            }
            Msg::Fetch(attachment) => tell(Command::Fetch { attachment }),
            Msg::OpenFile(path) => sola_kit::opener::open(&path),
            Msg::OpenLink(url) => sola_kit::opener::open(&url),
            Msg::Earlier => {
                self.limit += PAGE;
                self.read_thread();
            }
            Msg::Relink => {
                self.linked = Linked::No { code: None, failed: None };
                tell(Command::Link { device_name: DEVICE_NAME.into() });
            }
            Msg::Pasted(Some(text)) => return self.menu_edit(MenuEdit::Paste(text)),
            Msg::Pasted(None) => {}
            Msg::MenuEdited(edit, done) => return self.on_menu_edited(edit, done),
        }
        Task::none()
    }

    fn on_bus(&mut self, message: &Message) -> Task<Msg> {
        self.float.update(message);
        apply_theme_update(message, &mut self.theme);
        if is_self_quit(message, APP_ID) {
            self.keep_draft();
            return iced::exit();
        }
        match Topic::parse(message) {
            Some(Topic::MenuAction(action)) if action.app_id == APP_ID => self.on_menu(&action.action_id),
            _ => Task::none(),
        }
    }

    fn on_menu(&mut self, action: &str) -> Task<Msg> {
        match action {
            "quit" => {
                self.keep_draft();
                iced::exit()
            }
            "cut" => self.menu_edit(MenuEdit::Cut),
            "copy" => self.menu_edit(MenuEdit::Copy),
            "paste" => iced::clipboard::read().map(Msg::Pasted),
            "select_all" => self.menu_edit(MenuEdit::SelectAll),
            "find" => iced::widget::operation::focus(search_id()),
            "attach" if self.open.is_some() => self.update(Msg::Attach),
            "connect" => {
                tell(Command::Connect);
                Task::none()
            }
            _ => match (action.strip_prefix("timer:").and_then(|s| s.parse().ok()), self.open) {
                (Some(seconds), Some(conversation)) => {
                    tell(Command::SetTimer { conversation, seconds });
                    Task::none()
                }
                _ => Task::none(),
            },
        }
    }

    /// An Edit menu command: to the search field if it has the cursor, else
    /// to the composer.
    fn menu_edit(&self, edit: MenuEdit) -> Task<Msg> {
        text_input::edit_focused(edit.clone()).map(move |done| Msg::MenuEdited(edit.clone(), done))
    }

    fn on_menu_edited(&mut self, edit: MenuEdit, done: Option<MenuEdited>) -> Task<Msg> {
        let Some(done) = done else {
            return match edit {
                MenuEdit::Paste(text) => {
                    self.draft.perform(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(text))));
                    Task::none()
                }
                MenuEdit::Copy => match self.draft.selection().filter(|selection| !selection.is_empty()) {
                    Some(selection) => iced::clipboard::write(selection),
                    None => Task::none(),
                },
                MenuEdit::Cut => match self.draft.selection().filter(|selection| !selection.is_empty()) {
                    Some(selection) => {
                        self.draft.perform(text_editor::Action::Edit(text_editor::Edit::Delete));
                        iced::clipboard::write(selection)
                    }
                    None => Task::none(),
                },
                MenuEdit::SelectAll => {
                    self.draft.perform(text_editor::Action::SelectAll);
                    Task::none()
                }
                MenuEdit::Undo | MenuEdit::Redo => Task::none(),
            };
        };
        if let Some(value) = done.value {
            self.search = value;
        }
        match done.copied {
            Some(text) if !text.is_empty() => iced::clipboard::write(text),
            _ => Task::none(),
        }
    }

    fn on_engine(&mut self, event: Event) -> Task<Msg> {
        match event {
            Event::Unlinked => {
                self.linked = Linked::No { code: None, failed: None };
                self.open = None;
                tell(Command::Link { device_name: DEVICE_NAME.into() });
            }
            Event::LinkCode(url) => self.linked = Linked::No { code: code(&url), failed: None },
            Event::LinkFailed(why) => self.linked = Linked::No { code: None, failed: Some(why) },
            Event::Linked => self.take_account(),
            Event::Connecting => self.wire = Wire::Connecting,
            Event::Online => self.wire = Wire::Online,
            Event::Offline(why) => {
                tracing::info!(%why, "offline");
                self.wire = Wire::Offline;
            }
            Event::Changed(conversation) => {
                self.stale_list = true;
                if self.open == Some(conversation) {
                    self.stale_thread = true;
                    self.mark_read();
                }
            }
            Event::People => {
                self.stale_list = true;
                self.stale_thread = true;
                self.people.clear();
            }
            Event::Arrived { conversation, title, text } => {
                if self.typing.as_ref().is_some_and(|(typing, ..)| *typing == conversation) {
                    self.typing = None;
                }
                if !(self.focused && self.open == Some(conversation)) {
                    notify(conversation, &title, &text);
                }
            }
            Event::Typing { conversation, who, started } => {
                self.typing = started.then(|| (conversation, who, Instant::now()));
            }
        }
        Task::none()
    }

    fn read_list(&mut self) {
        match store().chats() {
            Ok(mut chats) => {
                // The conversation with ourselves is the notes we keep.
                for chat in chats.iter_mut().filter(|chat| chat.recipient == Some(self.me)) {
                    chat.title = "Note to Self".into();
                }
                self.chats = chats;
            }
            Err(e) => tracing::warn!("reading the conversations: {e:#}"),
        }
    }

    fn read_thread(&mut self) {
        let Some(open) = self.open else { return };
        match store().thread(open, self.limit, self.me) {
            Ok(thread) => self.thread = thread,
            Err(e) => tracing::warn!("reading the thread: {e:#}"),
        }
    }

    /// What is on screen has been read, if the window is in front.
    fn mark_read(&self) {
        if let (true, Some(conversation)) = (self.focused, self.open) {
            tell(Command::Read { conversation });
        }
    }

    fn keep_draft(&self) {
        if let (Some(open), None) = (self.open, self.editing) {
            let _ = store().set_draft(open, self.draft.text().trim_end());
        }
    }

    fn open_conversation(&mut self, conversation: Id) -> Task<Msg> {
        if self.open == Some(conversation) {
            return iced::widget::operation::focus(editor_id());
        }
        self.keep_draft();
        self.open = Some(conversation);
        self.limit = PAGE;
        self.picked = None;
        self.replying = None;
        self.editing = None;
        self.files.clear();
        let draft = self.chats.iter().find(|chat| chat.id == conversation).map(|chat| chat.draft.clone());
        self.draft = text_editor::Content::with_text(draft.as_deref().unwrap_or_default());
        self.read_thread();
        self.mark_read();
        Task::batch([
            iced::widget::operation::snap_to(thread_id(), iced::widget::scrollable::RelativeOffset::START),
            iced::widget::operation::focus(editor_id()),
        ])
    }

    fn send(&mut self) {
        let Some(conversation) = self.open else { return };
        let body = self.draft.text().trim().to_owned();
        if let Some(message) = self.editing.take() {
            if !body.is_empty() {
                tell(Command::Edit { message, body });
            }
        } else {
            if body.is_empty() && self.files.is_empty() {
                return;
            }
            tell(Command::Send {
                conversation,
                body,
                quote: self.replying.take(),
                files: std::mem::take(&mut self.files),
            });
        }
        self.draft = text_editor::Content::new();
        let _ = store().set_draft(conversation, "");
    }

    fn on_picker(&mut self, msg: PickerMsg) -> Task<Msg> {
        let Some(picker) = self.picker.as_mut() else { return Task::none() };
        let outcome = picker.update(msg);
        let follow = picker.scroll_task().map(Msg::Picker);
        let picked = match outcome {
            Some(Outcome::Cancelled) => Vec::new(),
            Some(Outcome::Picked(path)) => vec![path],
            Some(Outcome::PickedMany(paths)) => paths,
            None => return follow,
        };
        if let Some(dir) = picked.first().and_then(|path| path.parent()) {
            self.last_dir = Some(dir.to_path_buf());
        }
        self.files.extend(picked);
        self.picker = None;
        iced::widget::operation::focus(editor_id())
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let content: Element<'_, Msg> = match &self.linked {
            Linked::No { code, failed } => self.view_link(code.as_ref(), failed.as_deref()),
            Linked::Yes => row![self.view_list(), list::v_hairline(), self.view_thread()].height(Length::Fill).into(),
        };
        let body: Element<'_, Msg> =
            container(content).width(Length::Fill).height(Length::Fill).style(style::canvas).into();
        let framed = sola_kit::window_frame(
            self.float.chrome_any(),
            self.frame_title(),
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            body,
        );
        match &self.picker {
            Some(picker) => iced::widget::stack![framed, picker.overlay().map(Msg::Picker)].into(),
            None => framed,
        }
    }

    /// Not linked: the code for the phone to scan.
    fn view_link<'a>(&'a self, code: Option<&'a image::Handle>, failed: Option<&'a str>) -> Element<'a, Msg> {
        let mut card = column![kit_text::heading("Link Signal to your phone")]
            .spacing(SPACE_LG)
            .align_x(iced::Alignment::Center)
            .max_width(420);
        match (code, failed) {
            (_, Some(why)) => {
                card = card.push(kit_text::body(format!("Linking did not work: {why}")).style(kit_text::danger)).push(
                    sola_kit::components::button::labeled("Try Again", sola_kit::components::button::primary)
                        .on_press(Msg::Relink),
                );
            }
            (Some(code), None) => {
                card = card
                    .push(container(image(code.clone()).width(260).height(260)).padding(SPACE_MD).style(style::paper))
                    .push(
                        kit_text::body("On your phone, open Signal, go to Settings → Linked devices → Link a new device, and scan this code.")
                            .align_x(iced::Alignment::Center),
                    )
                    .push(
                        kit_text::caption(format!("This computer will appear there as “{DEVICE_NAME}”. Your phone stays the main device."))
                            .style(kit_text::muted)
                            .align_x(iced::Alignment::Center),
                    );
            }
            (None, None) => card = card.push(kit_text::body("Asking Signal for a code…").style(kit_text::muted)),
        }
        container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(SPACE_LG)
            .into()
    }

    /// Why the thread may be behind, said in its bar: nothing when online.
    fn wire_note(&self) -> Option<Element<'_, Msg>> {
        let note = match &self.wire {
            Wire::Online => return None,
            Wire::Connecting => "Connecting…".to_owned(),
            Wire::Offline => "Offline. Trying again…".to_owned(),
        };
        Some(row![Space::new().width(SPACE_SM), kit_text::caption(note).style(kit_text::warning)].into())
    }
}

/// The address to link by, as a code a phone can scan.
fn code(url: &str) -> Option<image::Handle> {
    const QUIET: usize = 2;
    const SCALE: usize = 8;
    let code = qrcode::QrCode::new(url).ok()?;
    let modules = code.width();
    let colors = code.to_colors();
    let side = (modules + 2 * QUIET) * SCALE;
    let mut pixels = vec![255u8; side * side * 4];
    for (index, color) in colors.iter().enumerate() {
        if *color != qrcode::Color::Dark {
            continue;
        }
        let (x, y) = ((index % modules + QUIET) * SCALE, (index / modules + QUIET) * SCALE);
        for row in y..y + SCALE {
            for column in x..x + SCALE {
                let at = (row * side + column) * 4;
                pixels[at..at + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
    }
    Some(image::Handle::from_rgba(side as u32, side as u32, pixels))
}

/// Tell the desk of a message; one card per conversation, the newest.
fn notify(conversation: Id, title: &str, text: &str) {
    let mut body: String = text.chars().take(240).collect();
    if body.len() < text.len() {
        body.push('…');
    }
    let note = AppNotification {
        id: String::new(),
        app_id: APP_ID.into(),
        source: "Signal".into(),
        title: title.to_owned(),
        body,
        tag: Some(format!("conversation:{conversation}")),
        tab_id: None,
        url: None,
    };
    if let Ok(mut bus) = sola_kit::app::bus().lock() {
        let _ = bus.emit(Topic::AppNotification(note));
    }
}
