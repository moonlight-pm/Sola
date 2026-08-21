//! File picker — kit overlay for Open / Save.
//!
//! **THESIS:** the path is a trail you walk, not a string you type.
//! Clickable breadcrumb chips are the signature; the name field is only
//! for the leaf.
//! **OWN-WORLD:** graphite modal (`card::modal`), etch places rail,
//! quiet file stack. Accent only on the single confirm action.
//! **STORY:** pick a place, walk crumbs, select a row, Open or Save.
//! **FIRST VIEWPORT:** title + crumbs on top; Places | files; name +
//! Cancel / confirm along the bottom.
//!
//! Stateful like [`ColorPicker`]: the caller holds a [`FilePicker`],
//! routes [`Message`] through [`FilePicker::update`], and acts on
//! [`Outcome`].
//!
//! ```ignore
//! self.picker = Some(FilePicker::open().title("Open image").filter("Images", &["png"]));
//! match picker.update(m) {
//!     Some(Outcome::Picked(path)) => { /* … */ }
//!     Some(Outcome::Cancelled) => self.picker = None,
//!     None => {}
//! }
//! picker.overlay() // dim + centered modal
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use iced::widget::{Space, column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};

use crate::components::button as kit_btn;
use crate::components::card;
use crate::components::icon::{icon_handle, icon_svg};
use crate::components::style::{
    HAIRLINE_A, PAD_CONTROL_SM, RADIUS_MD, RADIUS_SM, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL,
    hairline_on, inset_surface,
};
use crate::components::text as kit_text;
use crate::components::text_input::text_input;
use crate::components::{SidebarItem, SidebarPanel, SidebarSection};
use crate::fonts;

/// Open an existing file, or name a destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Open,
    Save,
}

/// Extension filter. Directories always stay visible.
#[derive(Debug, Clone)]
pub struct Filter {
    pub label: String,
    pub extensions: Vec<String>,
}

/// Messages the picker emits. The parent forwards them into [`FilePicker::update`].
#[derive(Debug, Clone)]
pub enum Message {
    Select(PathBuf),
    Activate(PathBuf),
    Crumb(PathBuf),
    Place(usize),
    NameChanged(String),
    Confirm,
    Cancel,
    Parent,
}

/// Terminal result of [`FilePicker::update`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Picked(PathBuf),
    Cancelled,
}

#[derive(Debug, Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
    size: Option<u64>,
}

#[derive(Debug, Clone)]
struct Place {
    label: String,
    path: PathBuf,
}

/// In-app file picker. Reloads the directory listing on each navigation.
pub struct FilePicker {
    mode: Mode,
    title: String,
    cwd: PathBuf,
    home: Option<PathBuf>,
    entries: Vec<Entry>,
    selected: Option<PathBuf>,
    name: String,
    filter: Option<Filter>,
    error: Option<String>,
    places: Vec<Place>,
}

impl FilePicker {
    pub fn open() -> Self {
        Self::new(Mode::Open, "Open")
    }

    pub fn save() -> Self {
        Self::new(Mode::Save, "Save")
    }

    fn new(mode: Mode, title: &str) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let cwd = home
            .as_ref()
            .filter(|p| p.is_dir())
            .cloned()
            .unwrap_or_else(|| PathBuf::from("/"));
        let places = default_places(home.as_deref());
        let mut picker = Self {
            mode,
            title: title.into(),
            cwd,
            home,
            entries: Vec::new(),
            selected: None,
            name: String::new(),
            filter: None,
            error: None,
            places,
        };
        picker.reload();
        picker
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
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
        self.cd(dir);
        self
    }

    pub fn suggested_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn update(&mut self, msg: Message) -> Option<Outcome> {
        match msg {
            Message::Select(path) => {
                self.selected = Some(path.clone());
                self.error = None;
                if path.is_file() {
                    if let Some(name) = path.file_name() {
                        self.name = name.to_string_lossy().into_owned();
                    }
                }
                None
            }
            Message::Activate(path) => {
                if path.is_dir() {
                    self.cd(path);
                    None
                } else {
                    Some(Outcome::Picked(path))
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
                None
            }
            Message::Confirm => self.confirm(),
            Message::Cancel => Some(Outcome::Cancelled),
            Message::Parent => {
                if let Some(parent) = self.cwd.parent() {
                    self.cd(parent.to_path_buf());
                }
                None
            }
        }
    }

    fn confirm(&mut self) -> Option<Outcome> {
        match self.mode {
            Mode::Open => {
                if let Some(sel) = self.selected.clone() {
                    if sel.is_dir() {
                        self.cd(sel);
                        return None;
                    }
                    return Some(Outcome::Picked(sel));
                }
                let typed = self.name.trim();
                if typed.is_empty() {
                    self.error = Some("Choose a file".into());
                    return None;
                }
                let path = resolve_typed(&self.cwd, typed);
                if path.is_dir() {
                    self.cd(path);
                    return None;
                }
                if path.is_file() {
                    return Some(Outcome::Picked(path));
                }
                self.error = Some(format!("Can't find {}", path.display()));
                None
            }
            Mode::Save => {
                let typed = self.name.trim();
                let path = if typed.is_empty() {
                    match self.selected.clone() {
                        Some(p) if p.is_file() => p,
                        _ => {
                            self.error = Some("Name the file".into());
                            return None;
                        }
                    }
                } else {
                    resolve_typed(&self.cwd, typed)
                };
                if path.is_dir() {
                    self.cd(path);
                    return None;
                }
                Some(Outcome::Picked(path))
            }
        }
    }

    fn cd(&mut self, path: PathBuf) {
        self.cwd = path;
        self.selected = None;
        if self.mode == Mode::Open {
            self.name.clear();
        }
        self.reload();
    }

    fn reload(&mut self) {
        self.error = None;
        match list_dir(&self.cwd, self.filter.as_ref()) {
            Ok(entries) => self.entries = entries,
            Err(e) => {
                self.entries.clear();
                self.error = Some(e);
            }
        }
    }

    fn can_confirm(&self) -> bool {
        if !self.name.trim().is_empty() {
            return true;
        }
        matches!(&self.selected, Some(p) if p.is_file() || (self.mode == Mode::Open && p.is_dir()))
    }

    /// Dimmed full-window overlay. Stack this over the app.
    pub fn overlay(&self) -> Element<'_, Message, Theme> {
        container(self.view())
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.48,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            })
            .into()
    }

    /// The modal panel (no dim). Storybook uses this; apps usually want [`Self::overlay`].
    pub fn view(&self) -> Element<'_, Message, Theme> {
        let crumbs = self.breadcrumb_row();
        let places = self.places_rail();
        let files = self.file_list();
        let body = row![places, files]
            .spacing(SPACE_MD)
            .height(Length::Fixed(340.0));

        let name = text_input("Name", &self.name)
            .on_input(Message::NameChanged)
            .on_submit(Message::Confirm);

        let confirm_label = match self.mode {
            Mode::Open => "Open",
            Mode::Save => "Save",
        };
        let mut confirm = kit_btn::labeled(confirm_label, kit_btn::primary);
        if self.can_confirm() {
            confirm = confirm.on_press(Message::Confirm);
        }
        let cancel = kit_btn::labeled("Cancel", kit_btn::secondary).on_press(Message::Cancel);

        let mut footer = column![
            row![container(name).width(Length::Fill), cancel, confirm,]
                .spacing(SPACE_MD)
                .align_y(Alignment::Center),
        ]
        .spacing(SPACE_SM);

        if let Some(err) = self.error.as_deref() {
            footer = footer.push(kit_text::caption(err).style(kit_text::danger));
        } else if let Some(filter) = self.filter.as_ref() {
            footer = footer.push(
                kit_text::caption(format!("Showing {}", filter.label)).style(kit_text::muted),
            );
        }

        let panel = column![
            kit_text::subheading(self.title.clone()),
            crumbs,
            body,
            footer,
        ]
        .spacing(SPACE_LG)
        .padding(SPACE_XL)
        .width(Length::Fill);

        // Width lives on the modal frame. `card::modal` faces are Fill, so
        // a Fixed child is ignored and the overlay would stretch full-bleed.
        card::modal(panel).width(Length::Fixed(PANEL_W)).into()
    }

    fn breadcrumb_row(&self) -> Element<'_, Message, Theme> {
        let crumbs = breadcrumbs(&self.cwd, self.home.as_deref());
        let last = crumbs.len().saturating_sub(1);
        let mut trail = row![].spacing(SPACE_XS_LOCAL).align_y(Alignment::Center);
        for (i, (label, path)) in crumbs.into_iter().enumerate() {
            if i > 0 {
                trail = trail.push(icon_svg(chevron_handle(), 12));
            }
            // Same chip metrics for current and ancestors so Home (one
            // label) and Downloads (Home › Downloads) share a row height.
            trail = trail.push(crumb_chip(label, (i != last).then_some(path)));
        }
        container(trail)
            .width(Length::Fill)
            .height(Length::Fixed(CRUMB_H))
            .center_y(Length::Fixed(CRUMB_H))
            .clip(true)
            .into()
    }

    fn places_rail(&self) -> Element<'_, Message, Theme> {
        let items: Vec<SidebarItem<'_, Message>> = self
            .places
            .iter()
            .enumerate()
            .map(|(i, place)| {
                SidebarItem::new(place.label.clone(), Message::Place(i))
                    .active(closest_place(&self.places, &self.cwd) == Some(i))
            })
            .collect();

        container(
            SidebarPanel::new(vec![SidebarSection::new("Places", items)])
                .fill_width()
                .build(),
        )
        .width(Length::Fixed(168.0))
        .height(Length::Fill)
        .into()
    }

    fn file_list(&self) -> Element<'_, Message, Theme> {
        let body: Element<'_, Message, Theme> = if self.entries.is_empty() && self.error.is_none() {
            container(kit_text::caption("This folder is empty").style(kit_text::muted))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .padding(SPACE_XL)
                .into()
        } else {
            let rows: Vec<Element<'_, Message, Theme>> = self
                .entries
                .iter()
                .map(|entry| {
                    file_row(
                        entry,
                        self.selected.as_deref() == Some(entry.path.as_path()),
                    )
                })
                .collect();
            scrollable(column(rows).spacing(1).width(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(SPACE_SM)
            .style(list_well)
            .into()
    }
}

const SPACE_XS_LOCAL: f32 = 6.0;
/// Dialog width — not Fill. Overlay centers this; the modal face is Fill inside it.
const PANEL_W: f32 = 720.0;
/// 12px type + [`PAD_CONTROL_SM`] vertical. Fixed so crumb count can't
/// resize the modal (Home vs Home › Downloads).
const CRUMB_H: f32 = 28.0;

fn crumb_chip(label: String, target: Option<PathBuf>) -> Element<'static, Message, Theme> {
    let current = target.is_none();
    let label = text(label)
        .font(if current {
            fonts::ui_medium()
        } else {
            fonts::ui()
        })
        .size(12);
    let mut chip = iced::widget::button(label)
        .padding(PAD_CONTROL_SM)
        .style(kit_btn::ghost);
    if let Some(path) = target {
        chip = chip.on_press(Message::Crumb(path));
    }
    chip.into()
}

fn file_row(entry: &Entry, selected: bool) -> Element<'static, Message, Theme> {
    let icon = if entry.is_dir {
        folder_handle()
    } else if is_image_name(&entry.name) {
        image_handle()
    } else {
        file_handle()
    };
    let size = entry.size.map(human_size).unwrap_or_default();
    let size_el: Element<'static, Message, Theme> = if size.is_empty() {
        Space::new().width(0).into()
    } else {
        text(size)
            .font(fonts::mono())
            .size(11)
            .style(kit_text::muted)
            .into()
    };
    let content = row![
        icon_svg(icon, 14),
        text(entry.name.clone())
            .font(fonts::ui())
            .size(13)
            .width(Length::Fill),
        size_el,
    ]
    .spacing(SPACE_MD)
    .align_y(Alignment::Center)
    .padding(Padding {
        top: 6.0,
        right: SPACE_MD,
        bottom: 6.0,
        left: SPACE_MD,
    });

    let path = entry.path.clone();
    let activate = entry.path.clone();
    mouse_area(
        container(content)
            .width(Length::Fill)
            .style(move |theme| row_style(theme, selected)),
    )
    .on_press(Message::Select(path))
    .on_double_click(Message::Activate(activate))
    .into()
}

fn row_style(theme: &Theme, selected: bool) -> container::Style {
    let p = theme.extended_palette();
    if selected {
        let fill = inset_surface(p.background.weaker.color, 0.10);
        container::Style {
            background: Some(Background::Color(fill)),
            border: hairline_on(fill, HAIRLINE_A, RADIUS_SM),
            ..container::Style::default()
        }
    } else {
        container::Style {
            background: None,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: RADIUS_SM.into(),
            },
            ..container::Style::default()
        }
    }
}

fn list_well(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let fill = p.background.base.color;
    container::Style {
        background: Some(Background::Color(fill)),
        border: hairline_on(fill, HAIRLINE_A, RADIUS_MD),
        ..container::Style::default()
    }
}

fn resolve_typed(cwd: &Path, typed: &str) -> PathBuf {
    let path = PathBuf::from(typed);
    if path.is_absolute() {
        path
    } else {
        cwd.join(typed)
    }
}

fn default_places(home: Option<&Path>) -> Vec<Place> {
    let mut places = Vec::new();
    if let Some(home) = home {
        if home.is_dir() {
            places.push(Place {
                label: "Home".into(),
                path: home.to_path_buf(),
            });
        }
        for (label, name) in [
            ("Desktop", "Desktop"),
            ("Pictures", "Pictures"),
            ("Downloads", "Downloads"),
        ] {
            let p = home.join(name);
            if p.is_dir() {
                places.push(Place {
                    label: label.into(),
                    path: p,
                });
            }
        }
    }
    let shots = PathBuf::from("/tmp/sola/screenshots");
    if shots.is_dir() {
        places.push(Place {
            label: "Screenshots".into(),
            path: shots,
        });
    }
    places.push(Place {
        label: "Disk".into(),
        path: PathBuf::from("/"),
    });
    places
}

/// Longest matching place prefix (skip bare `/` unless that's the only hit).
fn closest_place(places: &[Place], cwd: &Path) -> Option<usize> {
    places
        .iter()
        .enumerate()
        .filter(|(_, p)| cwd.starts_with(&p.path))
        .max_by_key(|(_, p)| p.path.as_os_str().len())
        .map(|(i, _)| i)
}

pub(crate) fn breadcrumbs(cwd: &Path, home: Option<&Path>) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    if let Some(home) = home {
        if cwd == home {
            return vec![("Home".into(), home.to_path_buf())];
        }
        if cwd.starts_with(home) {
            out.push(("Home".into(), home.to_path_buf()));
            if let Ok(rel) = cwd.strip_prefix(home) {
                let mut acc = home.to_path_buf();
                for comp in rel.components() {
                    acc.push(&comp);
                    out.push((comp.as_os_str().to_string_lossy().into_owned(), acc.clone()));
                }
            }
            return out;
        }
    }
    out.push(("/".into(), PathBuf::from("/")));
    if cwd == Path::new("/") {
        return out;
    }
    let mut acc = PathBuf::from("/");
    for comp in cwd.components().skip(1) {
        acc.push(&comp);
        out.push((comp.as_os_str().to_string_lossy().into_owned(), acc.clone()));
    }
    out
}

fn list_dir(dir: &Path, filter: Option<&Filter>) -> Result<Vec<Entry>, String> {
    let rd = fs::read_dir(dir).map_err(|e| format!("Can't open {} — {e}", dir.display()))?;
    let mut entries = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let path = ent.path();
        // `file_type` is usually free (d_type); avoid a second stat per row.
        let is_dir = ent
            .file_type()
            .map(|t| t.is_dir())
            .unwrap_or_else(|_| path.is_dir());
        if !is_dir && !matches_filter(&name, filter) {
            continue;
        }
        let size = if is_dir {
            None
        } else {
            ent.metadata().ok().map(|m| m.len())
        };
        entries.push(Entry {
            path,
            name: name.into_owned(),
            is_dir,
            size,
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

fn matches_filter(name: &str, filter: Option<&Filter>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    if filter.extensions.is_empty() {
        return true;
    }
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    filter
        .extensions
        .iter()
        .any(|want| ext.eq_ignore_ascii_case(want))
}

const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "tga",
];

fn is_image_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| IMAGE_EXTS.iter().any(|want| ext.eq_ignore_ascii_case(want)))
}

pub(crate) fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let n = bytes as f64;
    if n < KB {
        format!("{bytes} B")
    } else if n < MB {
        format!("{:.0} KB", n / KB)
    } else if n < GB {
        format!("{:.1} MB", n / MB)
    } else {
        format!("{:.1} GB", n / GB)
    }
}

fn chevron_handle() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/chevron-right"))
        .clone()
}

fn folder_handle() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/folder")).clone()
}

fn file_handle() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/file")).clone()
}

fn image_handle() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/image")).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crumbs_under_home() {
        let home = Path::new("/home/ada");
        let crumbs = breadcrumbs(&home.join("Pictures/shots"), Some(home));
        assert_eq!(crumbs.len(), 3);
        assert_eq!(crumbs[0].0, "Home");
        assert_eq!(crumbs[1].0, "Pictures");
        assert_eq!(crumbs[2].0, "shots");
        assert_eq!(crumbs[2].1, home.join("Pictures/shots"));
    }

    #[test]
    fn crumbs_at_home() {
        let home = Path::new("/home/ada");
        let crumbs = breadcrumbs(home, Some(home));
        assert_eq!(crumbs.len(), 1);
        assert_eq!(crumbs[0].0, "Home");
    }

    #[test]
    fn crumbs_from_root() {
        let crumbs = breadcrumbs(Path::new("/usr/share"), None);
        assert_eq!(
            crumbs.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(),
            ["/", "usr", "share"]
        );
    }

    #[test]
    fn filter_matches_extension_case() {
        let filter = Filter {
            label: "Images".into(),
            extensions: vec!["png".into(), "JPEG".into()],
        };
        assert!(matches_filter("Shot.PNG", Some(&filter)));
        assert!(matches_filter("x.jpeg", Some(&filter)));
        assert!(!matches_filter("notes.txt", Some(&filter)));
        assert!(matches_filter("any.bin", None));
    }

    #[test]
    fn human_size_steps() {
        assert_eq!(human_size(400), "400 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(1_572_864), "1.5 MB");
    }

    #[test]
    fn closest_place_prefers_longest_prefix() {
        let places = vec![
            Place {
                label: "Home".into(),
                path: PathBuf::from("/home/ada"),
            },
            Place {
                label: "Pictures".into(),
                path: PathBuf::from("/home/ada/Pictures"),
            },
            Place {
                label: "Disk".into(),
                path: PathBuf::from("/"),
            },
        ];
        assert_eq!(
            closest_place(&places, Path::new("/home/ada/Pictures/x")),
            Some(1)
        );
        assert_eq!(closest_place(&places, Path::new("/tmp")), Some(2));
    }
}
