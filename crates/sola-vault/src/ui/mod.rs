//! Vault's window: Mail's three columns once unlocked (places · items ·
//! the record), one calm card before that (sign in, two-step, unlock, the
//! service away), sheets over it all (generator, settings, export,
//! folders, the questions before something can't be undone).

mod answers;
mod ask;
mod bar;
mod copy;
mod edit;
mod fields;
mod foot;
mod items;
mod keys;
mod list;
mod record;
mod sheets;
mod sidebar;
mod signin;
mod style;
mod widgets;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::keyboard::key::Physical;
use iced::keyboard::{Key, Modifiers};
use iced::widget::{column, container, opaque, row, stack, text_editor};
use iced::window::Direction;
use iced::{Element, Event, Length, Padding, Subscription, Task, Theme, event, keyboard};
use sola_bus::Message;
use sola_bus::topics::Topic;
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::components::file_picker::{FilePicker, Message as PickerMsg};
use sola_kit::components::style::{SPACE_LG, SPACE_MD};
use sola_kit::components::text_input::{MenuEdit, MenuEdited};
use sola_kit::theme::default_theme;
use sola_vault_proto::{
    Collection, Folder, GeneratorOptions, ItemKind, ItemRecord, ItemSummary, Organization,
    Request, Settings, Status, TwoFactorKind,
};

use crate::daemon::{self, Answer, Link};
use crate::model::clip::ClipGuard;
use crate::model::section::Section;
use crate::model::sort::Sort;
use crate::model::when;
use crate::{APP_ID, instance, menus};
pub use ask::Ask;
use copy::CopyWhat;
use edit::EditForm;
use fields::{Field, Text};
use sheets::{ExportMsg, GenMsg, SetMsg, ShareMsg, Sheet};
use signin::{ServerChoice, SignIn, Unlock};

/// How long a row says "Copied".
const COPIED_FOR: Duration = Duration::from_millis(1500);
/// How long a toast stays.
const TOAST_FOR: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    Link(Link),
    Answer(Ask, Answer),
    /// A later launch: the item it named.
    Handoff(Option<String>),
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(Direction),
    TitleClose,
    Key {
        key: Key,
        physical: Physical,
        mods: Modifiers,
        captured: bool,
    },
    Tick,
    Noop,

    Input(Field, Text),
    Submit(Field),
    MenuEdited(MenuEdit, Option<MenuEdited>),
    Pasted(Option<Text>),
    /// Open (or close, `None`) the one menu or select.
    Popup(Option<Popup>),

    // Signing in, unlocking.
    Server(ServerChoice),
    RevealForm,
    SignIn,
    VerifyTwoStep,
    UseKind(TwoFactorKind),
    SendCode,
    BackToSignIn,
    Unlock,
    OtherAccount,

    // Browsing.
    Show(Section),
    Select(String),
    SortBy(Sort),
    SearchFocus,
    SearchClear,
    NewItem(ItemKind),
    Retry,

    // The record.
    Reveal(String),
    Copy(CopyWhat),
    OpenUrl(String),
    History(bool),
    Favorite,
    Edit,
    Clone,
    Trash,
    Restore,
    AskDeleteForever,
    Reprompt,
    RepromptCancel,

    // Editing.
    Draft(edit::DraftMsg),
    Notes(text_editor::Action),
    Save,
    CancelEdit,
    Discard,
    KeepEditing,

    // Sheets.
    CloseSheet,
    OpenSettings,
    AskMove,
    MoveTo(Option<String>),
    AskShare,
    Share(ShareMsg),
    NewFolder,
    RenameFolder(String),
    AskDeleteFolder(String),
    SaveFolder,
    Confirm,
    Generator(GenMsg),
    Settings(SetMsg),
    Export(ExportMsg),
    Picker(PickerMsg),
    Lock,
    Sync,
    AskSignOut,
    DismissToast,
}

/// The one menu or select open at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    NewItem,
    RecordMore,
    FolderMore,
    Server,
    IdleLock,
    ClipClear,
    ExportFormat,
    Match(usize),
    EditFolder,
}

/// Is the daemon there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkState {
    Connecting,
    Up,
    Down,
}

/// What the window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Connecting,
    Unavailable,
    SignedOut,
    Locked,
    Unlocked,
}

/// The record pane.
pub enum Pane {
    Empty,
    Loading,
    Open(Box<ItemRecord>),
    Failed(String),
}

/// An item that asks for the master password before it opens.
pub struct Reprompt {
    pub id: String,
    pub password: Text,
    pub reveal: bool,
    pub busy: bool,
    pub error: Option<String>,
}

pub struct Toast {
    pub text: String,
    pub until: Instant,
}

pub struct App {
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,

    link: LinkState,
    /// The service is there but would not let Vault in: why.
    refused: Option<String>,
    status: Option<Status>,
    settings: Settings,

    signin: SignIn,
    unlock: Unlock,

    items: Vec<ItemSummary>,
    unreadable: u32,
    loaded: bool,
    folders: Vec<Folder>,
    orgs: Vec<Organization>,
    collections: Vec<Collection>,

    section: Section,
    query: String,
    sort: Sort,
    selected: Option<String>,
    /// `--item` from the command line or a handoff, until the list has it.
    pending_open: Option<String>,
    record: Pane,
    reprompt: Option<Reprompt>,
    /// Reprompt items whose master password was given since the unlock.
    verified: HashSet<String>,
    /// Secrets shown on the open record (field keys).
    revealed: HashSet<String>,
    history_open: bool,
    edit: Option<EditForm>,

    popup: Option<Popup>,
    sheet: Option<Sheet>,
    picker: Option<FilePicker>,
    gen_options: GeneratorOptions,
    copied: Option<(String, Instant)>,
    clip: Option<ClipGuard>,
    toast: Option<Toast>,
    /// Unix seconds, for codes and the foot; moved on by the tick.
    now: u64,
}

impl App {
    pub fn boot(item: Option<String>) -> (Self, Task<Msg>) {
        let app = Self {
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            link: LinkState::Connecting,
            refused: None,
            status: None,
            settings: Settings::default(),
            signin: SignIn::default(),
            unlock: Unlock::default(),
            items: Vec::new(),
            unreadable: 0,
            loaded: false,
            folders: Vec::new(),
            orgs: Vec::new(),
            collections: Vec::new(),
            section: Section::All,
            query: String::new(),
            sort: Sort::default(),
            selected: None,
            pending_open: item,
            record: Pane::Empty,
            reprompt: None,
            verified: HashSet::new(),
            revealed: HashSet::new(),
            history_open: false,
            edit: None,
            popup: None,
            sheet: None,
            picker: None,
            gen_options: GeneratorOptions::default(),
            copied: None,
            clip: None,
            toast: None,
            now: when::now_unix(),
        };
        (app, sola_kit::window_ready_task(Msg::WindowReady))
    }

    /// The window's name (the shell's tab bar lists it).
    pub fn title(&self) -> String {
        "Vault".into()
    }

    pub fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.chrome_any(), &self.theme)
    }

    fn phase(&self) -> Phase {
        match (self.link, &self.status) {
            (LinkState::Down, _) => Phase::Unavailable,
            (_, None) => Phase::Connecting,
            (_, Some(s)) if !s.signed_in => Phase::SignedOut,
            (_, Some(s)) if !s.unlocked => Phase::Locked,
            _ => Phase::Unlocked,
        }
    }

    /// Something on screen moves with the clock.
    fn ticking(&self) -> bool {
        let code = matches!(&self.record, Pane::Open(r) if r.totp_secret.is_some());
        code || self.copied.is_some()
            || self.clip.is_some()
            || self.toast.is_some()
            || self.phase() == Phase::Unlocked
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        let tick = if self.ticking() {
            // The foot's "2 min ago" is coarse; a code's ring is not.
            let fast = matches!(&self.record, Pane::Open(r) if r.totp_secret.is_some())
                || self.copied.is_some();
            let every = if fast { Duration::from_secs(1) } else { Duration::from_secs(15) };
            iced::time::every(every).map(|_| Msg::Tick)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            daemon::link().map(Msg::Link),
            instance::subscription().map(Msg::Handoff),
            if self.picker.is_some() {
                FilePicker::keys().map(Msg::Picker)
            } else {
                Subscription::none()
            },
            event::listen_with(|event, status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key,
                    physical_key,
                    modifiers,
                    ..
                }) => Some(Msg::Key {
                    key,
                    physical: physical_key,
                    mods: modifiers,
                    captured: status == event::Status::Captured,
                }),
                _ => None,
            }),
            tick,
        ])
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(m) => self.on_bus(&m),
            Msg::Link(link) => self.on_link(link),
            Msg::Answer(ask, answer) => self.on_answer(ask, answer),
            Msg::Handoff(item) => {
                self.raise();
                match item {
                    Some(id) => self.open_item(id),
                    None => Task::none(),
                }
            }
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
            Msg::Key {
                key,
                physical,
                mods,
                captured,
            } => self.on_key(key, physical, mods, captured),
            Msg::Tick => self.on_tick(),
            Msg::Noop => Task::none(),

            Msg::Input(field, value) => self.set_field(field, value),
            Msg::Submit(field) => self.on_submit(field),
            Msg::MenuEdited(edit, done) => self.on_menu_edited(edit, done),
            Msg::Pasted(Some(t)) => self.menu_edit(MenuEdit::Paste(t.to_string())),
            Msg::Pasted(None) => Task::none(),
            Msg::Popup(p) => {
                self.popup = p;
                Task::none()
            }

            Msg::Server(choice) => {
                self.popup = None;
                self.signin.server = choice;
                self.signin.error = None;
                if choice == ServerChoice::SelfHosted {
                    return Field::ServerUrl.focus();
                }
                Task::none()
            }
            Msg::RevealForm => {
                self.signin.reveal = !self.signin.reveal;
                self.unlock.reveal = !self.unlock.reveal;
                if let Some(r) = self.reprompt.as_mut() {
                    r.reveal = !r.reveal;
                }
                Task::none()
            }
            Msg::SignIn => self.sign_in(),
            Msg::VerifyTwoStep => self.verify_two_step(),
            Msg::UseKind(kind) => self.use_kind(kind),
            Msg::SendCode => self.send_code(),
            Msg::BackToSignIn => {
                self.signin.two_step = None;
                Field::Password.focus()
            }
            Msg::Unlock => self.do_unlock(),
            Msg::OtherAccount => {
                self.signin = SignIn::default();
                self.ask(Ask::Logout, Request::Logout)
            }

            Msg::Show(section) => self.show(section),
            Msg::Select(id) => self.select(id),
            Msg::SortBy(sort) => {
                self.sort = sort;
                sola_kit::menu::publish(menus::app_menu(APP_ID, sort));
                Task::none()
            }
            Msg::SearchFocus => {
                if self.phase() == Phase::Unlocked {
                    return Task::batch([
                        Field::Search.focus(),
                        iced::widget::operation::select_all(Field::Search.id()),
                    ]);
                }
                Task::none()
            }
            Msg::SearchClear => {
                self.query.clear();
                Task::none()
            }
            Msg::NewItem(kind) => self.new_item(kind),
            Msg::Retry => self.ask(Ask::Sync, Request::Sync { force: true }),

            Msg::Reveal(key) => {
                if !self.revealed.remove(&key) {
                    self.revealed.insert(key);
                }
                Task::none()
            }
            Msg::Copy(what) => self.copy(what),
            Msg::OpenUrl(url) => {
                sola_core::open_url_logged(&items::launchable(&url));
                Task::none()
            }
            Msg::History(open) => {
                self.history_open = open;
                Task::none()
            }
            Msg::Favorite => self.toggle_favorite(),
            Msg::Edit => self.begin_edit(),
            Msg::Clone => self.clone_item(),
            Msg::Trash => self.trash(),
            Msg::Restore => self.restore(),
            Msg::AskDeleteForever => self.ask_delete_forever(),
            Msg::Reprompt => self.submit_reprompt(),
            Msg::RepromptCancel => {
                self.reprompt = None;
                self.selected = None;
                self.record = Pane::Empty;
                Task::none()
            }

            Msg::Draft(m) => self.on_draft(m),
            Msg::Notes(action) => {
                if let Some(e) = self.edit.as_mut() {
                    e.notes.perform(action);
                }
                Task::none()
            }
            Msg::Save => self.save(),
            Msg::CancelEdit => self.cancel_edit(),
            Msg::Discard => {
                self.edit = None;
                Task::none()
            }
            Msg::KeepEditing => {
                if let Some(e) = self.edit.as_mut() {
                    e.confirm_discard = false;
                }
                Task::none()
            }

            Msg::CloseSheet => {
                self.close_sheet();
                Task::none()
            }
            Msg::OpenSettings => self.open_settings(),
            Msg::AskMove => self.ask_move(),
            Msg::MoveTo(folder) => self.move_to(folder),
            Msg::AskShare => self.ask_share(),
            Msg::Share(m) => self.on_share(m),
            Msg::NewFolder => self.new_folder(),
            Msg::RenameFolder(id) => self.rename_folder(id),
            Msg::AskDeleteFolder(id) => self.ask_delete_folder(id),
            Msg::SaveFolder => self.save_folder(),
            Msg::Confirm => self.confirm(),
            Msg::Generator(m) => self.on_generator(m),
            Msg::Settings(m) => self.on_settings(m),
            Msg::Export(m) => self.on_export(m),
            Msg::Picker(m) => self.on_picker(m),
            Msg::Lock => self.lock(),
            Msg::Sync => self.ask(Ask::Sync, Request::Sync { force: true }),
            Msg::AskSignOut => self.ask_sign_out(),
            Msg::DismissToast => {
                self.toast = None;
                Task::none()
            }
        }
    }

    // ── The bus, the daemon ──────────────────────────────────────────

    fn on_bus(&mut self, message: &Message) -> Task<Msg> {
        self.float.update(message);
        apply_theme_update(message, &mut self.theme);
        if is_self_quit(message, APP_ID) {
            return self.quit();
        }
        match Topic::parse(message) {
            Some(Topic::MenuAction(p)) if p.app_id == APP_ID => self.on_menu_action(&p.action_id),
            _ => Task::none(),
        }
    }

    fn on_link(&mut self, link: Link) -> Task<Msg> {
        match link {
            Link::Up => {
                self.link = LinkState::Up;
                self.refused = None;
                Task::batch([
                    self.ask(Ask::Status, Request::Status),
                    self.ask(Ask::Settings, Request::Settings),
                ])
            }
            Link::Down { refused } => {
                if self.link != LinkState::Down {
                    tracing::info!("vault service away; showing the waiting card");
                }
                self.link = LinkState::Down;
                self.refused = refused;
                self.status = None;
                self.forget_vault();
                Task::none()
            }
            Link::Event(ev) => self.on_event(ev),
        }
    }

    fn on_event(&mut self, ev: sola_vault_proto::Event) -> Task<Msg> {
        use sola_vault_proto::Event;
        match ev {
            Event::Status(s) => self.apply_status(s),
            Event::ItemsChanged { ids } => {
                if self.phase() != Phase::Unlocked {
                    return Task::none();
                }
                let mut tasks = vec![self.ask(Ask::List, Request::List { url: None })];
                // The open record changed elsewhere (a sync, the browser):
                // read it again, unless it is being edited (Save writes
                // only what changed, onto what the server has).
                if let (Some(id), Pane::Open(_), None) = (&self.selected, &self.record, &self.edit)
                    && ids.iter().any(|i| i == id)
                {
                    tasks.push(self.ask(Ask::Get(id.clone()), Request::Get { id: id.clone() }));
                }
                Task::batch(tasks)
            }
            Event::Folders => {
                if self.phase() != Phase::Unlocked {
                    return Task::none();
                }
                self.ask(Ask::Folders, Request::Folders)
            }
        }
    }

    /// The daemon's status: what the window shows follows it.
    fn apply_status(&mut self, s: Status) -> Task<Msg> {
        let before = self.status.as_ref().map(|o| (o.signed_in, o.unlocked));
        let after = (s.signed_in, s.unlocked);
        // A login waiting for its second step (Vault reopened meanwhile).
        if self.signin.email.is_empty() && s.two_factor_pending {
            self.signin.email = s.email.clone();
        }
        self.status = Some(s);
        if before == Some(after) {
            return Task::none();
        }
        match after {
            (true, true) => {
                self.signin.clear_secrets();
                self.unlock = Unlock::default();
                Task::batch([
                    self.load_vault(),
                    self.ask(Ask::Settings, Request::Settings),
                    Field::Search.focus(),
                ])
            }
            (true, false) => {
                self.forget_vault();
                self.signin.clear_secrets();
                Field::Unlock.focus()
            }
            (false, _) => {
                self.forget_vault();
                self.unlock = Unlock::default();
                if self.signin.email.is_empty() {
                    Field::Email.focus()
                } else {
                    Field::Password.focus()
                }
            }
        }
    }

    /// Drop everything an unlocked vault showed: locked, signed out, or
    /// the service gone.
    fn forget_vault(&mut self) {
        self.items.clear();
        self.unreadable = 0;
        self.loaded = false;
        self.folders.clear();
        self.orgs.clear();
        self.collections.clear();
        self.selected = None;
        self.record = Pane::Empty;
        self.reprompt = None;
        self.verified.clear();
        self.revealed.clear();
        self.history_open = false;
        self.edit = None;
        self.popup = None;
        if !matches!(self.sheet, Some(Sheet::Confirm(sheets::Confirm::SignOut))) {
            self.sheet = None;
        }
        self.picker = None;
    }

    fn quit(&mut self) -> Task<Msg> {
        // A copied secret should not outlive Vault on the clipboard.
        match self.clip.take() {
            Some(guard) => {
                Task::perform(copy::clear_if_ours(guard), |_| Msg::Noop).chain(iced::exit())
            }
            None => iced::exit(),
        }
    }

    fn raise(&self) {
        if let Some(window_id) = self.float.any_window_id() {
            sola_kit::show_window(window_id);
        }
    }

    fn on_tick(&mut self) -> Task<Msg> {
        let now = Instant::now();
        self.now = when::now_unix();
        if self.copied.as_ref().is_some_and(|(_, until)| now >= *until) {
            self.copied = None;
        }
        if self.toast.as_ref().is_some_and(|t| now >= t.until) {
            self.toast = None;
        }
        match self.clip.take() {
            Some(guard) if guard.is_due(now) => {
                Task::perform(copy::clear_if_ours(guard), |_| Msg::Noop)
            }
            other => {
                self.clip = other;
                Task::none()
            }
        }
    }

    fn show_toast(&mut self, text: impl Into<String>) -> Task<Msg> {
        self.toast = Some(Toast {
            text: text.into(),
            until: Instant::now() + TOAST_FOR,
        });
        Task::none()
    }

    fn close_sheet(&mut self) {
        self.sheet = None;
        self.picker = None;
        self.popup = None;
    }

    // ── View ─────────────────────────────────────────────────────────

    pub fn view(&self) -> Element<'_, Msg> {
        let content: Element<'_, Msg> = match self.phase() {
            Phase::Unlocked => self.view_vault(),
            phase => self.view_gate(phase),
        };
        let body: Element<'_, Msg> = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::canvas)
            .into();
        let body = match &self.toast {
            Some(t) => stack![
                body,
                container(opaque(foot::toast(t)))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::Alignment::Center)
                    .align_y(iced::Alignment::End)
                    .padding(Padding {
                        bottom: 40.0,
                        ..Padding::from([SPACE_MD, SPACE_LG])
                    }),
            ]
            .into(),
            None => body,
        };
        let framed = sola_kit::window_frame(
            self.float.chrome_any(),
            self.frame_title(),
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            body,
        );
        let mut layers = vec![framed];
        if let Some(sheet) = &self.sheet {
            layers.push(self.view_sheet(sheet));
        }
        if let Some(picker) = &self.picker {
            layers.push(picker.overlay().map(Msg::Picker));
        }
        if layers.len() == 1 {
            return layers.pop().expect("framed");
        }
        iced::widget::Stack::with_children(layers).into()
    }

    fn frame_title(&self) -> String {
        match self.phase() {
            Phase::Unlocked => {
                if let Some(e) = &self.edit {
                    return e.title();
                }
                self.section_title()
            }
            _ => "Vault".into(),
        }
    }

    fn view_vault(&self) -> Element<'_, Msg> {
        let main = column![
            self.view_bar(),
            widgets::h_hairline(),
            row![self.view_list(), widgets::v_hairline(), self.view_record()].height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill);
        column![
            row![self.view_sidebar(), widgets::v_hairline(), main].height(Length::Fill),
            widgets::h_hairline(),
            self.view_foot(),
        ]
        .into()
    }
}
