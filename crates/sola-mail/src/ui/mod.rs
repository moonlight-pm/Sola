//! Kit UI: three-pane mail client (mailboxes · list · letter / compose).

mod chrome;
mod compose;
mod format;
mod keys;
mod list;
mod list_sync;
mod moves;
mod reader;
mod sidebar;
mod style;
mod widgets;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::Id as ScrollId;
use iced::widget::scrollable::Viewport;
use iced::widget::{Space, column, container, row, text_editor};
use iced::{Element, Event, Length, Padding, Subscription, Task, Theme, event, keyboard};
use sola_bus::Message;
use sola_bus::topics::{MailConfig, Topic, mail_addr_key};
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit};
use sola_kit::components::file_picker::{FilePicker, Message as PickerMsg, Outcome};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM};
use sola_kit::components::text as kit_text;
use sola_kit::theme::default_theme;

use crate::bridge::{self, mail_send};
use crate::protocol::from::pick_from_for_reply;
use crate::protocol::types::folder_label;
use crate::protocol::{Account, Folder, FolderRef, MailAttachment, MailId, MessageSummary, attachments};
use crate::snapshot::{self, Snapshot};
use crate::worker::{Flag, LinkState, MailCmd, MailEvent, Op, Page};
use chrome::Toast;
use compose::{Compose, Edit};
use list::{ListState, Load, SearchState};
use moves::{Confirmed, Moves};
use reader::{Letter, Reader};
use sidebar::AccountLink;

const APP_ID: &str = "sola-mail";
const PAGE: u32 = 50;
/// A quiet refresh re-fetches at most this many newest rows.
const REFRESH_MAX: u32 = 150;
/// Undo / status toast stays long enough to click Undo, then leaves.
const TOAST_TTL: Duration = Duration::from_secs(5);
/// Wait for rapid deletes to settle before fetching the next body.
const SETTLE_SELECT: Duration = Duration::from_millis(120);
/// Safety net when IDLE stays quiet (another client's expunge, other boxes).
const POLL: Duration = Duration::from_secs(45);
/// Shared list-header / reader-toolbar height so icons sit on one line.
const CHROME_H: f32 = 40.0;

fn list_scroll_id() -> ScrollId {
    ScrollId::new("mail-message-list")
}

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    Worker(MailEvent),
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
    KeyPressed {
        key: keyboard::Key,
        mods: keyboard::Modifiers,
        captured: bool,
    },

    SelectFolder(FolderRef),
    SelectMessage(MailId),
    /// After rapid delete/advance, load the body of the row we landed on.
    SettleSelect {
        id: MailId,
        generation: u64,
    },
    SearchChanged(String),
    SearchSubmit,
    SearchClear,
    ListScrolled(Viewport),
    /// Quiet background re-fetch (IDLE + multi-client safety net).
    PollRefresh,
    /// Get New Mail: reconnect failed accounts and refresh now.
    Refresh,

    OpenUrl(String),
    /// Visible letter selection (None = caret / cleared).
    BodySelect(Option<String>),
    CopyBody,
    RetryBody,
    OpenAttachment(usize),
    SaveAttachment(usize),

    MoveSelected(String),
    Undo,
    ToggleSeen,
    ToggleFlag,
    AskErase(String),
    ConfirmErase,
    CancelConfirm,

    Compose,
    Reply {
        all: bool,
    },
    Forward,
    EditDraft,
    Draft(Edit),
    Send,
    SaveDraft,
    CancelCompose,
    DiscardDraft,
    KeepEditing,
    Attach,
    RemoveAttach(usize),
    ClipboardPasted(Option<String>),
    Picker(PickerMsg),

    DismissToast,
    /// Auto-hide a toast after [`TOAST_TTL`]; ignored if a newer one replaced it.
    ToastExpired {
        generation: u64,
    },
}

#[derive(Debug, Clone, Copy)]
enum PickerKind {
    Attach,
    Save { index: usize },
}

/// Mail is usable only once a config with an IMAP account has arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Setup {
    Unknown,
    Missing,
    Ready,
}

pub struct App {
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
    config: MailConfig,
    setup: Setup,
    links: Vec<AccountLink>,
    folders: Vec<Folder>,
    smart: Vec<Folder>,
    list: ListState,
    search_input: String,
    selected: Option<MailId>,
    reader: Reader,
    compose: Option<Box<Compose>>,
    moves: Moves,
    toast: Option<Toast>,
    /// Bumped on every toast so a late expiry cannot hide a newer one.
    toast_gen: u64,
    /// Mailbox waiting on "Erase every message?".
    confirm: Option<String>,
    picker: Option<(PickerKind, FilePicker)>,
    last_picker_dir: Option<PathBuf>,
    /// Last inbox unread we published on `Topic::MailStatus`.
    published_unread: Option<u32>,
    /// Bumped on every selection change so delayed body fetches can cancel.
    select_gen: u64,
}

fn empty_mailboxes() -> Vec<Folder> {
    crate::protocol::boxes::CANONICAL
        .iter()
        .map(|n| Folder {
            name: (*n).to_string(),
            unread: 0,
            total: 0,
        })
        .collect()
}

impl App {
    pub fn boot() -> (Self, Task<Msg>) {
        // Open on the last list instead of a blocking Connecting screen.
        let snap = snapshot::load();
        let (list, folders) = match snap {
            Some(s) => (
                ListState::new(s.folder, s.rows, s.total),
                Some(s.folders).filter(|f| !f.is_empty()).unwrap_or_else(empty_mailboxes),
            ),
            None => (ListState::new(FolderRef::inbox(), Vec::new(), 0), empty_mailboxes()),
        };
        let app = Self {
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            config: MailConfig::default(),
            setup: Setup::Unknown,
            links: Vec::new(),
            folders,
            smart: Vec::new(),
            list,
            search_input: String::new(),
            selected: None,
            reader: Reader::Empty,
            compose: None,
            moves: Moves::default(),
            toast: None,
            toast_gen: 0,
            confirm: None,
            picker: None,
            last_picker_dir: None,
            published_unread: None,
            select_gen: 0,
        };
        (app, sola_kit::window_ready_task(Msg::WindowReady))
    }

    pub fn title(&self) -> String {
        "Mail".into()
    }

    pub fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        let poll = if self.any_ready() {
            iced::time::every(POLL).map(|_| Msg::PollRefresh)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            bridge::mail_subscription().map(Msg::Worker),
            event::listen_with(|event, status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    Some(Msg::KeyPressed {
                        key,
                        mods: modifiers,
                        captured: status == event::Status::Captured,
                    })
                }
                _ => None,
            }),
            poll,
        ])
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        let needs_letter = matches!(msg, Msg::Reply { .. } | Msg::Forward | Msg::EditDraft);
        if needs_letter && self.reader.body().is_none() {
            return Task::none();
        }
        match msg {
            Msg::Bus(message) => return self.on_bus(&message),
            Msg::Worker(ev) => return self.on_worker(ev),
            Msg::WindowReady(id) => self.window_id = id,
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => sola_kit::close_app(APP_ID),
            Msg::KeyPressed {
                key,
                mods,
                captured,
            } => return self.on_key(key, mods, captured),

            Msg::SelectFolder(folder) => self.open_folder(folder),
            Msg::SelectMessage(id) => self.request_select(id),
            Msg::SettleSelect { id, generation } => {
                if generation == self.select_gen && self.selected.as_ref() == Some(&id) {
                    self.open_message(id);
                }
            }
            Msg::SearchChanged(q) => self.search_input = q,
            Msg::SearchSubmit => self.submit_search(),
            Msg::SearchClear => self.clear_search(),
            Msg::ListScrolled(vp) => self.on_scroll(vp),
            Msg::PollRefresh => self.refresh(false),
            Msg::Refresh => {
                mail_send(MailCmd::Retry);
                self.refresh(true);
            }

            Msg::OpenUrl(url) => sola_core::open_url_logged(&url),
            Msg::BodySelect(sel) => {
                if let Some(l) = self.reader.letter_mut() {
                    l.selection = sel;
                }
            }
            Msg::CopyBody => return self.copy_letter(),
            Msg::RetryBody => {
                if let Some(id) = self.selected.clone() {
                    self.reader = Reader::Loading;
                    mail_send(MailCmd::FetchBody(id));
                }
            }
            Msg::OpenAttachment(i) => return self.open_attachment(i),
            Msg::SaveAttachment(i) => self.begin_save_attachment(i),

            Msg::MoveSelected(dest) => {
                if let Some(id) = self.selected.clone().filter(|_| self.compose.is_none()) {
                    return self.move_and_advance(id, dest);
                }
            }
            Msg::Undo => return self.undo(),
            Msg::ToggleSeen => self.toggle_flag(Flag::Seen),
            Msg::ToggleFlag => self.toggle_flag(Flag::Flagged),
            Msg::AskErase(folder) => self.confirm = Some(folder),
            Msg::ConfirmErase => {
                if let Some(folder) = self.confirm.take() {
                    return self.erase(folder);
                }
            }
            Msg::CancelConfirm => self.confirm = None,

            Msg::Compose => self.start_compose(|app| compose::blank(app.default_from())),
            Msg::Reply { all } => self.start_compose(|app| {
                let body = app.reader.body().expect("guarded in update");
                let from = app.reply_from(body);
                compose::reply(body, from, all, &|a| app.is_self(a))
            }),
            Msg::Forward => self.start_compose(|app| {
                let body = app.reader.body().expect("guarded in update");
                compose::forward(body, app.reply_from(body))
            }),
            Msg::EditDraft => self.start_compose(|app| {
                let body = app.reader.body().expect("guarded in update");
                let from = app
                    .identities()
                    .into_iter()
                    .find(|i| mail_addr_key(i) == mail_addr_key(&body.from))
                    .unwrap_or_else(|| app.default_from());
                compose::reopen(body, from)
            }),
            Msg::Draft(edit) => {
                if let Some(c) = self.compose.as_mut() {
                    c.apply(edit);
                }
            }
            Msg::Send => return self.send(),
            Msg::SaveDraft => self.save_draft(),
            Msg::CancelCompose => {
                if let Some(c) = self.compose.as_mut() {
                    if c.dirty() {
                        c.confirm_discard = true;
                    } else {
                        self.close_compose();
                    }
                }
            }
            Msg::DiscardDraft => self.close_compose(),
            Msg::KeepEditing => {
                if let Some(c) = self.compose.as_mut() {
                    c.confirm_discard = false;
                    c.then_open = None;
                }
            }
            Msg::Attach => self.begin_attach(),
            Msg::RemoveAttach(i) => {
                if let Some(c) = self.compose.as_mut()
                    && i < c.draft.attachments.len() {
                        c.draft.attachments.remove(i);
                        c.touch();
                    }
            }
            Msg::ClipboardPasted(Some(t)) => {
                if let Some(c) = self.compose.as_mut() {
                    c.apply(Edit::Body(text_editor::Action::Edit(text_editor::Edit::Paste(
                        Arc::new(t),
                    ))));
                }
            }
            Msg::ClipboardPasted(None) => {}
            Msg::Picker(m) => return self.on_picker(m),

            Msg::DismissToast => self.clear_toast(),
            Msg::ToastExpired { generation } => {
                if self.toast_gen == generation {
                    self.clear_toast();
                }
            }
        }
        Task::none()
    }

    // ── Bus ───────────────────────────────────────────────────────────

    fn on_bus(&mut self, message: &Message) -> Task<Msg> {
        self.float.update(message);
        apply_theme_update(message, &mut self.theme);
        if is_self_quit(message, APP_ID) {
            return self.quit();
        }
        match Topic::parse(message) {
            Some(Topic::MenuAction(p)) if p.app_id == APP_ID => self.on_menu_action(&p.action_id),
            Some(Topic::MailConfig(cfg)) => {
                self.apply_config(cfg);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn apply_config(&mut self, cfg: MailConfig) {
        let accounts = Account::imap_accounts(&cfg);
        if accounts.is_empty() {
            self.setup = Setup::Missing;
            self.links.clear();
        } else {
            self.setup = Setup::Ready;
            self.links = accounts
                .iter()
                .map(|a| {
                    let id = a.id();
                    let state = self
                        .links
                        .iter()
                        .find(|l| l.id == id)
                        .map(|l| l.state.clone())
                        .unwrap_or(LinkState::Connecting);
                    AccountLink {
                        id,
                        email: a.email.clone(),
                        state,
                    }
                })
                .collect();
            // Rows of accounts that are gone (or from a snapshot of another
            // account set) must not linger.
            let ids: Vec<String> = accounts.iter().map(Account::id).collect();
            self.list.rows.retain(|m| ids.contains(&m.id.account));
        }
        self.config = cfg.clone();
        mail_send(MailCmd::Reconfigure(cfg));
    }

    fn on_menu_action(&mut self, action: &str) -> Task<Msg> {
        let msg = match action {
            "cut" => return self.edit_cut(),
            "copy" => return self.edit_copy(),
            "paste" => return self.edit_paste(),
            "select_all" => {
                self.select_all();
                return Task::none();
            }
            "quit" => return self.quit(),
            "refresh" => Msg::Refresh,
            "compose" => Msg::Compose,
            "save_draft" => Msg::SaveDraft,
            "attach" => Msg::Attach,
            "reply" => Msg::Reply { all: false },
            "reply_all" => Msg::Reply { all: true },
            "forward" => Msg::Forward,
            "toggle_seen" => Msg::ToggleSeen,
            "toggle_flag" => Msg::ToggleFlag,
            "archive" => Msg::MoveSelected("Archive".into()),
            "trash" => Msg::MoveSelected("Trash".into()),
            "junk" => Msg::MoveSelected("Junk".into()),
            "inbox" => Msg::MoveSelected("INBOX".into()),
            "undo" => Msg::Undo,
            "copy_message" => Msg::CopyBody,
            "empty_junk" => Msg::AskErase("Junk".into()),
            "empty_trash" => Msg::AskErase("Trash".into()),
            "next" => return self.select_next(true),
            "prev" => return self.select_prev(true),
            _ => return Task::none(),
        };
        self.update(msg)
    }

    fn quit(&mut self) -> Task<Msg> {
        self.retract_mail_status();
        mail_send(MailCmd::Shutdown);
        iced::exit()
    }

    // ── Worker events ─────────────────────────────────────────────────

    fn on_worker(&mut self, ev: MailEvent) -> Task<Msg> {
        match ev {
            MailEvent::AccountLink {
                account,
                email,
                state,
            } => match self.links.iter_mut().find(|l| l.id == account) {
                Some(link) => link.state = state,
                None => self.links.push(AccountLink {
                    id: account,
                    email,
                    state,
                }),
            },
            MailEvent::Changed => self.refresh(true),
            MailEvent::NewMail => self.refresh(true),
            MailEvent::NotConfigured => {
                self.setup = Setup::Missing;
                self.links.clear();
                self.folders = empty_mailboxes();
                self.smart.clear();
                self.list.rows.clear();
                self.list.total = 0;
                self.selected = None;
                self.reader = Reader::Empty;
                self.publish_inbox_unread();
            }
            MailEvent::Folders { folders, smart } => {
                self.folders = folders;
                self.smart = smart;
                if let FolderRef::Smart(name) = &self.list.folder
                    && !self.smart.iter().any(|f| f.name == *name) {
                        self.open_folder(FolderRef::inbox());
                    }
                self.publish_inbox_unread();
            }
            MailEvent::Messages {
                epoch,
                folder,
                page,
                rows,
                total,
                failed,
            } => self.on_messages(epoch, folder, page, rows, total, failed),
            MailEvent::SearchResults {
                epoch,
                rows,
                capped,
            } => {
                if epoch != self.list.epoch {
                    return Task::none();
                }
                if let Some(search) = self.list.search.as_mut() {
                    search.capped = capped;
                    let gone = self.moves.gone();
                    self.list.rows = rows.into_iter().filter(|m| !gone.contains(&m.id)).collect();
                    self.list.load = Load::Idle;
                }
            }
            MailEvent::Body(body) => {
                if self.selected.as_ref() == Some(&body.id) {
                    tracing::debug!(
                        uid = body.id.uid,
                        blocks = body.blocks.len(),
                        attachments = body.attachments.len(),
                        "opened message body"
                    );
                    self.reader = Reader::Open(Box::new(Letter {
                        body: *body,
                        selection: None,
                        select_all: 0,
                    }));
                }
            }
            MailEvent::Moved { id, dest, dest_uid } => match self.moves.confirmed(&id, &dest, dest_uid) {
                Confirmed::Nothing => {}
                Confirmed::Reverse(id, dest) => mail_send(MailCmd::Move { id, dest }),
                Confirmed::Restored { old, new } => self.rewrite_id(&old, new),
                Confirmed::UndoImpossible => {
                    return self.show_toast("Couldn’t undo: the moved copy wasn’t found", false);
                }
            },
            MailEvent::MoveFailed { id, message } => {
                if let Some((summary, dest)) = self.moves.failed(&id) {
                    self.restore_row(summary, &dest);
                }
                return self.show_toast(format!("Couldn’t move the message: {message}"), false);
            }
            MailEvent::Emptied { folder, failed } => {
                mail_send(MailCmd::ListFolders);
                if self.list.folder.is(&folder) {
                    self.open_folder(self.list.folder.clone());
                }
                let label = folder_label(&folder);
                let text = if failed.is_empty() {
                    format!("{label} erased")
                } else {
                    format!("{label} erased, except {}", failed.join("; "))
                };
                return self.show_toast(text, false);
            }
            MailEvent::Sent => {
                self.compose = None;
                self.picker = None;
                mail_send(MailCmd::ListFolders);
                return self.show_toast("Message sent", false);
            }
            MailEvent::DraftSaved => {
                let then = self.compose.take().and_then(|c| c.then_open);
                self.picker = None;
                if self.list.folder.is("Drafts") {
                    self.refresh(true);
                }
                if let Some(id) = then {
                    self.open_message(id);
                }
                return self.show_toast("Draft saved", false);
            }
            MailEvent::Failed { op, message } => return self.on_failed(op, message),
        }
        Task::none()
    }

    fn on_messages(
        &mut self,
        epoch: u64,
        folder: FolderRef,
        page: Page,
        rows: Vec<MessageSummary>,
        total: u32,
        failed: Vec<String>,
    ) {
        if epoch != self.list.epoch || folder != self.list.folder || self.list.search.is_some() {
            return;
        }
        self.list.load = Load::Idle;
        let mailbox = folder.mailbox().to_string();
        match page {
            Page::Replace | Page::Refresh => {
                self.moves.prune(&mailbox, &rows);
                let gone = self.moves.gone();
                self.list.rows = if page == Page::Replace {
                    rows.into_iter().filter(|m| !gone.contains(&m.id)).collect()
                } else {
                    list_sync::merge_refresh(&self.list.rows, rows, total, gone, &failed)
                };
            }
            Page::More => {
                self.list.exhausted = rows.is_empty();
                list_sync::append_more(&mut self.list.rows, rows, self.moves.gone());
            }
        }
        // The server still counts our not-yet-confirmed moves.
        let pending = self
            .moves
            .gone()
            .iter()
            .filter(|id| id.folder.eq_ignore_ascii_case(&mailbox))
            .count() as u32;
        self.list.total = total.saturating_sub(pending);
        if self.selected.as_ref().is_some_and(|id| self.list.index_of(id).is_none())
            && self.compose.is_none()
            && page != Page::More
        {
            // Moved / expunged elsewhere.
            self.selected = None;
            self.reader = Reader::Empty;
        }
        self.persist_snapshot();
    }

    fn on_failed(&mut self, op: Op, message: String) -> Task<Msg> {
        match op {
            Op::List => {
                tracing::debug!("list: {message}");
                self.list.load = Load::Idle;
                Task::none()
            }
            Op::Search => {
                self.list.load = Load::Idle;
                self.show_toast(format!("Search failed: {message}"), false)
            }
            Op::Open(id) => {
                if self.selected.as_ref() == Some(&id) {
                    self.reader = Reader::Failed(message);
                }
                Task::none()
            }
            Op::Flag(_) => {
                self.refresh(true);
                self.show_toast(format!("Couldn’t update the message: {message}"), false)
            }
            Op::Erase => {
                if self.list.erasable().is_some() {
                    self.open_folder(self.list.folder.clone());
                }
                self.show_toast(format!("Couldn’t erase: {message}"), false)
            }
            Op::Send => {
                if let Some(c) = self.compose.as_mut() {
                    c.busy = None;
                }
                self.show_toast(format!("Couldn’t send: {message}"), false)
            }
            Op::SaveDraft => {
                if let Some(c) = self.compose.as_mut() {
                    c.busy = None;
                }
                self.show_toast(format!("Couldn’t save the draft: {message}"), false)
            }
        }
    }

    // ── List ──────────────────────────────────────────────────────────

    fn open_folder(&mut self, folder: FolderRef) {
        self.list.folder = folder;
        self.list.search = None;
        self.search_input.clear();
        self.list.rows.clear();
        self.list.scroll_y = 0.0;
        if self.compose.is_none() {
            self.selected = None;
            self.reader = Reader::Empty;
        }
        self.select_gen += 1;
        self.request(Page::Replace);
    }

    /// Ask for a page of the current list.
    fn request(&mut self, page: Page) {
        let (limit, cursors) = match page {
            Page::Replace => {
                self.list.epoch += 1;
                self.list.load = Load::Replacing;
                self.list.exhausted = false;
                (PAGE, Vec::new())
            }
            Page::Refresh => {
                self.list.load = Load::Refreshing;
                let loaded = self.list.rows.len() as u32;
                (loaded.clamp(PAGE, REFRESH_MAX), Vec::new())
            }
            Page::More => {
                self.list.load = Load::More;
                (PAGE, list_sync::cursors(&self.list.rows))
            }
        };
        mail_send(MailCmd::List {
            epoch: self.list.epoch,
            folder: self.list.folder.clone(),
            page,
            cursors,
            limit,
        });
    }

    /// Folder counts + the newest rows. `force` supersedes a page in
    /// flight (new mail, accounts changed); the poll waits its turn.
    fn refresh(&mut self, force: bool) {
        if self.setup != Setup::Ready || !self.any_ready() {
            return;
        }
        mail_send(MailCmd::ListFolders);
        if self.list.search.is_some() {
            return;
        }
        if self.list.load != Load::Idle {
            if !force {
                return;
            }
            self.list.epoch += 1;
        }
        self.request(if self.list.rows.is_empty() {
            Page::Replace
        } else {
            Page::Refresh
        });
    }

    fn on_scroll(&mut self, vp: Viewport) {
        let y = vp.absolute_offset().y;
        let scrolled_down = y > self.list.scroll_y + 1.0;
        self.list.scroll_y = y;
        let remain = vp.content_bounds().height - vp.bounds().height - y;
        let list = &self.list;
        if scrolled_down
            && remain < 280.0
            && list.load == Load::Idle
            && list.search.is_none()
            && !list.exhausted
            && (list.rows.len() as u32) < list.total
        {
            self.request(Page::More);
        }
    }

    fn submit_search(&mut self) {
        let query = self.search_input.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.list.search = Some(SearchState {
            query: query.clone(),
            capped: false,
        });
        self.list.epoch += 1;
        self.list.load = Load::Replacing;
        self.list.rows.clear();
        self.list.scroll_y = 0.0;
        let also = match &self.list.folder {
            FolderRef::Mailbox(name) if matches!(name.as_str(), "Junk" | "Trash") => Some(name.clone()),
            _ => None,
        };
        mail_send(MailCmd::Search {
            epoch: self.list.epoch,
            query,
            also,
        });
    }

    fn clear_search(&mut self) {
        self.search_input.clear();
        if self.list.search.is_some() {
            self.open_folder(self.list.folder.clone());
        }
    }

    // ── Selection ─────────────────────────────────────────────────────

    /// A click on a row. A draft with edits asks first.
    fn request_select(&mut self, id: MailId) {
        if let Some(c) = self.compose.as_mut() {
            if c.dirty() {
                c.confirm_discard = true;
                c.then_open = Some(id);
                return;
            }
            self.close_compose();
        }
        self.open_message(id);
    }

    fn open_message(&mut self, id: MailId) {
        self.select_gen += 1;
        self.selected = Some(id.clone());
        if self.reader.body().is_none_or(|b| b.id != id) {
            self.reader = Reader::Loading;
        }
        mail_send(MailCmd::FetchBody(id.clone()));
        let unread = self.list.row_mut(&id).filter(|m| !m.seen).map(|m| m.seen = true);
        if unread.is_some() {
            mail_send(MailCmd::SetFlag {
                id: id.clone(),
                flag: Flag::Seen,
                on: true,
            });
            self.bump_unread(&id, -1);
        }
    }

    fn select_prev(&mut self, from_end: bool) -> Task<Msg> {
        let target = match self.selected.as_ref().map(|id| self.list.index_of(id)) {
            Some(Some(i)) if i > 0 => Some(self.list.rows[i - 1].id.clone()),
            None if from_end => self.list.rows.last().map(|m| m.id.clone()),
            _ => None,
        };
        if let Some(id) = target {
            self.open_message(id);
        }
        Task::none()
    }

    fn select_next(&mut self, from_start: bool) -> Task<Msg> {
        let target = match self.selected.as_ref().map(|id| self.list.index_of(id)) {
            Some(Some(i)) => self.list.rows.get(i + 1).map(|m| m.id.clone()),
            None if from_start => self.list.rows.first().map(|m| m.id.clone()),
            _ => None,
        };
        if let Some(id) = target {
            self.open_message(id);
        }
        Task::none()
    }

    fn toggle_flag(&mut self, flag: Flag) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let Some(m) = self.list.row_mut(&id) else {
            return;
        };
        let on = match flag {
            Flag::Seen => {
                m.seen = !m.seen;
                m.seen
            }
            Flag::Flagged => {
                m.flagged = !m.flagged;
                m.flagged
            }
        };
        mail_send(MailCmd::SetFlag {
            id: id.clone(),
            flag,
            on,
        });
        if flag == Flag::Seen {
            self.bump_unread(&id, if on { -1 } else { 1 });
        }
    }

    // ── Moves ─────────────────────────────────────────────────────────

    fn move_and_advance(&mut self, id: MailId, dest: String) -> Task<Msg> {
        if id.folder.eq_ignore_ascii_case(&dest) {
            return Task::none();
        }
        let idx = self.list.index_of(&id);
        let summary = idx.map(|i| self.list.rows[i].clone());
        self.moves.begin(id.clone(), dest.clone(), summary.clone());
        if let Some(s) = &summary {
            self.adjust_counts(s, &id.folder, &dest, true);
            self.list.total = self.list.total.saturating_sub(1);
            self.list.rows.retain(|m| m.id != id);
        }
        mail_send(MailCmd::Move {
            id,
            dest: dest.clone(),
        });
        let toast = self.show_toast(format!("Moved to {}", folder_label(&dest)), true);
        self.select_gen += 1;
        let Some(i) = idx.filter(|_| !self.list.rows.is_empty()) else {
            self.selected = None;
            self.reader = Reader::Empty;
            return toast;
        };
        let next = self.list.rows[i.saturating_sub(1).min(self.list.rows.len() - 1)]
            .id
            .clone();
        self.selected = Some(next.clone());
        self.reader = Reader::Loading;
        let generation = self.select_gen;
        Task::batch([
            toast,
            Task::perform(tokio::time::sleep(SETTLE_SELECT), move |_| Msg::SettleSelect {
                id: next,
                generation,
            }),
        ])
    }

    fn undo(&mut self) -> Task<Msg> {
        let Some(undo) = self.moves.undo() else {
            return Task::none();
        };
        if let Some(summary) = undo.summary {
            self.restore_row(summary, &undo.to);
        }
        if let Some((id, dest)) = undo.reverse {
            mail_send(MailCmd::Move { id, dest });
        }
        self.show_toast("Move undone", false)
    }

    /// Put a row back after a failed move / undo (counts too).
    fn restore_row(&mut self, summary: MessageSummary, moved_to: &str) {
        let from = summary.id.folder.clone();
        self.adjust_counts(&summary, &from, moved_to, false);
        let shown = self.list.search.is_some() || self.list.folder.mailbox().eq_ignore_ascii_case(&from);
        if shown {
            self.list.total = self.list.total.saturating_add(1);
            list_sync::insert(&mut self.list.rows, summary);
        }
    }

    /// The undone message now lives under a new UID.
    fn rewrite_id(&mut self, old: &MailId, new: MailId) {
        if let Some(m) = self.list.row_mut(old) {
            m.id = new.clone();
        }
        if self.selected.as_ref() == Some(old) {
            self.selected = Some(new.clone());
        }
        if let Some(l) = self.reader.letter_mut()
            && l.body.id == *old {
                l.body.id = new;
            }
        self.list.rows.sort_by(MessageSummary::cmp_recency);
    }

    fn adjust_counts(&mut self, m: &MessageSummary, from: &str, dest: &str, leaving: bool) {
        let unread = i32::from(!m.seen);
        let sign = if leaving { -1 } else { 1 };
        if let FolderRef::Smart(name) = &self.list.folder {
            bump(&mut self.smart, name, sign, sign * unread);
        } else {
            bump(&mut self.folders, from, sign, sign * unread);
        }
        bump(&mut self.folders, dest, -sign, -sign * unread);
        self.publish_inbox_unread();
    }

    fn bump_unread(&mut self, id: &MailId, delta: i32) {
        match &self.list.folder {
            FolderRef::Smart(name) => bump(&mut self.smart, name, 0, delta),
            FolderRef::Mailbox(_) => bump(&mut self.folders, &id.folder, 0, delta),
        }
        self.publish_inbox_unread();
    }

    fn erase(&mut self, folder: String) -> Task<Msg> {
        if self.list.folder.is(&folder) {
            self.list.rows.clear();
            self.list.load = Load::Replacing;
            self.moves.clear();
            self.selected = None;
            self.reader = Reader::Empty;
        }
        let toast = self.show_toast(format!("Erasing {}…", folder_label(&folder)), false);
        mail_send(MailCmd::Empty { folder });
        toast
    }

    // ── Compose ───────────────────────────────────────────────────────

    /// Open the compose pane with `make(self)`. Reply / forward / edit
    /// need an open letter; an edited draft is never replaced silently.
    fn start_compose(&mut self, make: impl FnOnce(&Self) -> compose::Draft) {
        if let Some(c) = self.compose.as_mut()
            && c.dirty() {
                c.confirm_discard = true;
                return;
            }
        let draft = make(self);
        self.compose = Some(Box::new(Compose::new(draft)));
    }

    fn close_compose(&mut self) {
        let then = self.compose.take().and_then(|c| c.then_open);
        self.picker = None;
        if let Some(id) = then {
            self.open_message(id);
        }
    }

    fn send(&mut self) -> Task<Msg> {
        let Some(c) = self.compose.as_mut() else {
            return Task::none();
        };
        if c.busy.is_some() {
            return Task::none();
        }
        if c.draft.to.trim().is_empty() {
            return self.show_toast("Add at least one recipient", false);
        }
        c.busy = Some(compose::Busy::Sending);
        c.confirm_discard = false;
        mail_send(MailCmd::Send(c.outgoing()));
        Task::none()
    }

    fn save_draft(&mut self) {
        let Some(c) = self.compose.as_mut() else {
            return;
        };
        if c.busy.is_some() {
            return;
        }
        c.busy = Some(compose::Busy::Saving);
        c.confirm_discard = false;
        mail_send(MailCmd::SaveDraft(c.outgoing()));
    }

    fn begin_attach(&mut self) {
        if self.compose.is_none() {
            self.compose = Some(Box::new(Compose::new(compose::blank(self.default_from()))));
        }
        let picker = FilePicker::open()
            .title("Attach file")
            .start_dir(self.picker_start_dir());
        self.picker = Some((PickerKind::Attach, picker));
    }

    fn attach_path(&mut self, path: PathBuf) -> Task<Msg> {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => return self.show_toast(format!("Can’t attach: {e}"), false),
        };
        let Some(c) = self.compose.as_mut() else {
            return Task::none();
        };
        let filename = attachments::sanitize_filename(
            &path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "attachment".into()),
        );
        c.draft.attachments.push(MailAttachment {
            mime: attachments::mime_from_filename(&filename),
            filename,
            size: bytes.len() as u64,
            bytes: Arc::from(bytes),
        });
        c.touch();
        Task::none()
    }

    fn identities(&self) -> Vec<String> {
        self.config.from_addresses()
    }

    fn default_from(&self) -> String {
        self.identities()
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| self.config.primary_from_address())
    }

    fn reply_from(&self, body: &crate::protocol::MessageBody) -> String {
        pick_from_for_reply(&self.identities(), &body.to, &body.cc)
            .unwrap_or_else(|| self.default_from())
    }

    /// One of our own addresses (identities or IMAP accounts).
    fn is_self(&self, addr: &str) -> bool {
        let key = mail_addr_key(addr);
        !key.is_empty()
            && (self.identities().iter().any(|i| mail_addr_key(i) == key)
                || self.links.iter().any(|l| mail_addr_key(&l.email) == key))
    }

    // ── Attachments ───────────────────────────────────────────────────

    fn picker_start_dir(&self) -> PathBuf {
        self.last_picker_dir
            .clone()
            .unwrap_or_else(attachments::downloads_dir)
    }

    fn attachment(&self, index: usize) -> Option<&MailAttachment> {
        self.reader.body().and_then(|b| b.attachments.get(index))
    }

    fn open_attachment(&mut self, index: usize) -> Task<Msg> {
        let Some(att) = self.attachment(index) else {
            return Task::none();
        };
        match attachments::write_open_temp(&att.filename, &att.bytes) {
            Ok(path) => {
                attachments::open_path(&path);
                Task::none()
            }
            Err(e) => self.show_toast(e, false),
        }
    }

    fn begin_save_attachment(&mut self, index: usize) {
        let Some(att) = self.attachment(index) else {
            return;
        };
        let picker = FilePicker::save()
            .title("Save attachment")
            .start_dir(self.picker_start_dir())
            .suggested_name(att.filename.clone());
        self.picker = Some((PickerKind::Save { index }, picker));
    }

    fn on_picker(&mut self, msg: PickerMsg) -> Task<Msg> {
        let Some((kind, picker)) = self.picker.as_mut() else {
            return Task::none();
        };
        let kind = *kind;
        match picker.update(msg) {
            Some(Outcome::Cancelled) => {
                self.picker = None;
                Task::none()
            }
            Some(Outcome::Picked(path)) => {
                if let Some(parent) = path.parent() {
                    self.last_picker_dir = Some(parent.to_path_buf());
                }
                self.picker = None;
                match kind {
                    PickerKind::Attach => self.attach_path(path),
                    PickerKind::Save { index } => {
                        let result = self
                            .attachment(index)
                            .map(|att| (att.filename.clone(), std::fs::write(&path, att.bytes.as_ref())));
                        match result {
                            Some((name, Ok(()))) => self.show_toast(format!("Saved {name}"), false),
                            Some((_, Err(e))) => self.show_toast(format!("Can’t save: {e}"), false),
                            None => self.show_toast("That letter is no longer open", false),
                        }
                    }
                }
            }
            None => Task::none(),
        }
    }

    // ── Edit menu ─────────────────────────────────────────────────────

    fn copy_letter(&self) -> Task<Msg> {
        match self.reader.body().map(|b| b.display_text()) {
            Some(t) if !t.is_empty() => iced::clipboard::write(t),
            _ => Task::none(),
        }
    }

    fn edit_copy(&self) -> Task<Msg> {
        if let Some(c) = &self.compose {
            let t = c
                .draft
                .body
                .selection()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| c.draft.body.text());
            return if t.is_empty() {
                Task::none()
            } else {
                iced::clipboard::write(t)
            };
        }
        match self.reader.letter().and_then(|l| l.selection.clone()) {
            Some(sel) if !sel.is_empty() => iced::clipboard::write(sel),
            // No selection → copy the open letter (flatten includes URLs).
            _ => self.copy_letter(),
        }
    }

    fn edit_cut(&mut self) -> Task<Msg> {
        let Some(c) = self.compose.as_mut() else {
            return Task::none();
        };
        let Some(sel) = c.draft.body.selection().filter(|s| !s.is_empty()) else {
            return Task::none();
        };
        c.apply(Edit::Body(text_editor::Action::Edit(text_editor::Edit::Delete)));
        iced::clipboard::write(sel)
    }

    fn edit_paste(&self) -> Task<Msg> {
        if self.compose.is_none() {
            return Task::none();
        }
        iced::clipboard::read().map(Msg::ClipboardPasted)
    }

    fn select_all(&mut self) {
        if let Some(c) = self.compose.as_mut() {
            c.apply(Edit::Body(text_editor::Action::SelectAll));
        } else if let Some(l) = self.reader.letter_mut() {
            l.select_all += 1;
            let visible = sola_kit::components::prose::visible_text(&l.body.blocks);
            l.selection = (!visible.is_empty()).then_some(visible);
        }
    }

    // ── Toast ─────────────────────────────────────────────────────────

    fn show_toast(&mut self, text: impl Into<String>, undo: bool) -> Task<Msg> {
        self.toast = Some(Toast {
            text: text.into(),
            undo,
        });
        self.toast_gen += 1;
        let generation = self.toast_gen;
        Task::perform(tokio::time::sleep(TOAST_TTL), move |_| Msg::ToastExpired {
            generation,
        })
    }

    fn clear_toast(&mut self) {
        self.toast = None;
        self.toast_gen += 1;
    }

    // ── Accounts / status ─────────────────────────────────────────────

    fn any_ready(&self) -> bool {
        self.links.iter().any(|l| l.state == LinkState::Ready)
    }

    fn connected_accounts(&self) -> usize {
        self.links.iter().filter(|l| l.state == LinkState::Ready).count()
    }

    fn account_addresses(&self) -> Vec<String> {
        self.links.iter().map(|l| l.email.clone()).collect()
    }

    fn publish_inbox_unread(&mut self) {
        let n = self
            .folders
            .iter()
            .find(|f| f.name == "INBOX")
            .map_or(0, |f| f.unread);
        if self.published_unread == Some(n) {
            return;
        }
        self.published_unread = Some(n);
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let _ = bus.emit(Topic::MailStatus(sola_bus::topics::MailStatus {
                inbox_unread: n,
            }));
        }
    }

    fn retract_mail_status(&mut self) {
        self.published_unread = None;
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            let _ = bus.retract(Topic::MailStatus(sola_bus::topics::MailStatus {
                inbox_unread: 0,
            }));
        }
    }

    fn persist_snapshot(&self) {
        if self.list.rows.is_empty() || self.list.search.is_some() || self.setup != Setup::Ready {
            return;
        }
        snapshot::save(&Snapshot {
            version: snapshot::VERSION,
            folder: self.list.folder.clone(),
            folders: self.folders.clone(),
            rows: self.list.rows.clone(),
            total: self.list.total,
        });
    }

    // ── View ──────────────────────────────────────────────────────────

    pub fn view(&self) -> Element<'_, Msg> {
        let content: Element<'_, Msg> = if self.setup == Setup::Missing {
            container(
                column![
                    kit_text::heading("Mail is not set up"),
                    kit_text::caption("Add an account in Settings → Mail.").style(kit_text::muted),
                ]
                .spacing(SPACE_SM),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(style::canvas)
            .into()
        } else {
            let pane = match &self.compose {
                Some(c) => c.view(&self.identities()),
                None => self.view_reader(),
            };
            row![
                self.view_sidebar(),
                widgets::v_hairline(),
                self.view_list(),
                widgets::v_hairline(),
                column![self.view_toolbar(), pane]
                    .width(Length::Fill)
                    .height(Length::Fill),
            ]
            .height(Length::Fill)
            .into()
        };

        let mut col = column![content].width(Length::Fill).height(Length::Fill);
        if self.setup == Setup::Ready && self.show_link_card() {
            col = col.push(
                row![Space::new().width(Length::Fill), self.view_link_card()].padding(Padding {
                    top: 0.0,
                    right: SPACE_LG,
                    bottom: SPACE_MD,
                    left: SPACE_LG,
                }),
            );
        }
        if let Some(folder) = &self.confirm {
            col = col.push(self.view_erase_confirm(folder));
        } else if let Some(toast) = &self.toast {
            col = col.push(self.view_toast(toast));
        }
        let body: Element<'_, Msg> = container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::canvas)
            .into();

        let framed = sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            "Mail",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            body,
        );
        match &self.picker {
            Some((_, picker)) => iced::widget::stack![framed, picker.overlay().map(Msg::Picker)].into(),
            None => framed,
        }
    }
}

fn bump(folders: &mut [Folder], name: &str, d_total: i32, d_unread: i32) {
    let Some(f) = folders.iter_mut().find(|f| f.name.eq_ignore_ascii_case(name)) else {
        return;
    };
    f.total = f.total.saturating_add_signed(d_total);
    f.unread = f.unread.saturating_add_signed(d_unread);
}
