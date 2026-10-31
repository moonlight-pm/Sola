//! File picker — the kit's Open / Save / Choose Folder panel, one for every
//! app (as macOS has one).
//!
//! **THESIS:** the path is a trail you walk, not a string you type.
//! Clickable breadcrumb chips are the signature; the name field is only
//! for the leaf.
//! **OWN-WORLD:** graphite modal (`card::modal`), etch places rail,
//! quiet file stack. Accent only on the single confirm action.
//! **STORY:** pick a place, walk crumbs, select a row (or several), Open,
//! Save or Choose.
//!
//! Stateful like [`ColorPicker`](crate::components::ColorPicker): the
//! caller holds a [`FilePicker`], routes [`Message`] through
//! [`FilePicker::update`], acts on [`Outcome`], and while it is open folds
//! [`FilePicker::keys`] into its subscription (arrows, Return, Escape,
//! ⌘↑ / ⌘↓, ⌘⇧. for hidden files, typing a name to jump to it) and runs
//! [`FilePicker::scroll_task`] after each update so the keyboard's row
//! stays in view.
//!
//! ```ignore
//! self.picker = Some(FilePicker::open().multiple().title("Attach files"));
//! match picker.update(m) {
//!     Some(Outcome::PickedMany(paths)) => { /* … */ }
//!     Some(Outcome::Cancelled) => self.picker = None,
//!     _ => {}
//! }
//! let follow = picker.scroll_task().map(Msg::Picker);
//! picker.overlay() // dim + centered modal
//! ```
//!
//! Save asks before it replaces a file; Save and Choose Folder offer New
//! Folder.

mod fs;
mod view;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::keyboard::{self, Key, Modifiers, key::Named};
use iced::widget::scrollable::AbsoluteOffset;
use iced::{Event, Subscription, Task, event};

use fs::breadcrumbs;
pub use fs::Filter;
use fs::{Entry, Place, Show};

/// What the panel is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Open an existing file (or several, with [`FilePicker::multiple`]).
    Open,
    /// Name a file to write.
    Save,
    /// Choose a folder.
    Folder,
}

/// Messages the picker emits. The parent forwards them into [`FilePicker::update`].
#[derive(Debug, Clone)]
pub enum Message {
    /// A row was clicked (Shift / ⌘ extend the selection when several may
    /// be picked).
    Select(PathBuf),
    /// A row was double-clicked.
    Activate(PathBuf),
    Crumb(PathBuf),
    Place(usize),
    NameChanged(String),
    Confirm,
    Cancel,
    Parent,
    /// A key no field took ([`FilePicker::keys`]).
    Key(Key, Modifiers),
    Modifiers(Modifiers),
    ToggleHidden,
    NewFolder,
    FolderNameChanged(String),
    CreateFolder,
    CancelFolder,
    /// Save over the file that is there.
    Replace,
    /// Keep the file that is there; back to naming.
    KeepFile,
    /// The list scrolled (its offset, for keeping the keyboard's row in view).
    Scrolled(f32),
}

/// Terminal result of [`FilePicker::update`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Picked(PathBuf),
    /// [`FilePicker::multiple`]: every file chosen (one or more).
    PickedMany(Vec<PathBuf>),
    Cancelled,
}

/// Type-ahead: keys this close together spell one name.
const TYPEAHEAD: Duration = Duration::from_millis(900);

/// In-app file picker. Reloads the directory listing on each navigation.
pub struct FilePicker {
    mode: Mode,
    title: String,
    action: Option<String>,
    multiple: bool,
    cwd: PathBuf,
    home: Option<PathBuf>,
    entries: Vec<Entry>,
    /// Chosen rows, in the order they were chosen.
    selected: Vec<PathBuf>,
    /// The row the keyboard is on (and Shift extends from).
    cursor: Option<usize>,
    anchor: Option<usize>,
    name: String,
    filter: Option<Filter>,
    hidden: bool,
    error: Option<String>,
    places: Vec<Place>,
    mods: Modifiers,
    /// New Folder: the name being typed.
    naming_folder: Option<String>,
    /// Save: the file that would be replaced, waiting for an answer.
    replacing: Option<PathBuf>,
    typed: String,
    typed_at: Option<Instant>,
    scroll_y: f32,
    follow: bool,
}

impl FilePicker {
    pub fn open() -> Self {
        Self::new(Mode::Open, "Open")
    }

    pub fn save() -> Self {
        Self::new(Mode::Save, "Save")
    }

    /// Choose a folder (the listing shows folders only).
    pub fn folder() -> Self {
        Self::new(Mode::Folder, "Choose a Folder")
    }

    fn new(mode: Mode, title: &str) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let cwd = home
            .as_ref()
            .filter(|p| p.is_dir())
            .cloned()
            .unwrap_or_else(|| PathBuf::from("/"));
        let places = fs::default_places(home.as_deref());
        let mut picker = Self {
            mode,
            title: title.into(),
            action: None,
            multiple: false,
            cwd,
            home,
            entries: Vec::new(),
            selected: Vec::new(),
            cursor: None,
            anchor: None,
            name: String::new(),
            filter: None,
            hidden: false,
            error: None,
            places,
            mods: Modifiers::default(),
            naming_folder: None,
            replacing: None,
            typed: String::new(),
            typed_at: None,
            scroll_y: 0.0,
            follow: false,
        };
        picker.reload();
        picker
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// The confirm button's word (`Attach`, `Upload`); Open / Save / Choose
    /// otherwise.
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }

    /// Open: several files may be chosen ([`Outcome::PickedMany`]).
    pub fn multiple(mut self) -> Self {
        self.multiple = self.mode == Mode::Open;
        self
    }

    pub fn filter(mut self, label: impl Into<String>, extensions: &[&str]) -> Self {
        self.filter = Some(Filter {
            label: label.into(),
            extensions: extensions.iter().map(|e| e.to_string()).collect(),
        });
        self.reload();
        self
    }

    pub fn start_dir(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or(path)
        };
        if dir.is_dir() {
            self.cd(dir);
        }
        self
    }

    pub fn suggested_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The panel's keys: fold into the app's subscription while it is open.
    /// Only keys no field took arrive (typing a name stays in the field).
    pub fn keys() -> Subscription<Message> {
        event::listen_with(|event, status, _window| match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => Some(Message::Modifiers(m)),
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if status == event::Status::Ignored =>
            {
                Some(Message::Key(key, modifiers))
            }
            _ => None,
        })
    }

    /// Scroll the list to the keyboard's row when it moved out of view.
    pub fn scroll_task(&mut self) -> Task<Message> {
        if !std::mem::take(&mut self.follow) {
            return Task::none();
        }
        let Some(row) = self.cursor else {
            return Task::none();
        };
        match view::follow_offset(row, self.scroll_y) {
            Some(y) => iced::widget::operation::scroll_to(
                view::list_id(),
                AbsoluteOffset { x: 0.0, y },
            ),
            None => Task::none(),
        }
    }

    pub fn update(&mut self, msg: Message) -> Option<Outcome> {
        match msg {
            Message::Select(path) => {
                if let Some(i) = self.index_of(&path) {
                    let extend = self.multiple && self.mods.shift();
                    let toggle = self.multiple && (self.mods.command() || self.mods.logo());
                    self.select_row(i, extend, toggle);
                }
                None
            }
            Message::Activate(path) => {
                if path.is_dir() && self.mode != Mode::Folder {
                    self.cd(path);
                    None
                } else if path.is_dir() {
                    Some(Outcome::Picked(path))
                } else {
                    self.picked(vec![path])
                }
            }
            Message::Crumb(path) => {
                self.cd(path);
                None
            }
            Message::Place(i) => {
                if let Some(place) = self.places.get(i) {
                    self.cd(place.path.clone());
                }
                None
            }
            Message::NameChanged(s) => {
                self.name = s;
                self.error = None;
                self.replacing = None;
                None
            }
            Message::Confirm => self.confirm(),
            Message::Cancel => Some(Outcome::Cancelled),
            Message::Parent => {
                if let Some(parent) = self.cwd.parent() {
                    let from = self.cwd.clone();
                    self.cd(parent.to_path_buf());
                    // Land on the folder we came out of.
                    if let Some(i) = self.index_of(&from) {
                        self.select_row(i, false, false);
                    }
                }
                None
            }
            Message::Key(key, mods) => self.key(key, mods),
            Message::Modifiers(mods) => {
                self.mods = mods;
                None
            }
            Message::ToggleHidden => {
                self.hidden = !self.hidden;
                self.reload();
                None
            }
            Message::NewFolder => {
                self.naming_folder = Some(fs::free_folder_name(&self.cwd));
                None
            }
            Message::FolderNameChanged(s) => {
                self.naming_folder = Some(s);
                None
            }
            Message::CreateFolder => {
                self.create_folder();
                None
            }
            Message::CancelFolder => {
                self.naming_folder = None;
                None
            }
            Message::Replace => self.replacing.take().map(Outcome::Picked),
            Message::KeepFile => {
                self.replacing = None;
                None
            }
            Message::Scrolled(y) => {
                self.scroll_y = y;
                None
            }
        }
    }

    // ── Selection ─────────────────────────────────────────────────────

    fn index_of(&self, path: &Path) -> Option<usize> {
        self.entries.iter().position(|e| e.path == path)
    }

    /// Click (or key) on row `i`: alone, the range from the anchor
    /// (`extend`), or in / out of the selection (`toggle`).
    fn select_row(&mut self, i: usize, extend: bool, toggle: bool) {
        let Some(entry) = self.entries.get(i) else {
            return;
        };
        let path = entry.path.clone();
        if extend && let Some(anchor) = self.anchor {
            let (a, b) = (anchor.min(i), anchor.max(i));
            self.selected = self.entries[a..=b].iter().map(|e| e.path.clone()).collect();
        } else if toggle {
            match self.selected.iter().position(|p| *p == path) {
                Some(at) => {
                    self.selected.remove(at);
                }
                None => self.selected.push(path.clone()),
            }
            self.anchor = Some(i);
        } else {
            self.selected = vec![path.clone()];
            self.anchor = Some(i);
        }
        self.cursor = Some(i);
        self.error = None;
        self.replacing = None;
        // The name field follows a single chosen file.
        if self.mode != Mode::Folder && self.selected.len() == 1 && !entry.is_dir {
            self.name = entry.name.clone();
        }
    }

    fn move_cursor(&mut self, to: usize, extend: bool) {
        if self.entries.is_empty() {
            return;
        }
        let to = to.min(self.entries.len() - 1);
        self.select_row(to, extend && self.multiple, false);
        self.follow = true;
    }

    fn key(&mut self, key: Key, mods: Modifiers) -> Option<Outcome> {
        self.mods = mods;
        if self.naming_folder.is_some() {
            if matches!(key, Key::Named(Named::Escape)) {
                self.naming_folder = None;
            }
            return None;
        }
        if self.replacing.is_some() {
            return match key {
                Key::Named(Named::Escape) => {
                    self.replacing = None;
                    None
                }
                Key::Named(Named::Enter) => self.replacing.take().map(Outcome::Picked),
                _ => None,
            };
        }
        let cmd = mods.command() || mods.logo();
        let last = self.entries.len().saturating_sub(1);
        match key.as_ref() {
            Key::Named(Named::Escape) => return Some(Outcome::Cancelled),
            Key::Named(Named::ArrowUp) if cmd => return self.update(Message::Parent),
            Key::Named(Named::ArrowDown) if cmd => {
                let path = self.cursor.and_then(|i| self.entries.get(i)).map(|e| e.path.clone());
                return path.and_then(|p| self.update(Message::Activate(p)));
            }
            Key::Named(Named::ArrowDown) => {
                let to = self.cursor.map_or(0, |i| i + 1);
                self.move_cursor(to, mods.shift());
            }
            Key::Named(Named::ArrowUp) => {
                let to = self.cursor.map_or(0, |i| i.saturating_sub(1));
                self.move_cursor(to, mods.shift());
            }
            Key::Named(Named::Home) => self.move_cursor(0, mods.shift()),
            Key::Named(Named::End) => self.move_cursor(last, mods.shift()),
            Key::Named(Named::Enter) => {
                // Return on one folder walks into it (Choose Folder picks it).
                if self.mode != Mode::Folder
                    && let [one] = self.selected.as_slice()
                    && one.is_dir()
                {
                    let one = one.clone();
                    self.cd(one);
                    return None;
                }
                return self.confirm();
            }
            Key::Character(c) if cmd && mods.shift() && (c == "." || c == ">") => {
                return self.update(Message::ToggleHidden);
            }
            Key::Character(c) if !cmd && !mods.control() && !mods.alt() => {
                self.type_ahead(c);
            }
            _ => {}
        }
        None
    }

    /// Typing a name jumps to the first row it begins.
    fn type_ahead(&mut self, chars: &str) {
        let now = Instant::now();
        if self.typed_at.is_none_or(|t| now.duration_since(t) > TYPEAHEAD) {
            self.typed.clear();
        }
        self.typed_at = Some(now);
        self.typed.push_str(&chars.to_lowercase());
        let hit = self
            .entries
            .iter()
            .position(|e| e.name.to_lowercase().starts_with(&self.typed));
        if let Some(i) = hit {
            self.move_cursor(i, false);
        }
    }

    // ── Confirm ───────────────────────────────────────────────────────

    /// Open's result: one path, or every chosen file.
    fn picked(&self, paths: Vec<PathBuf>) -> Option<Outcome> {
        if self.multiple {
            Some(Outcome::PickedMany(paths))
        } else {
            paths.into_iter().next().map(Outcome::Picked)
        }
    }

    fn confirm(&mut self) -> Option<Outcome> {
        match self.mode {
            Mode::Open => {
                let files: Vec<PathBuf> =
                    self.selected.iter().filter(|p| p.is_file()).cloned().collect();
                if !files.is_empty() {
                    return self.picked(files);
                }
                if let [one] = self.selected.as_slice()
                    && one.is_dir()
                {
                    let one = one.clone();
                    self.cd(one);
                    return None;
                }
                let typed = self.name.trim();
                if typed.is_empty() {
                    self.error = Some("Choose a file".into());
                    return None;
                }
                let path = fs::resolve_typed(&self.cwd, typed);
                if path.is_dir() {
                    self.cd(path);
                    return None;
                }
                if path.is_file() {
                    return self.picked(vec![path]);
                }
                self.error = Some(format!("Can't find {}", path.display()));
                None
            }
            Mode::Save => {
                let typed = self.name.trim();
                let path = if typed.is_empty() {
                    match self.selected.first() {
                        Some(p) if p.is_file() => p.clone(),
                        _ => {
                            self.error = Some("Name the file".into());
                            return None;
                        }
                    }
                } else {
                    fs::resolve_typed(&self.cwd, typed)
                };
                if path.is_dir() {
                    self.cd(path);
                    return None;
                }
                if path.exists() {
                    // macOS asks before it replaces.
                    self.replacing = Some(path);
                    return None;
                }
                Some(Outcome::Picked(path))
            }
            Mode::Folder => {
                let chosen = match self.selected.as_slice() {
                    [one] if one.is_dir() => one.clone(),
                    _ => self.cwd.clone(),
                };
                Some(Outcome::Picked(chosen))
            }
        }
    }

    fn create_folder(&mut self) {
        let Some(name) = self.naming_folder.take() else {
            return;
        };
        let name = name.trim();
        if name.is_empty() || name.contains('/') {
            self.error = Some("A folder needs a name without “/”".into());
            return;
        }
        let path = self.cwd.join(name);
        match std::fs::create_dir(&path) {
            // Save and Choose Folder go into the new folder (as macOS does);
            // Open picks it out in the list.
            Ok(()) if self.mode == Mode::Open => {
                self.reload();
                if let Some(i) = self.index_of(&path) {
                    self.select_row(i, false, false);
                }
            }
            Ok(()) => self.cd(path),
            Err(e) => self.error = Some(format!("Can't make “{name}” — {e}")),
        }
    }

    fn cd(&mut self, path: PathBuf) {
        self.cwd = path;
        self.selected.clear();
        self.cursor = None;
        self.anchor = None;
        self.naming_folder = None;
        self.replacing = None;
        if self.mode == Mode::Open {
            self.name.clear();
        }
        self.scroll_y = 0.0;
        self.reload();
    }

    fn reload(&mut self) {
        self.error = None;
        let show = Show {
            filter: self.filter.as_ref(),
            hidden: self.hidden,
            dirs_only: self.mode == Mode::Folder,
        };
        match fs::list_dir(&self.cwd, show) {
            Ok(entries) => self.entries = entries,
            Err(e) => {
                self.entries.clear();
                self.error = Some(e);
            }
        }
        // Rows that went away leave the selection.
        self.selected.retain(|p| self.entries.iter().any(|e| e.path == *p));
    }

    fn can_confirm(&self) -> bool {
        match self.mode {
            Mode::Folder => true,
            Mode::Save => !self.name.trim().is_empty(),
            Mode::Open => {
                !self.name.trim().is_empty() || self.selected.iter().any(|p| p.exists())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picker over a scratch folder: `a/`, `b.txt`, `c.txt`, `d.png`.
    fn scratch(name: &str, picker: FilePicker) -> (PathBuf, FilePicker) {
        let dir = std::env::temp_dir().join(format!("kit-picker-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a")).unwrap();
        for f in ["b.txt", "c.txt", "d.png"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        let picker = picker.start_dir(&dir);
        (dir, picker)
    }

    fn key(p: &mut FilePicker, named: Named) -> Option<Outcome> {
        p.update(Message::Key(Key::Named(named), Modifiers::default()))
    }

    #[test]
    fn arrows_walk_the_rows_and_return_opens() {
        let (dir, mut p) = scratch("arrows", FilePicker::open());
        key(&mut p, Named::ArrowDown);
        assert_eq!(p.selected, vec![dir.join("a")]);
        // Return on a folder walks into it.
        assert_eq!(key(&mut p, Named::Enter), None);
        assert_eq!(p.cwd(), dir.join("a"));
        // ⌘↑ comes back out, on the folder.
        let up = Modifiers::LOGO;
        p.update(Message::Key(Key::Named(Named::ArrowUp), up));
        assert_eq!(p.cwd(), dir);
        assert_eq!(p.selected, vec![dir.join("a")]);
        key(&mut p, Named::ArrowDown);
        assert_eq!(
            key(&mut p, Named::Enter),
            Some(Outcome::Picked(dir.join("b.txt")))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn typing_jumps_to_a_name() {
        let (dir, mut p) = scratch("typeahead", FilePicker::open());
        p.update(Message::Key(Key::Character("c".into()), Modifiers::default()));
        assert_eq!(p.selected, vec![dir.join("c.txt")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn several_files_with_shift_and_command() {
        let (dir, mut p) = scratch("many", FilePicker::open().multiple());
        p.update(Message::Select(dir.join("b.txt")));
        p.update(Message::Modifiers(Modifiers::SHIFT));
        p.update(Message::Select(dir.join("d.png")));
        assert_eq!(p.selected.len(), 3);
        p.update(Message::Modifiers(Modifiers::LOGO));
        p.update(Message::Select(dir.join("c.txt")));
        assert_eq!(
            p.update(Message::Confirm),
            Some(Outcome::PickedMany(vec![dir.join("b.txt"), dir.join("d.png")]))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_asks_before_replacing() {
        let (dir, mut p) = scratch("replace", FilePicker::save().suggested_name("b.txt"));
        assert_eq!(p.update(Message::Confirm), None);
        assert_eq!(p.replacing, Some(dir.join("b.txt")));
        assert_eq!(p.update(Message::KeepFile), None);
        p.update(Message::Confirm);
        assert_eq!(
            p.update(Message::Replace),
            Some(Outcome::Picked(dir.join("b.txt")))
        );
        p.update(Message::NameChanged("new.txt".into()));
        assert_eq!(
            p.update(Message::Confirm),
            Some(Outcome::Picked(dir.join("new.txt")))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_folder_and_choose_folder() {
        let (dir, mut p) = scratch("folder", FilePicker::folder());
        // Folders only.
        assert_eq!(p.entries.len(), 1);
        p.update(Message::NewFolder);
        p.update(Message::FolderNameChanged("Made".into()));
        p.update(Message::CreateFolder);
        assert_eq!(p.cwd(), dir.join("Made"));
        assert_eq!(
            p.update(Message::Confirm),
            Some(Outcome::Picked(dir.join("Made")))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hidden_files_on_request() {
        let (dir, mut p) = scratch("hidden", FilePicker::open());
        std::fs::write(dir.join(".dot"), "x").unwrap();
        p.reload();
        let had = p.entries.len();
        p.update(Message::Key(Key::Character(".".into()), Modifiers::LOGO | Modifiers::SHIFT));
        assert_eq!(p.entries.len(), had + 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
