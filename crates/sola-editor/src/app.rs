//! The application: a window per open file (each a row in the shell's bar),
//! its menus, and everything that isn't editing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::widget::{Space, canvas, column, container, row, stack, text};
use iced::{Alignment, Element, Event, Length, Padding, Size, Subscription, Task, Theme, event, keyboard, window};
use sola_bus::topics::{MenuDefinition, MenuItem, Topic, TopicKind};
use sola_core::{KeyChord, KeyCode};
use sola_kit::app::{BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup};
use sola_kit::components::file_picker::{FilePicker, Message as PickerMsg, Outcome};
use sola_kit::fonts;
use sola_kit::tabs::TabWindows;
use sola_kit::theme::default_theme;

use crate::Msg;
use crate::buffer::{Group, Pos};
use crate::command::{Command, Motion};
use crate::editor::Out;
use crate::files::{self, Loaded};
use crate::instance;
use crate::keymap::{self, Feed, Keymap, Stroke, StrokeKey};
use crate::overlay::{self, Answer, Mini, MiniKind, Palette, Prompt};
use crate::search;
use crate::session::{self, Session};
use crate::syntax;
use crate::view::{Clicks, Surface};
use crate::watch;
use crate::win::{Follow, Win, mini_id, palette_id};

pub const APP_ID: &str = "sola-editor";

/// How much a frame spends on colors before the screen gets to update.
const HIGHLIGHT_BUDGET: Duration = Duration::from_millis(6);
/// How often unsaved text goes to the recovery files, while there is some.
const RECOVER_EVERY: Duration = Duration::from_secs(2);
const WINDOW_SIZE: Size = Size::new(1000.0, 800.0);

pub fn run() -> iced::Result {
    startup(APP_ID);

    let targets = targets_of_argv();
    // An opener's launch with no editor up: the session starts it, so it is
    // tracked and comes back after a restart, and this process hands it the
    // files. With no session to ask, this process becomes the editor.
    if sola_core::opener::is_open_launch()
        && !instance::live()
        && sola_kit::opener::start(APP_ID, |ws| sola_kit::opener::has_window(ws, APP_ID))
    {
        hand_off(&targets);
        return Ok(());
    }
    if !instance::claim() {
        hand_off(&targets);
        return Ok(());
    }
    syntax::warm_up();

    let mut setup = BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu_definition(MenuDefinition {
            label: "Editor".into(),
            // No key: Super is Meta here, and M-w is Copy.
            items: vec![MenuItem::Action {
                id: sola_kit::menu::ACTION_CLOSE_WINDOW.into(),
                label: "Close Window".into(),
                shortcut: None,
                disabled: false,
                checked: false,
            }],
        });
    for menu in menus() {
        setup = setup.app_menu_definition(menu);
    }
    setup.opens_own().install();

    iced::daemon(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .run()
}

fn targets_of_argv() -> Vec<PathBuf> {
    std::env::args()
        .skip(1)
        .filter(|a| !a.is_empty() && a != sola_core::opener::OPEN_FLAG)
        .map(|a| files::absolute(Path::new(&a)))
        .collect()
}

fn hand_off(targets: &[PathBuf]) {
    let raise = [String::new()];
    let targets: Vec<String> = targets.iter().map(|t| t.display().to_string()).collect();
    for target in if targets.is_empty() { &raise[..] } else { &targets[..] } {
        if let Err(e) = sola_open::to_app(APP_ID, target) {
            tracing::warn!(%target, error = %e, "handing a file to the running editor failed");
        }
    }
}

fn item(id: &str, label: &str, shortcut: Option<KeyChord>) -> MenuItem {
    MenuItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut,
        disabled: false,
        checked: false,
    }
}

/// The menus. Super is Meta here, and a Super key on a menu is the shell's to
/// grab: it arrives as the menu item and `keymap` never sees it. So no item may
/// have a Meta key that does something else. Find, Cut and Paste go without a
/// key, under ids of their own since the shared ones (`find`, `cut`, `paste`)
/// bring Super+F, Super+X and Super+V with them. Close has none: Super+W copies.
fn menus() -> Vec<MenuDefinition> {
    vec![
        MenuDefinition {
            label: "File".into(),
            items: vec![
                item("open", "Open…", Some(KeyCode::O.meta())),
                item("save", "Save", Some(KeyCode::S.meta())),
                item("save_as", "Save As…", Some(KeyCode::S.meta().shift())),
                MenuItem::Divider,
                item("close_file", "Close File", None),
            ],
        },
        MenuDefinition {
            label: "Edit".into(),
            items: vec![
                item("undo", "Undo", Some(KeyCode::Z.meta())),
                item("redo", "Redo", Some(KeyCode::Z.meta_shift())),
                MenuItem::Divider,
                item("cut_text", "Cut", None),
                item("copy", "Copy", Some(KeyCode::C.meta())),
                item("yank", "Paste", None),
                item("select_all", "Select All", Some(KeyCode::A.meta())),
                MenuItem::Divider,
                item("find_text", "Find", None),
                item("go_to_line", "Go to Line", None),
                item("toggle_comment", "Toggle Comment", Some(KeyCode::SLASH.meta())),
            ],
        },
        MenuDefinition {
            label: "View".into(),
            items: vec![item("palette", "Command Palette", Some(KeyCode::P.meta_shift()))],
        },
    ]
}

fn command_of_menu(action: &str) -> Option<Command> {
    Some(match action {
        "quit" => Command::Quit,
        "open" => Command::OpenFile,
        "save" => Command::Save,
        "save_as" => Command::SaveAs,
        "close_file" => Command::CloseFile,
        "undo" => Command::Undo,
        "redo" => Command::Redo,
        "cut_text" => Command::Cut,
        "copy" => Command::Copy,
        "yank" => Command::Yank,
        "select_all" => Command::SelectAll,
        "find_text" => Command::Find,
        "go_to_line" => Command::GotoLine,
        "toggle_comment" => Command::ToggleComment,
        "palette" => Command::Palette,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerKind {
    Open,
    SaveAs,
}

pub struct App {
    theme: Theme,
    tabs: TabWindows,
    wins: HashMap<window::Id, Win>,
    /// The windows in the order they opened, for the session.
    order: Vec<window::Id>,
    focused: Option<window::Id>,
    float: sola_kit::FloatState,
    keymap: Keymap,
    picker: Option<(PickerKind, FilePicker, window::Id)>,
    /// The modifiers as last announced (see `keymap::merged_modifiers`).
    modifiers: keyboard::Modifiers,
    /// The modifier keys seen going down and not yet up.
    held: keyboard::Modifiers,
    /// What C-s C-s searches for again.
    last_find: String,
    next_untitled: u64,
    /// A [`Msg::HighlightMore`] is on its way.
    highlight_queued: bool,
}

impl App {
    fn new() -> Self {
        Self {
            theme: default_theme(),
            tabs: TabWindows::new(APP_ID),
            wins: HashMap::new(),
            order: Vec::new(),
            focused: None,
            float: sola_kit::FloatState::new(APP_ID),
            keymap: Keymap::new(),
            picker: None,
            modifiers: keyboard::Modifiers::empty(),
            held: keyboard::Modifiers::empty(),
            last_find: String::new(),
            next_untitled: 1,
            highlight_queued: false,
        }
    }

    /// The saved session and the files named on the command line, a window
    /// each, in order and one at a time. Nothing at all: an empty one.
    pub fn boot() -> (Self, Task<Msg>) {
        let mut app = Self::new();
        let saved = session::load();
        let front = saved.front;
        let mut wins = Vec::new();
        for (i, entry) in saved.open.into_iter().enumerate() {
            if let Some(win) = app.restore(entry) {
                wins.push((win, Some(i) == front));
            }
        }
        for path in targets_of_argv() {
            match files::load(&path) {
                Ok(loaded) => {
                    // Already back from the session: it just comes to the front.
                    if let Some(found) = wins.iter_mut().find(|(w, _)| w.path.as_deref() == Some(&path)) {
                        found.1 = true;
                    } else {
                        wins.iter_mut().for_each(|(_, f)| *f = false);
                        wins.push((win_of_loaded(loaded), true));
                    }
                }
                Err(e) => tracing::warn!(error = %e, "can't open a file from the command line"),
            }
        }
        if wins.is_empty() {
            let key = app.untitled_key();
            wins.push((Win::new("", None, key, None), true));
        }
        let mut chain = Task::none();
        for (win, in_front) in wins {
            chain = chain.chain(app.add_window(win, !in_front));
        }
        app.persist();
        (app, chain)
    }

    fn restore(&mut self, entry: session::Entry) -> Option<Win> {
        let recovered = entry.recover.as_deref().and_then(session::read_recover);
        let mut win = match &entry.path {
            Some(path) => {
                // A file that has gone only comes back for its unsaved text.
                if !path.is_file() && recovered.is_none() {
                    return None;
                }
                win_of_loaded(files::load(path).ok()?)
            }
            None => {
                recovered.as_ref()?;
                let key = self.untitled_key();
                Win::new("", None, key, None)
            }
        };
        if let Some(text) = &recovered {
            win.ed.replace_all(text);
            win.rebuild_highlight();
            // An untitled window's key, and so its file's name, is new.
            win.write_recover();
            if let Some(old) = entry.recover.as_deref().filter(|old| *old != win.recover_name()) {
                session::remove_recover(old);
            }
        }
        win.ed.set_point(Pos::new(entry.line, entry.col));
        win.top = entry.top.min(win.ed.buf.line_count() - 1);
        Some(win)
    }

    fn untitled_key(&mut self) -> String {
        let key = format!("untitled-{}", self.next_untitled);
        self.next_untitled += 1;
        key
    }

    fn add_window(&mut self, win: Win, background: bool) -> Task<Msg> {
        let mut settings = sola_kit::app::window_settings_transparent(APP_ID);
        settings.size = WINDOW_SIZE;
        if let Some(path) = &win.path {
            watch::add(path);
        }
        let (id, open) = self.tabs.open(&win.key.clone(), settings, background);
        tracing::info!(key = %win.key, background, "editor window");
        self.wins.insert(id, win);
        self.order.push(id);
        if !background || self.focused.is_none() {
            self.focused = Some(id);
        }
        open.discard()
    }

    pub fn title(&self, window: window::Id) -> String {
        self.wins.get(&window).map_or_else(|| "Editor".to_string(), Win::title)
    }

    pub fn theme(&self, window: window::Id) -> Theme {
        sola_kit::theme_for(self.chrome(window), &self.theme)
    }

    fn chrome(&self, window: window::Id) -> sola_kit::Chrome {
        self.tabs
            .river_id(window)
            .map_or(sola_kit::Chrome::Tiled, |id| self.float.chrome(id))
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        let needs_recover = self.wins.values().any(Win::needs_recover);
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            sola_kit::call_subscription().map(Msg::Call),
            watch::changes().map(Msg::DiskChanged),
            if self.picker.is_some() {
                FilePicker::keys().map(Msg::Picker)
            } else {
                Subscription::none()
            },
            if needs_recover {
                iced::time::every(RECOVER_EVERY).map(|_| Msg::RecoverTick)
            } else {
                Subscription::none()
            },
            event::listen_with(|event, status, id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed { modified_key, modifiers, text, .. }) => {
                    Some(Msg::Key {
                        window: id,
                        key: modified_key,
                        modifiers,
                        text: text.map(|t| t.to_string()),
                        status,
                    })
                }
                Event::Keyboard(keyboard::Event::KeyReleased { modified_key, .. }) => {
                    keymap::modifier_bit(&modified_key).map(Msg::ModifierUp)
                }
                Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                    Some(Msg::Modifiers(modifiers))
                }
                Event::Window(
                    e @ (window::Event::Focused
                    | window::Event::Unfocused
                    | window::Event::CloseRequested
                    | window::Event::Closed),
                ) => Some(Msg::Window(id, e)),
                _ => None,
            }),
        ])
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        let task = self.dispatch(msg);
        let more = self.sync_highlights();
        Task::batch([task, more])
    }

    /// Bring each window's colors up to what it shows, a frame's worth at
    /// a time.
    fn sync_highlights(&mut self) -> Task<Msg> {
        let mut behind = false;
        for win in self.wins.values_mut() {
            if let Some(from) = win.ed.buf.take_changed() {
                win.hl.invalidate(from);
            }
            let until = win.top + win.rows() + 2;
            if !win.hl.advance(win.ed.buf.lines(), until, HIGHLIGHT_BUDGET) {
                behind = true;
            }
        }
        if behind && !self.highlight_queued {
            self.highlight_queued = true;
            return Task::done(Msg::HighlightMore);
        }
        Task::none()
    }

    fn dispatch(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Call(inc) => match sola_kit::opener::open_target(&inc) {
                Some(target) => {
                    inc.reply.ok(serde_json::json!({ "opened": true }));
                    self.open_target(&target)
                }
                None => {
                    inc.reply.err(format!("unknown method {}", inc.method));
                    Task::none()
                }
            },
            Msg::Bus(message) => {
                self.float.update(&message);
                self.tabs.update(&message);
                // The tab bar's stoplight: as the window's own close.
                if let Some(window) = self.tabs.close_request(&message) {
                    return self.request_close(window);
                }
                apply_theme_update(&message, &mut self.theme);
                if is_self_quit(&message, APP_ID) {
                    return self.quit();
                }
                match Topic::parse(&message) {
                    Some(Topic::MenuAction(p)) if p.app_id == APP_ID => {
                        let window = self.focused;
                        match (window, command_of_menu(&p.action_id)) {
                            (Some(window), Some(command)) => self.run(window, command),
                            _ => Task::none(),
                        }
                    }
                    _ => Task::none(),
                }
            }
            Msg::At(window, inner) => {
                self.focused = Some(window);
                self.dispatch_at(window, *inner)
            }
            Msg::Window(window, event) => self.on_window(window, event),
            Msg::Key { window, key, modifiers, text, status } => {
                self.on_key(window, &key, modifiers, text.as_deref(), status)
            }
            Msg::Modifiers(modifiers) => {
                self.modifiers = modifiers;
                Task::none()
            }
            Msg::ModifierUp(bit) => {
                self.held = self.held & !bit;
                Task::none()
            }
            Msg::Loaded(result) => self.on_loaded(result),
            Msg::Picker(m) => self.on_picker(m),
            Msg::Pasted(window, text) => {
                if let (Some(win), Some(text)) = (self.wins.get_mut(&window), text) {
                    let out = win.ed.insert(&text, Group::None);
                    note_of(win, out);
                    win.follow(Follow::Reveal);
                }
                Task::none()
            }
            Msg::DiskChanged(path) => {
                self.on_disk_changed(&path);
                Task::none()
            }
            Msg::RecoverTick => {
                self.wins.values_mut().for_each(Win::write_recover);
                self.persist();
                Task::none()
            }
            Msg::HighlightMore => {
                self.highlight_queued = false;
                Task::none()
            }
            // Only from a view, inside `At`.
            _ => Task::none(),
        }
    }

    fn dispatch_at(&mut self, window: window::Id, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Press { at, clicks, shift } => {
                self.keymap.clear();
                if let Some(win) = self.wins.get_mut(&window) {
                    win.mini = None;
                    win.note = None;
                    match (clicks, shift) {
                        (_, true) => win.ed.extend_to(at),
                        (Clicks::One, false) => win.ed.click(at),
                        (Clicks::Two, false) => win.ed.select_word(at),
                        (Clicks::Three, false) => win.ed.select_line(at),
                    }
                    win.follow(Follow::Reveal);
                }
                Task::none()
            }
            Msg::DragTo(at) => {
                if let Some(win) = self.wins.get_mut(&window) {
                    win.ed.extend_to(at);
                    win.follow(Follow::Reveal);
                }
                Task::none()
            }
            Msg::Wheel { lines, columns } => {
                if let Some(win) = self.wins.get_mut(&window) {
                    win.scroll(lines, columns);
                }
                Task::none()
            }
            Msg::ScrollTo(fraction) => {
                if let Some(win) = self.wins.get_mut(&window) {
                    let last = win.ed.buf.line_count().saturating_sub(1) as f32;
                    win.top = (fraction.clamp(0.0, 1.0) * last) as usize;
                }
                Task::none()
            }
            Msg::MiniInput(text) => {
                self.mini_input(window, text);
                Task::none()
            }
            Msg::MiniSubmit => {
                self.mini_submit(window);
                Task::none()
            }
            Msg::PaletteInput(query) => {
                if let Some(palette) = self.wins.get_mut(&window).and_then(|w| w.palette.as_mut()) {
                    palette.query = query;
                    palette.selected = 0;
                }
                Task::none()
            }
            Msg::PaletteSubmit => {
                let command = self
                    .wins
                    .get_mut(&window)
                    .and_then(|w| w.palette.take())
                    .and_then(|p| p.current());
                command.map_or_else(Task::none, |c| self.run(window, c))
            }
            Msg::PaletteRun(command) => {
                if let Some(win) = self.wins.get_mut(&window) {
                    win.palette = None;
                }
                self.run(window, command)
            }
            Msg::PaletteClose => {
                if let Some(win) = self.wins.get_mut(&window) {
                    win.palette = None;
                }
                Task::none()
            }
            Msg::Answer(answer) => self.on_answer(window, answer),
            Msg::Picker(m) => self.on_picker(m),
            Msg::TitleDrag => sola_kit::drag(Some(window)),
            Msg::TitleResize(direction) => sola_kit::drag_resize(Some(window), direction),
            Msg::TitleClose => self.request_close(window),
            other => self.dispatch(other),
        }
    }

    fn on_window(&mut self, window: window::Id, event: window::Event) -> Task<Msg> {
        match event {
            window::Event::Focused => {
                self.focused = Some(window);
                self.keymap.clear();
                if let Some(win) = self.wins.get_mut(&window) {
                    win.focused = true;
                }
                self.persist();
                Task::none()
            }
            window::Event::Unfocused => {
                if let Some(win) = self.wins.get_mut(&window) {
                    win.focused = false;
                }
                // The Super of a chord that took focus away never comes back up.
                self.modifiers = keyboard::Modifiers::empty();
                self.held = keyboard::Modifiers::empty();
                Task::none()
            }
            window::Event::CloseRequested => self.request_close(window),
            window::Event::Closed => {
                // Closed under us: anything unsaved stays in its recovery file.
                self.forget(window);
                self.persist();
                if self.wins.is_empty() { iced::exit() } else { Task::none() }
            }
            _ => Task::none(),
        }
    }

    fn on_key(
        &mut self,
        window: window::Id,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        typed: Option<&str>,
        status: event::Status,
    ) -> Task<Msg> {
        if self.picker.is_some() {
            return Task::none();
        }
        if let Some(bit) = keymap::modifier_bit(key) {
            self.held = self.held | bit;
            return Task::none();
        }
        let mods = keymap::merged_modifiers(modifiers, self.modifiers, self.held);
        let Some(stroke) = Stroke::from_iced(key, mods) else {
            return Task::none();
        };
        let Some(win) = self.wins.get_mut(&window) else {
            return Task::none();
        };

        if let Some(prompt) = win.prompt {
            if is_cancel(&stroke) {
                return self.on_answer(window, Answer::Cancel);
            }
            if stroke.key == StrokeKey::Named(keyboard::key::Named::Enter) {
                return self.on_answer(window, prompt.primary());
            }
            return Task::none();
        }
        if let Some(palette) = win.palette.as_mut() {
            match bound_motion(&stroke) {
                _ if is_cancel(&stroke) => win.palette = None,
                Some(Motion::Down) => palette.step(1),
                Some(Motion::Up) => palette.step(-1),
                _ => {}
            }
            return Task::none();
        }
        if win.mini.is_some() {
            return self.on_mini_key(window, &stroke);
        }
        // A field or a button took it.
        if status == event::Status::Captured {
            return Task::none();
        }

        match self.keymap.feed(stroke) {
            Feed::Run(command) => self.run(window, command),
            Feed::Pending(keys) => {
                if let Some(win) = self.wins.get_mut(&window) {
                    win.note = Some(format!("{keys}-"));
                }
                Task::none()
            }
            Feed::Undefined(keys) => {
                if let Some(win) = self.wins.get_mut(&window) {
                    win.note = Some(format!("{keys} is undefined"));
                }
                Task::none()
            }
            Feed::Plain => {
                let Some(text) = typed.filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
                else {
                    return Task::none();
                };
                if let Some(win) = self.wins.get_mut(&window) {
                    win.note = None;
                    let out = win.ed.insert(text, Group::Typing);
                    note_of(win, out);
                    win.follow(Follow::Reveal);
                }
                Task::none()
            }
        }
    }

    /// A key while the line at the foot has the field: typing is the field's;
    /// these are ours.
    fn on_mini_key(&mut self, window: window::Id, stroke: &Stroke) -> Task<Msg> {
        if is_cancel(stroke) {
            self.close_mini(window);
            return Task::none();
        }
        let forward = is_key(stroke, 's');
        if (forward || is_key(stroke, 'r'))
            && self.wins.get(&window).and_then(|w| w.mini.as_ref()).is_some_and(|m| m.kind == MiniKind::Find)
        {
            self.find_step(window, forward);
        }
        Task::none()
    }

    /// Run `command` in `window`.
    fn run(&mut self, window: window::Id, command: Command) -> Task<Msg> {
        if let Some(win) = self.wins.get_mut(&window) {
            win.note = None;
        }
        match command {
            Command::Find => return self.open_mini(window, MiniKind::Find),
            Command::GotoLine => return self.open_mini(window, MiniKind::GotoLine),
            Command::Palette => {
                if let Some(win) = self.wins.get_mut(&window) {
                    win.palette = Some(Palette::default());
                }
                return iced::widget::operation::focus(palette_id());
            }
            Command::OpenFile => {
                self.open_picker(window);
                return Task::none();
            }
            Command::Save => return self.save(window),
            Command::SaveAs => {
                self.save_picker(window);
                return Task::none();
            }
            Command::CloseFile => return self.request_close(window),
            Command::Quit => return self.quit(),
            Command::Yank => {
                return iced::clipboard::read().map(move |text| Msg::Pasted(window, text));
            }
            _ => {}
        }
        let Some(win) = self.wins.get_mut(&window) else {
            return Task::none();
        };
        let out = win.ed.apply(command, win.page());
        win.follow(follow_of(command));
        match out {
            Out::None => Task::none(),
            Out::Note(note) => {
                win.note = Some(note);
                Task::none()
            }
            Out::Clipboard(text) => iced::clipboard::write(text),
        }
    }

    fn open_mini(&mut self, window: window::Id, kind: MiniKind) -> Task<Msg> {
        let Some(win) = self.wins.get_mut(&window) else {
            return Task::none();
        };
        win.mini = Some(Mini::new(kind, win.ed.point));
        iced::widget::operation::focus(mini_id())
    }

    fn close_mini(&mut self, window: window::Id) {
        if let Some(win) = self.wins.get_mut(&window) {
            let was_find = win.mini.take().is_some_and(|m| m.kind == MiniKind::Find);
            // The match stays where the point is, without the region.
            if was_find {
                let point = win.ed.point;
                win.ed.set_point(point);
            }
        }
    }

    fn mini_input(&mut self, window: window::Id, text: String) {
        let Some(win) = self.wins.get_mut(&window) else {
            return;
        };
        let Some(mini) = win.mini.as_mut() else {
            return;
        };
        mini.text = text;
        if mini.kind != MiniKind::Find {
            return;
        }
        // As you type, from where the search started.
        let query = mini.text.clone();
        let found = search::next(&win.ed.buf, mini.origin, &query);
        mini.failed = found.is_none() && !query.is_empty();
        match found {
            Some((a, b)) => win.ed.select(a, b),
            None => {
                let origin = mini.origin;
                win.ed.set_point(origin);
            }
        }
        win.follow(Follow::Reveal);
        if !query.is_empty() {
            self.last_find = query;
        }
    }

    /// C-s and C-r in the field: on to the next match. In an empty field,
    /// the last search again.
    fn find_step(&mut self, window: window::Id, forward: bool) {
        let last = self.last_find.clone();
        let Some(win) = self.wins.get_mut(&window) else {
            return;
        };
        let Some(mini) = win.mini.as_mut() else {
            return;
        };
        if mini.text.is_empty() {
            mini.text = last;
        }
        let query = mini.text.clone();
        let (start, end) = win.ed.region().unwrap_or((win.ed.point, win.ed.point));
        let found = if forward {
            search::next(&win.ed.buf, end, &query)
        } else {
            search::prev(&win.ed.buf, start, &query)
        };
        mini.failed = found.is_none() && !query.is_empty();
        if let Some((a, b)) = found {
            win.ed.select(a, b);
            win.follow(Follow::Reveal);
        }
    }

    fn mini_submit(&mut self, window: window::Id) {
        let Some(mini) = self.wins.get(&window).and_then(|w| w.mini.clone()) else {
            return;
        };
        if mini.kind == MiniKind::GotoLine
            && let Some(at) = parse_goto(&mini.text)
            && let Some(win) = self.wins.get_mut(&window)
        {
            win.ed.set_point(at);
            win.follow(Follow::Center);
        }
        self.close_mini(window);
    }

    /// `open` from Sola's opener or `edit`: a file, or nothing at all, which
    /// brings the editor up.
    fn open_target(&mut self, target: &str) -> Task<Msg> {
        if target.is_empty() {
            if let Some(window) = self.focused {
                self.tabs.show(window);
            }
            return Task::none();
        }
        // `file://` from the opener.
        let path = sola_open::Target::parse(target, Path::new("/"))
            .map_or_else(|| target.to_string(), |t| t.as_arg());
        self.open_path(files::absolute(Path::new(&path)))
    }

    fn open_path(&mut self, path: PathBuf) -> Task<Msg> {
        let open = self.wins.iter().find(|(_, w)| w.path.as_deref() == Some(&path));
        if let Some((window, _)) = open {
            self.tabs.show(*window);
            return Task::none();
        }
        Task::perform(async move { files::load(&path) }, Msg::Loaded)
    }

    fn on_loaded(&mut self, result: Result<Loaded, String>) -> Task<Msg> {
        match result {
            Ok(loaded) => {
                // Opened twice before the first load came back.
                let open = self.wins.iter().find(|(_, w)| w.path.as_deref() == Some(&loaded.path));
                if let Some((window, _)) = open {
                    self.tabs.show(*window);
                    return Task::none();
                }
                // An empty window gives way to the file.
                let empty: Vec<window::Id> =
                    self.wins.iter().filter(|(_, w)| w.is_pristine()).map(|(id, _)| *id).collect();
                let mut tasks: Vec<Task<Msg>> = empty
                    .into_iter()
                    .map(|id| {
                        self.forget(id);
                        window::close(id)
                    })
                    .collect();
                tasks.push(self.add_window(win_of_loaded(loaded), false));
                self.persist();
                Task::batch(tasks)
            }
            Err(e) => {
                tracing::warn!(error = %e, "can't open a file");
                if let Some(win) = self.focused.and_then(|f| self.wins.get_mut(&f)) {
                    win.note = Some(e);
                }
                Task::none()
            }
        }
    }

    fn start_dir(&self, window: window::Id) -> PathBuf {
        self.wins
            .get(&window)
            .and_then(|w| w.path.as_ref())
            .and_then(|p| p.parent())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    fn open_picker(&mut self, window: window::Id) {
        let picker = FilePicker::open().multiple().title("Open").start_dir(self.start_dir(window));
        self.picker = Some((PickerKind::Open, picker, window));
    }

    fn save_picker(&mut self, window: window::Id) {
        let name = self.wins.get(&window).map_or_else(String::new, Win::name);
        let picker = FilePicker::save()
            .title("Save As")
            .start_dir(self.start_dir(window))
            .suggested_name(name);
        self.picker = Some((PickerKind::SaveAs, picker, window));
    }

    fn on_picker(&mut self, msg: PickerMsg) -> Task<Msg> {
        let Some((kind, picker, window)) = self.picker.as_mut() else {
            return Task::none();
        };
        let (kind, window) = (*kind, *window);
        let outcome = picker.update(msg);
        let follow = picker.scroll_task().map(Msg::Picker);
        match outcome {
            Some(Outcome::Cancelled) => self.picker = None,
            Some(Outcome::PickedMany(paths)) => {
                self.picker = None;
                let opens: Vec<Task<Msg>> = paths.into_iter().map(|p| self.open_path(p)).collect();
                return Task::batch(opens);
            }
            Some(Outcome::Picked(path)) => {
                self.picker = None;
                return match kind {
                    PickerKind::Open => self.open_path(path),
                    PickerKind::SaveAs => {
                        self.write_to(window, &path);
                        Task::none()
                    }
                };
            }
            _ => return follow,
        }
        Task::none()
    }

    /// C-x C-s. Not over something that changed since we loaded it without
    /// asking.
    fn save(&mut self, window: window::Id) -> Task<Msg> {
        let Some(win) = self.wins.get_mut(&window) else {
            return Task::none();
        };
        let Some(path) = win.path.clone() else {
            self.save_picker(window);
            return Task::none();
        };
        if win.ed.read_only {
            win.note = Some("This file is read-only".into());
            return Task::none();
        }
        if !win.ed.buf.is_dirty() && !win.changed_on_disk && win.stamp.is_some() {
            win.note = Some("No changes need to be saved".into());
            return Task::none();
        }
        if win.stamp.is_some() && files::stamp(&path) != win.stamp {
            win.prompt = Some(Prompt::SaveOverChange);
            return Task::none();
        }
        self.write_to(window, &path);
        Task::none()
    }

    fn write_to(&mut self, window: window::Id, path: &Path) {
        let Some(win) = self.wins.get_mut(&window) else {
            return;
        };
        match files::save(path, &win.ed.buf.text()) {
            Ok(stamp) => {
                win.mark_saved(path, stamp);
                win.note = Some(format!("Wrote {}", path.display()));
                watch::add(path);
                self.persist();
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "save failed");
                win.note = Some(format!("Can't save {}: {e}", path.display()));
            }
        }
    }

    fn on_answer(&mut self, window: window::Id, answer: Answer) -> Task<Msg> {
        let Some(win) = self.wins.get_mut(&window) else {
            return Task::none();
        };
        let prompt = win.prompt.take();
        match (prompt, answer) {
            (Some(Prompt::CloseUnsaved), Answer::Save) => {
                let task = self.save(window);
                // It saved, or it is asking: only saved goes on to close.
                let saved = self.wins.get(&window).is_some_and(|w| !w.ed.buf.is_dirty());
                if saved { self.close(window) } else { task }
            }
            (Some(Prompt::CloseUnsaved), Answer::Discard) => self.close(window),
            (Some(Prompt::SaveOverChange), Answer::Overwrite) => {
                if let Some(path) = self.wins.get(&window).and_then(|w| w.path.clone()) {
                    self.write_to(window, &path);
                }
                Task::none()
            }
            (Some(Prompt::SaveOverChange), Answer::Reload) => {
                self.reload(window);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn reload(&mut self, window: window::Id) {
        let Some(win) = self.wins.get_mut(&window) else {
            return;
        };
        let Some(path) = win.path.clone() else {
            return;
        };
        match files::load(&path) {
            Ok(loaded) => {
                session::remove_recover(&win.recover_name());
                win.reload(loaded);
                win.note = Some(format!("Reloaded {}", win.name()));
            }
            Err(e) => win.note = Some(e),
        }
    }

    /// Something changed in a folder we watch. A file with nothing unsaved
    /// follows the disk; one with unsaved edits says so, and asks on save.
    fn on_disk_changed(&mut self, path: &Path) {
        let found = self.wins.iter().find(|(_, w)| w.path.as_deref() == Some(path));
        let Some((window, win)) = found else {
            return;
        };
        let window = *window;
        let now = files::stamp(path);
        if now == win.stamp {
            return;
        }
        if now.is_none() {
            // Gone, or mid-replace: the write that follows comes through here too.
            return;
        }
        if win.ed.buf.is_dirty() {
            let Some(win) = self.wins.get_mut(&window) else {
                return;
            };
            win.changed_on_disk = true;
            win.note = Some(format!("{} changed on disk", win.name()));
            return;
        }
        self.reload(window);
    }

    fn request_close(&mut self, window: window::Id) -> Task<Msg> {
        let Some(win) = self.wins.get_mut(&window) else {
            return Task::none();
        };
        if win.ed.buf.is_dirty() {
            win.prompt = Some(Prompt::CloseUnsaved);
            self.tabs.show(window);
            return Task::none();
        }
        self.close(window)
    }

    fn close(&mut self, window: window::Id) -> Task<Msg> {
        if let Some(win) = self.wins.get(&window) {
            session::remove_recover(&win.recover_name());
        }
        self.forget(window);
        self.persist();
        if self.wins.is_empty() {
            tracing::info!("last editor window closed; quitting");
            return Task::batch([window::close(window), iced::exit()]);
        }
        window::close(window)
    }

    fn forget(&mut self, window: window::Id) {
        self.wins.remove(&window);
        self.order.retain(|id| *id != window);
        self.tabs.close(window);
        if self.focused == Some(window) {
            self.focused = self.order.last().copied();
        }
    }

    /// Quitting keeps your place: unsaved text waits in its recovery file and
    /// comes back with the session.
    fn quit(&mut self) -> Task<Msg> {
        self.wins.values_mut().for_each(Win::write_recover);
        self.persist();
        iced::exit()
    }

    fn persist(&self) {
        let open: Vec<session::Entry> =
            self.order.iter().filter_map(|id| self.wins.get(id)).map(Win::entry).collect();
        let front = self.focused.and_then(|f| self.order.iter().position(|id| *id == f));
        session::save(&Session { open, front });
    }

    pub fn view(&self, window: window::Id) -> Element<'_, Msg> {
        let Some(win) = self.wins.get(&window) else {
            return Space::new().into();
        };
        let surface = canvas(Surface { win }).width(Length::Fill).height(Length::Fill);
        let mut content = column![surface].width(Length::Fill).height(Length::Fill);
        if let Some(mini) = &win.mini {
            content = content.push(mini.view());
        }
        content = content.push(status_line(win));

        let framed = sola_kit::window_frame(
            self.chrome(window),
            win.title(),
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            content.into(),
        );

        let mut layers: Vec<Element<'_, Msg>> = vec![framed];
        if let Some(palette) = &win.palette {
            layers.push(palette.view());
        }
        if let Some(prompt) = win.prompt {
            layers.push(prompt.view(win_name(win)));
        }
        if let Some((_, picker, _)) = self.picker.as_ref().filter(|(_, _, w)| *w == window) {
            layers.push(picker.overlay().map(Msg::Picker));
        }
        let view: Element<'_, Msg> = stack(layers).into();
        view.map(move |msg| Msg::At(window, Box::new(msg)))
    }
}

fn win_name(win: &Win) -> &str {
    win.path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("untitled")
}

/// The line at the very foot: what just happened, and where you are.
fn status_line(win: &Win) -> Element<'_, Msg> {
    let left = win.note.clone().unwrap_or_else(|| {
        let path = win.path.as_ref().map_or_else(|| "untitled".to_string(), |p| p.display().to_string());
        if win.changed_on_disk { format!("{path} (changed on disk)") } else { path }
    });
    let ed = &win.ed;
    let mut right = format!("{}:{}", ed.point.line + 1, ed.point_column() + 1);
    if ed.mark_mode {
        right.push_str(" · Mark");
    }
    if ed.read_only {
        right.push_str(" · Read-only");
    }
    if let Some(name) = win.hl.name() {
        right.push_str(" · ");
        right.push_str(name);
    }
    container(
        row![
            text(left).font(fonts::ui()).size(12).style(overlay::muted),
            Space::new().width(Length::Fill),
            text(right).font(fonts::ui()).size(12).style(overlay::muted),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([3.0, 12.0]))
    .width(Length::Fill)
    .style(overlay::foot)
    .into()
}

fn win_of_loaded(loaded: Loaded) -> Win {
    let key = loaded.path.to_string_lossy().into_owned();
    let mut win = Win::new(&loaded.text, Some(loaded.path), key, loaded.stamp);
    win.ed.read_only = loaded.read_only;
    win
}

fn note_of(win: &mut Win, out: Out) {
    if let Out::Note(note) = out {
        win.note = Some(note);
    }
}

/// A page moves the point and keeps it mid-window, which is what `C-v` and
/// `M-v` did in Zed (`pagedown ctrl-l`); `C-l` is that alone.
fn follow_of(command: Command) -> Follow {
    match command {
        Command::Center | Command::Move(Motion::PageDown | Motion::PageUp) => Follow::Center,
        _ => Follow::Reveal,
    }
}

fn is_key(stroke: &Stroke, letter: char) -> bool {
    stroke.ctrl && !stroke.meta && stroke.key == StrokeKey::Char(letter)
}

/// C-g or Escape.
fn is_cancel(stroke: &Stroke) -> bool {
    is_key(stroke, 'g') || stroke.key == StrokeKey::Named(keyboard::key::Named::Escape)
}

/// C-n / C-p / arrows, in a list.
fn bound_motion(stroke: &Stroke) -> Option<Motion> {
    use keyboard::key::Named;
    if is_key(stroke, 'n') || stroke.key == StrokeKey::Named(Named::ArrowDown) {
        Some(Motion::Down)
    } else if is_key(stroke, 'p') || stroke.key == StrokeKey::Named(Named::ArrowUp) {
        Some(Motion::Up)
    } else {
        None
    }
}

/// `12` or `12:5`, counted from 1, as a position.
fn parse_goto(text: &str) -> Option<Pos> {
    let mut parts = text.trim().splitn(2, ':');
    let line: usize = parts.next()?.trim().parse().ok()?;
    let col: usize = parts.next().and_then(|c| c.trim().parse().ok()).unwrap_or(1);
    Some(Pos::new(line.saturating_sub(1), col.saturating_sub(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goto_counts_from_one() {
        assert_eq!(parse_goto("12"), Some(Pos::new(11, 0)));
        assert_eq!(parse_goto(" 12:5 "), Some(Pos::new(11, 4)));
        assert_eq!(parse_goto("0"), Some(Pos::new(0, 0)));
        assert_eq!(parse_goto("abc"), None);
    }

    #[test]
    fn every_menu_item_runs_a_command() {
        assert!(command_of_menu("quit").is_some());
        for menu in menus() {
            for item in &menu.items {
                if let MenuItem::Action { id, .. } = item {
                    assert!(command_of_menu(id).is_some(), "{id}");
                }
            }
        }
    }

    /// A menu's Super key goes to the menu item, so it had better be the same
    /// command `keymap` has on that key. A shared id gets its shared key
    /// whatever we declare, so that one is what counts.
    #[test]
    fn no_menu_takes_a_meta_key_for_something_else() {
        let mut map = Keymap::new();
        for menu in menus() {
            for item in &menu.items {
                let MenuItem::Action { id, shortcut, .. } = item else {
                    continue;
                };
                let common = sola_kit::keys::COMMON_ACTIONS
                    .iter()
                    .find(|(action, ..)| action == id)
                    .map(|(.., key)| *key);
                let Some(key) = common.or(*shortcut).filter(|k| k.meta) else {
                    continue;
                };
                let letter = key.keycode.keysym().and_then(char::from_u32).expect("a key");
                let stroke = Stroke {
                    ctrl: false,
                    meta: true,
                    shift: key.shift,
                    key: StrokeKey::Char(letter.to_ascii_lowercase()),
                };
                if let Feed::Run(command) = map.feed(stroke) {
                    assert_eq!(Some(command), command_of_menu(id), "{id} has {letter}");
                }
                map.clear();
            }
        }
    }

    #[test]
    fn super_w_and_the_other_meta_keys_are_on_no_menu() {
        let keys: Vec<KeyChord> = menus()
            .iter()
            .flat_map(|menu| menu.items.iter())
            .filter_map(|item| match item {
                MenuItem::Action { id, shortcut, .. } => sola_kit::keys::COMMON_ACTIONS
                    .iter()
                    .find(|(action, ..)| action == id)
                    .map(|(.., key)| *key)
                    .or(*shortcut),
                _ => None,
            })
            .collect();
        for code in [KeyCode::W, KeyCode::V, KeyCode::F, KeyCode::B, KeyCode::D, KeyCode::X, KeyCode::G, KeyCode::M] {
            assert!(!keys.contains(&code.meta()), "{code:?}");
        }
    }
}
