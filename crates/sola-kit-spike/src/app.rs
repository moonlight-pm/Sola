//! Storybook store → HTML/CSS → layout → GPU layers. Rust events, no JS.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::components::{Sidebar, SidebarItem};
use crate::css::{Sheet, parse_sheet};
use crate::gpu::Quad;
use crate::icons::Icons;
use crate::layout::{PaintItem, hit_test, hover_at, layout_tree, point_in_item};
use crate::markup::{self};
use crate::paint::{Fonts, PaintPass, paint_glyphs};
use crate::palette::{
    ATOMS, SELECT_NAMES, SELECT_SEEDS, format_hex, hsv_to_rgb, page_atoms, parse_hex, rgb_to_hsl,
    rgb_to_hsv, theme_vars,
};

const CSS: &str = include_str!("../assets/kit.css");
const HTML: &str = include_str!("../assets/kit.html");

struct Page {
    id: &'static str,
    label: &'static str,
    section: &'static str,
    heading: &'static str,
    lede: &'static str,
}

const PAGES: &[Page] = &[
    Page {
        id: "overview",
        label: "Overview",
        section: "System",
        heading: "Overview",
        lede: "Cool graphite tool UI. One filled accent per group. Selection is a quiet well, not a slab. Edit seeds on Theme.",
    },
    Page {
        id: "theme",
        label: "Theme",
        section: "System",
        heading: "Theme",
        lede: "Fonts and seed atoms. Edits stay in this window — no bus in the spike.",
    },
    Page {
        id: "shell",
        label: "Shell",
        section: "System",
        heading: "Shell",
        lede: "Shell chrome tokens. Colors carry alpha. The running shell restyles as you edit — not in this spike.",
    },
    Page {
        id: "divider",
        label: "Divider",
        section: "Layout",
        heading: "Divider",
        lede: "Hairline between panes. Drag lives on Split.",
    },
    Page {
        id: "split",
        label: "Split",
        section: "Layout",
        heading: "Split",
        lede: "Two panes, live divider. Drag the hairline.",
    },
    Page {
        id: "toolbar",
        label: "Toolbar",
        section: "Layout",
        heading: "Toolbar",
        lede: "A monitor-style action row.",
    },
    Page {
        id: "text",
        label: "Text",
        section: "Components",
        heading: "Text",
        lede: "Roles, not ad-hoc sizes. Display for rare emphasis; UI for everything else; mono for data.",
    },
    Page {
        id: "json",
        label: "JSON",
        section: "Components",
        heading: "JSON",
        lede: "Inspector payloads. Keys stay primary text; strings success; numbers warning; literals accent.",
    },
    Page {
        id: "button",
        label: "Button",
        section: "Components",
        heading: "Button",
        lede: "Use labeled / labeled_sm. One filled accent per group. Ghost stays muted until hover. Danger never competes with Save.",
    },
    Page {
        id: "titlebar",
        label: "Titlebar",
        section: "Components",
        heading: "Titlebar",
        lede: "macOS-adjacent float chrome: traffic-light close, centered title, rounded frame.",
    },
    Page {
        id: "badge",
        label: "Badge",
        section: "Components",
        heading: "Badge",
        lede: "Status pills. Accent is neon type on graphite — never a darkened-cyan fill.",
    },
    Page {
        id: "card",
        label: "Card",
        section: "Components",
        heading: "Card",
        lede: "Product surfaces, not an API sampler.",
    },
    Page {
        id: "field",
        label: "Field",
        section: "Components",
        heading: "Field",
        lede: "The stacked form used in dialogs and account panels. Not a catalog of inputs.",
    },
    Page {
        id: "form",
        label: "Form",
        section: "Components",
        heading: "Form",
        lede: "Settings-grade path. One panel, not two stacked sample cards.",
    },
    Page {
        id: "icon",
        label: "Icon",
        section: "Components",
        heading: "Icon",
        lede: "Kit atoms — not one-off hex.",
    },
    Page {
        id: "number_input",
        label: "NumberInput",
        section: "Components",
        heading: "NumberInput",
        lede: "Token steppers in a settings panel.",
    },
    Page {
        id: "readable",
        label: "Readable",
        section: "Components",
        heading: "Readable",
        lede: "Cap the measure. Long copy stays ~65ch instead of spanning the window.",
    },
    Page {
        id: "prose",
        label: "Prose",
        section: "Components",
        heading: "Prose",
        lede: "Letter measure: paragraphs, quoted replies, inline links.",
    },
    Page {
        id: "color_picker",
        label: "ColorPicker",
        section: "Components",
        heading: "ColorPicker",
        lede: "Drag the field and rails, or type Hex / RGB / HSL. Hue survives value → 0.",
    },
    Page {
        id: "file_picker",
        label: "FilePicker",
        section: "Components",
        heading: "FilePicker",
        lede: "Path is a trail of chips, not a typed string. Places on the left, files in a quiet well.",
    },
    Page {
        id: "popover",
        label: "Popover",
        section: "Components",
        heading: "Popover",
        lede: "Menu chrome: raised face, hairline, tight shadow. Anchor and dismiss stay with the caller.",
    },
    Page {
        id: "context_menu",
        label: "Context menu",
        section: "Components",
        heading: "Context menu",
        lede: "Flat actions at the pointer. Escape or click outside dismisses.",
    },
    Page {
        id: "select",
        label: "Select",
        section: "Components",
        heading: "Select",
        lede: "Identity select. The menu hangs under the trigger at the trigger's width — a raised popover, not a darker inset card.",
    },
    Page {
        id: "sidebar",
        label: "Sidebar",
        section: "Components",
        heading: "Sidebar",
        lede: "List etch: muted idle, reserved lip so selected text does not shift, inset active, hover-only ×.",
    },
];

const SIDE_ITEMS: [&str; 5] = ["Inbox", "Drafts", "Sent", "Archive", "Spam"];
const TAB_ORDER: &[&str] = &[
    "user",
    "email",
    "display",
    "hex",
    "rgb_r",
    "rgb_g",
    "rgb_b",
    "hsl_h",
    "hsl_s",
    "hsl_l",
    "radius",
    "opacity",
    "open-name",
    "save-name",
];

pub struct App {
    pub css_w: f32,
    pub css_h: f32,
    pub scale: f32,
    sheet: Sheet,
    fonts: Fonts,
    hover: Option<u32>,
    selected: &'static str,
    theme: String,
    theme_open: bool,
    theme_editable: bool,
    popover_open: bool,
    select_open: Option<String>,
    select_chrome: usize,
    select_form: usize,
    select_overview: usize,
    confirm_armed: bool,
    context: Option<(f32, f32)>,
    status: String,
    toggles: HashMap<String, bool>,
    css_path: std::path::PathBuf,
    css_mtime: Option<std::time::SystemTime>,
    html_path: std::path::PathBuf,
    html_mtime: Option<std::time::SystemTime>,
    html: String,
    assets: std::path::PathBuf,
    page_mtime: Option<std::time::SystemTime>,
    last_items: Vec<PaintItem>,
    icons: Icons,
    time: f32,
    split: f32,
    split_h: f32,
    picker_h: f32,
    picker_s: f32,
    picker_v: f32,
    picker_a: f32,
    drag: Drag,
    focused: Option<String>,
    fields: HashMap<String, String>,
    prose: Option<(u32, f32, f32)>,
    scrolls: HashMap<String, f32>,
    atom_overrides: HashMap<String, String>,
    editing_atom: Option<String>,
    open: Files,
    save: Files,
    side_order: Vec<usize>,
    side_selected: usize,
    side_group_closed: bool,
}

struct Files {
    cwd: PathBuf,
    selected: Option<PathBuf>,
    entries: Vec<FileEnt>,
    name: String,
    places: Vec<(String, PathBuf)>,
    filter: Vec<String>,
}

#[derive(Clone)]
struct FileEnt {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Drag {
    None,
    Split,
    SplitH,
    Prose,
    Sv,
    Hue,
    Alpha,
}

impl App {
    pub fn new(css_w: f32, css_h: f32, scale: f32) -> Self {
        let assets = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let css_path = assets.join("kit.css");
        let html_path = assets.join("kit.html");
        let (sheet, css_mtime) = load_sheet(&css_path);
        let (html, html_mtime) = load_html(&html_path);
        let mut me = Self {
            css_w,
            css_h,
            scale: scale.max(0.01),
            sheet,
            fonts: Fonts::new(),
            hover: None,
            selected: PAGES[0].id,
            theme: "Default".into(),
            theme_open: false,
            theme_editable: false,
            popover_open: false,
            select_open: None,
            select_chrome: 0,
            select_form: 0,
            select_overview: 0,
            confirm_armed: false,
            context: None,
            status: String::new(),
            toggles: HashMap::from([
                ("wifi".into(), true),
                ("bt".into(), false),
                ("login".into(), true),
                ("notify".into(), false),
                ("analytics".into(), false),
            ]),
            css_path,
            css_mtime,
            html_path,
            html_mtime,
            html,
            assets,
            page_mtime: None,
            last_items: Vec::new(),
            icons: Icons::new(),
            time: 0.0,
            split: 0.5,
            split_h: 0.45,
            picker_h: 0.52,
            picker_s: 0.55,
            picker_v: 0.96,
            picker_a: 1.0,
            drag: Drag::None,
            focused: None,
            fields: HashMap::from([
                ("user".into(), String::new()),
                ("email".into(), String::new()),
                ("display".into(), String::new()),
                ("radius".into(), "8".into()),
                ("opacity".into(), "80".into()),
                ("open-name".into(), String::new()),
                ("save-name".into(), "untitled.png".into()),
            ]),
            prose: None,
            scrolls: HashMap::new(),
            atom_overrides: HashMap::new(),
            editing_atom: None,
            open: Files::open(),
            save: Files::save(),
            side_order: (0..SIDE_ITEMS.len()).collect(),
            side_selected: 0,
            side_group_closed: false,
        };
        me.restyle();
        me.sync_picker_fields(true);
        me
    }

    pub fn tick(&mut self, dt: f32) {
        self.time += dt;
    }

    pub fn time(&self) -> f32 {
        self.time
    }

    pub fn needs_frame(&self) -> bool {
        self.selected == "sidebar"
            || self.selected == "icon"
            || self.focused.is_some()
            || self.drag != Drag::None
    }

    pub fn has_overlay(&self) -> bool {
        self.theme_open
            || self.popover_open
            || self.select_open.is_some()
            || self.context.is_some()
            || self.editing_atom.is_some()
    }

    pub fn has_focus(&self) -> bool {
        self.focused.is_some()
    }

    pub fn blur(&mut self) {
        self.focused = None;
        self.sync_picker_fields(true);
        self.clamp_numbers();
    }

    pub fn dismiss_overlays(&mut self) -> bool {
        let any = self.has_overlay();
        self.theme_open = false;
        self.popover_open = false;
        self.select_open = None;
        self.context = None;
        self.editing_atom = None;
        any
    }

    pub fn type_text(&mut self, s: &str) {
        let Some(id) = self.focused.clone() else {
            return;
        };
        self.fields.entry(id.clone()).or_default().push_str(s);
        self.after_field_edit(&id);
    }

    pub fn backspace(&mut self) {
        let Some(id) = self.focused.clone() else {
            return;
        };
        self.fields.entry(id.clone()).or_default().pop();
        self.after_field_edit(&id);
    }

    pub fn tab(&mut self, back: bool) {
        let cur = self.focused.as_deref();
        let idx = TAB_ORDER.iter().position(|k| Some(*k) == cur);
        let next = match (idx, back) {
            (Some(i), false) => (i + 1) % TAB_ORDER.len(),
            (Some(i), true) => (i + TAB_ORDER.len() - 1) % TAB_ORDER.len(),
            (None, false) => 0,
            (None, true) => TAB_ORDER.len() - 1,
        };
        self.focused = Some(TAB_ORDER[next].to_string());
        self.sync_picker_fields(true);
    }

    pub fn arrow(&mut self, up: bool) {
        let Some(id) = self.focused.clone() else {
            return;
        };
        if id == "radius" || id == "opacity" {
            self.step(&id, !up);
        }
    }

    pub fn enter(&mut self) {
        match self.focused.as_deref() {
            Some("open-name") => {
                self.confirm_file(true);
            }
            Some("save-name") => {
                self.confirm_file(false);
            }
            Some("hex") => {
                self.sync_picker_fields(true);
            }
            _ => {}
        }
    }

    pub fn mouse_up(&mut self) {
        self.drag = Drag::None;
    }

    pub fn buffer_size(&self) -> (u32, u32) {
        (
            (self.css_w * self.scale).round().max(1.0) as u32,
            (self.css_h * self.scale).round().max(1.0) as u32,
        )
    }

    pub fn reload_if_changed(&mut self) -> bool {
        let mut changed = false;
        if let Some((sheet, mtime)) = reload_if_newer(&self.css_path, self.css_mtime) {
            self.sheet = parse_sheet(&sheet);
            self.css_mtime = mtime;
            self.restyle();
            tracing::info!(path = %self.css_path.display(), "reloaded CSS");
            changed = true;
        }
        if let Some((html, mtime)) = reload_if_newer(&self.html_path, self.html_mtime) {
            self.html = html;
            self.html_mtime = mtime;
            tracing::info!(path = %self.html_path.display(), "reloaded HTML");
            changed = true;
        }
        let page_path = self
            .assets
            .join("pages")
            .join(format!("{}.html", self.selected));
        if let Some((_, mtime)) = reload_if_newer(&page_path, self.page_mtime) {
            self.page_mtime = mtime;
            tracing::info!(path = %page_path.display(), "reloaded page");
            changed = true;
        }
        changed
    }

    pub fn live_layers(&mut self) -> (Vec<Quad>, Option<Vec<u32>>) {
        self.reload_if_changed();
        self.rebuild_items();
        let (bw, bh) = self.buffer_size();
        let quads = chrome_quads(
            &self.last_items,
            self.scale,
            bw,
            bh,
            self.picker_h,
            hsv_to_rgb(self.picker_h, self.picker_s, self.picker_v, self.picker_a),
        );
        let sel = self.prose.map(|(uid, a, b)| (uid, a, b));
        let caret = self.caret_px();
        let pix = Some(paint_glyphs(
            &self.last_items,
            &mut self.fonts,
            self.css_w,
            self.css_h,
            self.scale,
            &mut PaintPass {
                time: self.time,
                sel,
                caret,
                field_scroll: 0.0,
                focus_uid: None,
                icons: &mut self.icons,
            },
        ));
        (quads, pix)
    }

    fn caret_px(&mut self) -> Option<(u32, f32)> {
        let id = self.focused.as_deref()?;
        if self.time.fract() > 0.5 {
            return None;
        }
        let item = self
            .last_items
            .iter()
            .find(|i| i.data_id.as_deref() == Some(id))?;
        let text = self.fields.get(id).map(|s| s.as_str()).unwrap_or("");
        let run = item.text.as_ref();
        let size = run.map(|r| r.size).unwrap_or(13.0);
        let weight = run.map(|r| r.weight).unwrap_or(400);
        let family = run.map(|r| r.family.as_str()).unwrap_or("SF Pro Text");
        let w = self.fonts.measure_width(text, size, weight, family);
        Some((item.uid, item.x + item.pad[3] + w))
    }

    pub fn wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if self.drag != Drag::None || dy.abs() < 0.1 {
            return false;
        }
        let mut found = None;
        for item in &self.last_items {
            if item.overflow_scroll && point_in_item(item, x, y) {
                if let Some(id) = item.data_id.as_deref() {
                    let max = (item.content_h - item.h).max(0.0);
                    found = Some((id.to_string(), max));
                }
            }
        }
        let Some((id, max)) = found else {
            return false;
        };
        let cur = self.scrolls.get(&id).copied().unwrap_or(0.0);
        let next = (cur + dy).clamp(0.0, max);
        if (next - cur).abs() < 0.5 {
            return false;
        }
        self.scrolls.insert(id, next);
        true
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        let mut dirty = false;
        match self.drag {
            Drag::Split => {
                if let Some(box_) = self.last_items.iter().find(|i| {
                    i.classes.iter().any(|c| c == "split")
                        && !i.classes.iter().any(|c| c == "split-col")
                }) {
                    let t = ((x - box_.x) / box_.w.max(1.0)).clamp(0.2, 0.8);
                    if (t - self.split).abs() > 0.002 {
                        self.split = t;
                        dirty = true;
                    }
                }
            }
            Drag::SplitH => {
                if let Some(box_) = self
                    .last_items
                    .iter()
                    .find(|i| i.classes.iter().any(|c| c == "split-col"))
                {
                    let t = ((y - box_.y) / box_.h.max(1.0)).clamp(0.2, 0.8);
                    if (t - self.split_h).abs() > 0.002 {
                        self.split_h = t;
                        dirty = true;
                    }
                }
            }
            Drag::Prose => {
                if let Some((uid, a, _)) = self.prose {
                    self.prose = Some((uid, a, x));
                    dirty = true;
                }
            }
            Drag::Sv => {
                if let Some(it) = self
                    .last_items
                    .iter()
                    .find(|i| i.classes.iter().any(|c| c == "sv-square"))
                {
                    self.picker_s = ((x - it.x) / it.w.max(1.0)).clamp(0.0, 1.0);
                    self.picker_v = (1.0 - (y - it.y) / it.h.max(1.0)).clamp(0.0, 1.0);
                    self.sync_picker_fields(true);
                    self.apply_editing_atom();
                    dirty = true;
                }
            }
            Drag::Hue => {
                if let Some(it) = self
                    .last_items
                    .iter()
                    .find(|i| i.classes.iter().any(|c| c == "hue-rail"))
                {
                    self.picker_h = ((x - it.x) / it.w.max(1.0)).clamp(0.0, 1.0);
                    self.sync_picker_fields(true);
                    self.apply_editing_atom();
                    dirty = true;
                }
            }
            Drag::Alpha => {
                if let Some(it) = self
                    .last_items
                    .iter()
                    .find(|i| i.classes.iter().any(|c| c == "alpha-rail"))
                {
                    self.picker_a = ((x - it.x) / it.w.max(1.0)).clamp(0.0, 1.0);
                    self.sync_picker_fields(true);
                    self.apply_editing_atom();
                    dirty = true;
                }
            }
            Drag::None => {}
        }
        let hover = hover_at(&self.last_items, x, y);
        if hover != self.hover {
            self.hover = hover;
            dirty = true;
        }
        dirty
    }

    pub fn right_click(&mut self, x: f32, y: f32) -> bool {
        if self.selected != "context_menu" {
            return false;
        }
        let Some(well) = self
            .last_items
            .iter()
            .find(|i| i.classes.iter().any(|c| c == "context-well"))
        else {
            return false;
        };
        if !point_in_item(well, x, y) {
            self.context = None;
            return true;
        }
        self.context = Some(((x - well.x).max(4.0), (y - well.y).max(4.0)));
        true
    }

    /// `close` / `drag` / `select:<id>` / none.
    pub fn click(&mut self, x: f32, y: f32) -> Click {
        let Some(hit) = hit_test(&self.last_items, x, y) else {
            if self.dismiss_overlays() {
                return Click::Select;
            }
            self.blur();
            return Click::None;
        };
        let action = hit.data_action.clone();
        let id = hit.data_id.clone();
        match action.as_deref() {
            Some("close") => return Click::Close,
            Some("drag") => return Click::Drag,
            Some("theme-toggle") => {
                self.theme_open = !self.theme_open;
                self.select_open = None;
                self.popover_open = false;
                return Click::Select;
            }
            Some("theme-pick") => {
                if let Some(name) = id {
                    self.set_theme(&name);
                }
                self.theme_open = false;
                return Click::Select;
            }
            Some("theme-new") => {
                if self.theme_editable {
                    self.status = "Already a fork.".into();
                } else {
                    self.theme = "Studio".into();
                    self.theme_editable = true;
                    self.status = "Forked Default → Studio. Click a swatch.".into();
                }
                return Click::Select;
            }
            Some("theme-delete") => {
                if self.theme_editable {
                    self.set_theme("Default");
                    self.status = "Deleted Studio.".into();
                } else {
                    self.status = "Can't delete Default.".into();
                }
                return Click::Select;
            }
            Some("split-drag") => {
                self.drag = Drag::Split;
                return Click::Select;
            }
            Some("split-drag-h") => {
                self.drag = Drag::SplitH;
                return Click::Select;
            }
            Some("sv") => {
                self.drag = Drag::Sv;
                if let Some(it) = self
                    .last_items
                    .iter()
                    .find(|i| i.classes.iter().any(|c| c == "sv-square"))
                {
                    self.picker_s = ((x - it.x) / it.w.max(1.0)).clamp(0.0, 1.0);
                    self.picker_v = (1.0 - (y - it.y) / it.h.max(1.0)).clamp(0.0, 1.0);
                    self.sync_picker_fields(true);
                    self.apply_editing_atom();
                }
                return Click::Select;
            }
            Some("hue") => {
                self.drag = Drag::Hue;
                if let Some(it) = self
                    .last_items
                    .iter()
                    .find(|i| i.classes.iter().any(|c| c == "hue-rail"))
                {
                    self.picker_h = ((x - it.x) / it.w.max(1.0)).clamp(0.0, 1.0);
                    self.sync_picker_fields(true);
                    self.apply_editing_atom();
                }
                return Click::Select;
            }
            Some("alpha") => {
                self.drag = Drag::Alpha;
                if let Some(it) = self
                    .last_items
                    .iter()
                    .find(|i| i.classes.iter().any(|c| c == "alpha-rail"))
                {
                    self.picker_a = ((x - it.x) / it.w.max(1.0)).clamp(0.0, 1.0);
                    self.sync_picker_fields(true);
                    self.apply_editing_atom();
                }
                return Click::Select;
            }
            Some("focus") => {
                self.focused = id;
                return Click::Select;
            }
            Some("toggle") => {
                if let Some(id) = id {
                    let v = self.toggles.get(&id).copied().unwrap_or(false);
                    self.toggles.insert(id, !v);
                }
                return Click::Select;
            }
            Some("step-up") | Some("step-down") => {
                let down = action.as_deref() == Some("step-down");
                if let Some(id) = id {
                    self.step(&id, down);
                }
                return Click::Select;
            }
            Some("popover-toggle") => {
                self.popover_open = !self.popover_open;
                self.theme_open = false;
                self.select_open = None;
                return Click::Select;
            }
            Some("prose") => {
                self.drag = Drag::Prose;
                self.prose = Some((hit.uid, x, x));
                return Click::Select;
            }
            Some("select-toggle") => {
                if let Some(id) = id {
                    if self.select_open.as_deref() == Some(id.as_str()) {
                        self.select_open = None;
                    } else {
                        self.select_open = Some(id);
                    }
                }
                self.theme_open = false;
                self.popover_open = false;
                return Click::Select;
            }
            Some("select-pick") => {
                if let Some(id) = id {
                    self.pick_select(&id);
                }
                return Click::Select;
            }
            Some("note") => {
                if let Some(id) = id {
                    self.status = id;
                }
                self.popover_open = false;
                return Click::Select;
            }
            Some("arm-delete") => {
                if self.confirm_armed {
                    self.confirm_armed = false;
                    self.status = "Deleted".into();
                } else {
                    self.confirm_armed = true;
                    self.status = "Click Confirm".into();
                }
                return Click::Select;
            }
            Some("edit-atom") => {
                if !self.theme_editable {
                    self.status = "Default is read-only. New Theme forks it.".into();
                    return Click::Select;
                }
                if let Some(id) = id {
                    self.seed_picker_from_atom(&id);
                    self.editing_atom = Some(id);
                }
                return Click::Select;
            }
            Some("file-select") => {
                if let Some(id) = id {
                    self.file_select(&id);
                }
                return Click::Select;
            }
            Some("file-place") => {
                if let Some(id) = id {
                    self.file_place(&id);
                }
                return Click::Select;
            }
            Some("file-crumb") => {
                if let Some(id) = id {
                    self.file_crumb(&id);
                }
                return Click::Select;
            }
            Some("file-confirm") => {
                self.confirm_file(id.as_deref() != Some("save"));
                return Click::Select;
            }
            Some("file-cancel") => {
                self.status = "Cancelled".into();
                return Click::Select;
            }
            Some("side-pick") => {
                if let Some(id) = id.and_then(|s| s.parse().ok()) {
                    self.side_selected = id;
                }
                return Click::Select;
            }
            Some("side-close") => {
                if let Some(id) = id.and_then(|s| s.parse::<usize>().ok()) {
                    self.side_order.retain(|i| *i != id);
                    self.status = format!("Closed {}", SIDE_ITEMS.get(id).unwrap_or(&"?"));
                }
                return Click::Select;
            }
            Some("side-group") => {
                self.side_group_closed = !self.side_group_closed;
                return Click::Select;
            }
            Some("context-pick") => {
                if let Some(id) = id {
                    self.status = format!("Last action: {id}");
                }
                self.context = None;
                return Click::Select;
            }
            Some("context-well") => {
                self.context = None;
                return Click::Select;
            }
            _ => {}
        }
        if let Some(id) = id.as_deref() {
            if PAGES.iter().any(|p| p.id == id) && self.selected != id {
                self.selected = PAGES.iter().find(|p| p.id == id).unwrap().id;
                self.page_mtime = None;
                self.theme_open = false;
                self.popover_open = false;
                self.select_open = None;
                self.context = None;
                self.editing_atom = None;
                self.confirm_armed = false;
                self.status.clear();
                self.focused = None;
                return Click::Select;
            }
        }
        if self.dismiss_overlays() {
            return Click::Select;
        }
        Click::None
    }

    fn rebuild_items(&mut self) {
        let page = PAGES
            .iter()
            .find(|p| p.id == self.selected)
            .unwrap_or(&PAGES[0]);
        let demo_path = self.assets.join("pages").join(format!("{}.html", page.id));
        let demo = std::fs::read_to_string(&demo_path).unwrap_or_default();
        let mut root = markup::expand(
            &self.html,
            &[],
            crate::WINDOW_TITLE,
            page.heading,
            page.lede,
            &demo,
            &self.theme,
            self.theme_open,
        );
        let mut next = markup::next_uid(&root);
        let sb = Sidebar::new(nav_items(self.selected))
            .nav_id("nav-scroll")
            .build(&mut next);
        markup::replace_slot(&mut root, "sidebar", sb);
        markup::apply_split(&mut root, self.split);
        markup::apply_split_h(&mut root, self.split_h);
        if self.status.is_empty() {
            self.fields.remove("status");
        } else {
            self.fields.insert("status".into(), self.status.clone());
        }
        self.fields.insert(
            "chrome-label".into(),
            SELECT_NAMES[self.select_chrome].into(),
        );
        self.fields
            .insert("form-label".into(), SELECT_NAMES[self.select_form].into());
        self.fields.insert(
            "overview-label".into(),
            SELECT_NAMES[self.select_overview].into(),
        );
        let col = hsv_to_rgb(self.picker_h, self.picker_s, self.picker_v, self.picker_a);
        self.fields.insert(
            "split-v".into(),
            format!(
                "Columns · {:.0}% / {:.0}%",
                self.split * 100.0,
                (1.0 - self.split) * 100.0
            ),
        );
        self.fields.insert(
            "split-h".into(),
            format!(
                "Rows · {:.0}% / {:.0}%",
                self.split_h * 100.0,
                (1.0 - self.split_h) * 100.0
            ),
        );
        self.fill_files(&mut root);
        self.fill_sidebar(&mut root);
        self.fill_atom_panel(&mut root);
        markup::apply_fields(&mut root, &self.fields);
        markup::apply_placeholder(
            &mut root,
            "user",
            field_empty(&self.fields, "user"),
            "naturalethic",
        );
        markup::apply_placeholder(
            &mut root,
            "email",
            field_empty(&self.fields, "email"),
            "joshua@sola.computer",
        );
        markup::apply_placeholder(
            &mut root,
            "display",
            field_empty(&self.fields, "display"),
            "must-not-be-empty",
        );
        markup::apply_focus(&mut root, self.focused.as_deref());
        markup::apply_toggles(&mut root, &self.toggles);
        markup::hide_slot(&mut root, "popover-menu", !self.popover_open);
        markup::hide_slot(
            &mut root,
            "select-chrome",
            self.select_open.as_deref() != Some("chrome"),
        );
        markup::hide_slot(
            &mut root,
            "select-form",
            self.select_open.as_deref() != Some("form"),
        );
        markup::hide_slot(
            &mut root,
            "select-overview",
            self.select_open.as_deref() != Some("overview"),
        );
        markup::hide_slot(&mut root, "context-menu", self.context.is_none());
        markup::hide_slot(&mut root, "atom-picker", self.editing_atom.is_none());
        if let Some((lx, ly)) = self.context {
            markup::walk_mut(&mut root, &mut |el| {
                if el.data_slot.as_deref() == Some("context-menu") {
                    el.style_attr = Some(format!(
                        "position:absolute;left:{lx}px;top:{ly}px;z-index:30;width:200px"
                    ));
                }
            });
        }
        markup::walk_mut(&mut root, &mut |el| {
            if el.classes.iter().any(|c| c == "picker-preview") {
                el.style_attr = Some(format!("background:{}", format_hex(col)));
            }
            if el.classes.iter().any(|c| c == "alpha-rail") {
                let rgb = hsv_to_rgb(self.picker_h, self.picker_s, self.picker_v, 1.0);
                el.style_attr = Some(format!("background:{}", format_hex(rgb)));
            }
            if el.data_id.as_deref() == Some("chrome-enamel") {
                el.data_id = Some(SELECT_SEEDS[self.select_chrome].into());
            }
            if el.data_id.as_deref() == Some("form-enamel") {
                el.data_id = Some(SELECT_SEEDS[self.select_form].into());
            }
            if el.data_id.as_deref() == Some("header-enamel") {
                el.data_id = Some(format!("seed-{}", self.theme.to_lowercase()));
            }
            if el.data_id.as_deref() == Some("overview-enamel") {
                el.data_id = Some(SELECT_SEEDS[self.select_overview].into());
            }
        });
        markup::apply_enamel(&mut root);
        if let Some(open) = self.select_open.clone() {
            markup::walk_mut(&mut root, &mut |el| {
                if el.data_action.as_deref() == Some("select-toggle")
                    && el.data_id.as_deref() == Some(open.as_str())
                {
                    markup::add_class(el, "is-open");
                }
            });
        }
        if self.popover_open {
            markup::walk_mut(&mut root, &mut |el| {
                if el.data_action.as_deref() == Some("popover-toggle") {
                    markup::add_class(el, "is-open");
                }
            });
        }
        self.apply_select_checks(&mut root);
        self.apply_atom_swatches(&mut root);
        if self.confirm_armed {
            markup::walk_mut(&mut root, &mut |el| {
                if el.data_action.as_deref() == Some("arm-delete") {
                    el.text = "Confirm".into();
                    markup::remove_class(el, "btn-danger-outline");
                    markup::add_class(el, "btn-danger");
                }
            });
        }
        self.last_items = layout_tree(
            &root,
            &self.sheet,
            self.hover,
            self.css_w,
            self.css_h,
            &mut self.fonts,
            &self.scrolls,
        );
    }

    fn restyle(&mut self) {
        for (k, v) in theme_vars(&self.theme) {
            if !self.atom_overrides.contains_key(*k) {
                self.sheet.set_var(k, *v);
            }
        }
        for (k, v) in &self.atom_overrides {
            self.sheet.set_var(k, v.clone());
        }
    }

    fn set_theme(&mut self, name: &str) {
        self.theme = name.to_string();
        self.theme_editable = name == "Studio";
        if !self.theme_editable {
            self.atom_overrides.clear();
            self.editing_atom = None;
        }
        self.restyle();
    }

    fn picker_color(&self) -> crate::css::Rgba {
        hsv_to_rgb(self.picker_h, self.picker_s, self.picker_v, self.picker_a)
    }

    fn sync_picker_fields(&mut self, force: bool) {
        let c = self.picker_color();
        let (hh, ss, ll) = rgb_to_hsl(c);
        let focused = if force { None } else { self.focused.clone() };
        let mut put = |k: &str, v: String| {
            if focused.as_deref() != Some(k) {
                self.fields.insert(k.into(), v);
            }
        };
        put("hex", format_hex(c));
        put("rgb_r", c.r.to_string());
        put("rgb_g", c.g.to_string());
        put("rgb_b", c.b.to_string());
        put("hsl_h", format!("{:.0}", hh * 360.0));
        put("hsl_s", format!("{:.0}", ss * 100.0));
        put("hsl_l", format!("{:.0}", ll * 100.0));
    }

    fn adopt_color(&mut self, c: crate::css::Rgba) {
        let (h, s, v) = rgb_to_hsv(c);
        self.picker_h = h;
        self.picker_s = s;
        self.picker_v = v;
        self.picker_a = c.a as f32 / 255.0;
        self.sync_picker_fields(false);
        self.apply_editing_atom();
    }

    fn after_field_edit(&mut self, id: &str) {
        match id {
            "hex" => {
                if let Some(c) = self.fields.get("hex").and_then(|s| parse_hex(s)) {
                    self.adopt_color(c);
                }
            }
            "rgb_r" | "rgb_g" | "rgb_b" => {
                let r = parse_u8(self.fields.get("rgb_r").map(|s| s.as_str()).unwrap_or(""));
                let g = parse_u8(self.fields.get("rgb_g").map(|s| s.as_str()).unwrap_or(""));
                let b = parse_u8(self.fields.get("rgb_b").map(|s| s.as_str()).unwrap_or(""));
                if let (Some(r), Some(g), Some(b)) = (r, g, b) {
                    self.adopt_color(crate::css::Rgba {
                        r,
                        g,
                        b,
                        a: (self.picker_a * 255.0) as u8,
                    });
                }
            }
            "hsl_h" | "hsl_s" | "hsl_l" => {
                let h = parse_f(self.fields.get("hsl_h").map(|s| s.as_str()).unwrap_or(""));
                let s = parse_f(self.fields.get("hsl_s").map(|s| s.as_str()).unwrap_or(""));
                let l = parse_f(self.fields.get("hsl_l").map(|s| s.as_str()).unwrap_or(""));
                if let (Some(h), Some(s), Some(l)) = (h, s, l) {
                    let c = crate::palette::hsl_to_rgb(
                        (h / 360.0).clamp(0.0, 1.0),
                        (s / 100.0).clamp(0.0, 1.0),
                        (l / 100.0).clamp(0.0, 1.0),
                        self.picker_a,
                    );
                    self.adopt_color(c);
                }
            }
            "radius" | "opacity" => self.clamp_numbers(),
            "open-name" => {
                self.open.name = self.fields.get("open-name").cloned().unwrap_or_default()
            }
            "save-name" => {
                self.save.name = self.fields.get("save-name").cloned().unwrap_or_default()
            }
            _ => {}
        }
    }

    fn clamp_numbers(&mut self) {
        if let Some(n) = self
            .fields
            .get("radius")
            .and_then(|s| s.parse::<i32>().ok())
        {
            self.fields
                .insert("radius".into(), n.clamp(0, 32).to_string());
        }
        if let Some(n) = self
            .fields
            .get("opacity")
            .and_then(|s| s.parse::<i32>().ok())
        {
            let n = (n / 5 * 5).clamp(0, 100);
            self.fields.insert("opacity".into(), n.to_string());
        }
    }

    fn step(&mut self, id: &str, down: bool) {
        let (min, max, step) = match id {
            "opacity" => (0, 100, 5),
            _ => (0, 32, 1),
        };
        let n: i32 = self
            .fields
            .get(id)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let n = if down { n - step } else { n + step };
        self.fields
            .insert(id.to_string(), n.clamp(min, max).to_string());
    }

    fn apply_editing_atom(&mut self) {
        let Some(atom) = self.editing_atom.clone() else {
            return;
        };
        if !self.theme_editable {
            return;
        }
        if let Some((_, _, var)) = ATOMS.iter().find(|(id, _, _)| *id == atom) {
            self.atom_overrides
                .insert((*var).into(), format_hex(self.picker_color()));
            self.restyle();
        }
    }

    fn seed_picker_from_atom(&mut self, atom: &str) {
        let Some((_, _, var)) = ATOMS.iter().find(|(id, _, _)| *id == atom) else {
            return;
        };
        let hex = self
            .atom_overrides
            .get(*var)
            .cloned()
            .or_else(|| {
                theme_vars(&self.theme)
                    .iter()
                    .find(|(k, _)| *k == *var)
                    .map(|(_, v)| (*v).to_string())
            })
            .unwrap_or_else(|| "#3dd6f5".into());
        if let Some(c) = parse_hex(&hex) {
            self.adopt_color(c);
        }
    }

    fn pick_select(&mut self, id: &str) {
        let Some((which, n)) = id.split_once(':') else {
            return;
        };
        let Ok(n) = n.parse::<usize>() else {
            return;
        };
        if n >= SELECT_NAMES.len() {
            return;
        }
        match which {
            "chrome" => self.select_chrome = n,
            "form" => self.select_form = n,
            "overview" => self.select_overview = n,
            _ => {}
        }
        self.select_open = None;
    }

    fn apply_select_checks(&self, root: &mut crate::dom::Elem) {
        let chrome = self.select_chrome;
        let form = self.select_form;
        let overview = self.select_overview;
        markup::walk_mut(root, &mut |el| {
            let Some(id) = el.data_id.clone() else {
                return;
            };
            if el.data_action.as_deref() != Some("select-pick") {
                return;
            }
            let selected = match id.split_once(':') {
                Some(("chrome", n)) => n.parse::<usize>().ok() == Some(chrome),
                Some(("form", n)) => n.parse::<usize>().ok() == Some(form),
                Some(("overview", n)) => n.parse::<usize>().ok() == Some(overview),
                _ => false,
            };
            if selected {
                markup::add_class(el, "is-active");
            }
        });
        hide_inactive_checks(root);
    }

    fn fill_atom_panel(&self, root: &mut crate::dom::Elem) {
        let tiles: Vec<(String, String, String)> = page_atoms(self.selected)
            .iter()
            .filter_map(|id| {
                ATOMS
                    .iter()
                    .find(|(a, _, _)| a == id)
                    .map(|(_, name, var)| {
                        let hex = self
                            .atom_overrides
                            .get(*var)
                            .cloned()
                            .or_else(|| {
                                theme_vars(&self.theme)
                                    .iter()
                                    .find(|(k, _)| *k == *var)
                                    .map(|(_, v)| (*v).to_string())
                            })
                            .unwrap_or_else(|| "#888888".into());
                        ((*id).to_string(), (*name).to_string(), hex)
                    })
            })
            .collect();
        markup::fill_atoms(root, &tiles);
    }

    fn apply_atom_swatches(&self, root: &mut crate::dom::Elem) {
        markup::walk_mut(root, &mut |el| {
            if el.data_action.as_deref() != Some("edit-atom") {
                return;
            }
            let Some(id) = el.data_id.as_deref() else {
                return;
            };
            let Some((_, _, var)) = ATOMS.iter().find(|(a, _, _)| *a == id) else {
                return;
            };
            let hex = self
                .atom_overrides
                .get(*var)
                .cloned()
                .or_else(|| {
                    theme_vars(&self.theme)
                        .iter()
                        .find(|(k, _)| *k == *var)
                        .map(|(_, v)| (*v).to_string())
                })
                .unwrap_or_else(|| "#888888".into());
            el.style_attr = Some(format!("background:{hex}"));
        });
    }

    fn fill_files(&mut self, root: &mut crate::dom::Elem) {
        if self.selected != "file_picker" {
            return;
        }
        self.fields
            .insert("open-name".into(), self.open.name.clone());
        self.fields
            .insert("save-name".into(), self.save.name.clone());
        fill_file_panel(root, "open", &self.open);
        fill_file_panel(root, "save", &self.save);
    }

    fn fill_sidebar(&self, root: &mut crate::dom::Elem) {
        if self.selected != "sidebar" {
            return;
        }
        let mut next = markup_max_uid(root) + 1;
        let mut kids = Vec::new();
        kids.push(markup::node(
            &mut next,
            &["side-head"],
            None,
            None,
            "MAILBOXES",
        ));
        for &idx in &self.side_order {
            let label = SIDE_ITEMS[idx];
            if matches!(label, "Sent" | "Archive") {
                continue;
            }
            kids.push(side_row(&mut next, idx, label, self.side_selected == idx));
        }
        kids.push(markup::node(
            &mut next,
            &["side-head"],
            Some("side-group"),
            None,
            if self.side_group_closed {
                "WORK  ▸"
            } else {
                "WORK  ▾"
            },
        ));
        if !self.side_group_closed {
            for &idx in &self.side_order {
                let label = SIDE_ITEMS[idx];
                if matches!(label, "Sent" | "Archive") {
                    kids.push(side_row(&mut next, idx, label, self.side_selected == idx));
                }
            }
        }
        for &idx in &self.side_order {
            let label = SIDE_ITEMS[idx];
            if label == "Spam" {
                kids.push(side_row(&mut next, idx, label, self.side_selected == idx));
            }
        }
        markup::fill_slot(root, "side-rows", kids);
    }

    fn file_select(&mut self, id: &str) {
        let Some((panel, idx)) = parse_panel_idx(id) else {
            return;
        };
        let files = if panel == "open" {
            &mut self.open
        } else {
            &mut self.save
        };
        let Some(ent) = files.entries.get(idx).cloned() else {
            return;
        };
        if ent.is_dir {
            files.cd(ent.path);
        } else {
            files.selected = Some(ent.path.clone());
            files.name = ent.name;
        }
    }

    fn file_place(&mut self, id: &str) {
        let Some((panel, idx)) = parse_panel_idx(id) else {
            return;
        };
        let files = if panel == "open" {
            &mut self.open
        } else {
            &mut self.save
        };
        if let Some((_, path)) = files.places.get(idx).cloned() {
            files.cd(path);
        }
    }

    fn file_crumb(&mut self, id: &str) {
        let Some((panel, idx)) = parse_panel_idx(id) else {
            return;
        };
        let files = if panel == "open" {
            &mut self.open
        } else {
            &mut self.save
        };
        if let Some((_, path)) = files.crumbs().get(idx).cloned() {
            files.cd(path);
        }
    }

    fn confirm_file(&mut self, open: bool) {
        let files = if open { &mut self.open } else { &mut self.save };
        if let Some(p) = files.selected.clone() {
            if p.is_dir() {
                files.cd(p);
                return;
            }
            self.status = format!("Picked {}", p.display());
            return;
        }
        if !files.name.trim().is_empty() {
            let p = files.cwd.join(files.name.trim());
            self.status = format!("Picked {}", p.display());
            return;
        }
        self.status = "Name the file".into();
    }
}

fn hide_inactive_checks(root: &mut crate::dom::Elem) {
    let mut hide = Vec::new();
    collect_menu_check_hide(root, &mut hide);
    hide_uids(root, &hide);
}

fn collect_menu_check_hide(el: &crate::dom::Elem, out: &mut Vec<u32>) {
    if el.data_action.as_deref() == Some("select-pick") {
        let active = el.classes.iter().any(|c| c == "is-active");
        if !active {
            collect_check_icons(el, out);
        }
    }
    for c in &el.children {
        collect_menu_check_hide(c, out);
    }
}

fn collect_check_icons(el: &crate::dom::Elem, out: &mut Vec<u32>) {
    if el.data_kind.as_deref() == Some("icon") && el.data_id.as_deref() == Some("lucide/check") {
        out.push(el.uid);
    }
    for c in &el.children {
        collect_check_icons(c, out);
    }
}

fn hide_uids(el: &mut crate::dom::Elem, uids: &[u32]) {
    if uids.contains(&el.uid) {
        markup::add_class(el, "is-hidden");
    }
    for c in &mut el.children {
        hide_uids(c, uids);
    }
}

fn markup_max_uid(el: &crate::dom::Elem) -> u32 {
    el.children
        .iter()
        .map(markup_max_uid)
        .max()
        .unwrap_or(el.uid)
        .max(el.uid)
}

fn side_row(next: &mut u32, idx: usize, label: &str, active: bool) -> crate::dom::Elem {
    let mut classes: Vec<&str> = vec!["row", "side-row"];
    if active {
        classes.push("is-active");
    }
    let mut row = markup::node(
        next,
        &classes,
        Some("side-pick"),
        Some(&idx.to_string()),
        "",
    );
    let mut etch = markup::node(next, &["etch"], None, None, "");
    let mut lab = markup::node(next, &["label"], None, None, label);
    lab.data_bind = None;
    let x = markup::node(
        next,
        &["side-x"],
        Some("side-close"),
        Some(&idx.to_string()),
        "×",
    );
    etch.children.push(lab);
    etch.children.push(x);
    row.children.push(etch);
    row
}

fn fill_file_panel(root: &mut crate::dom::Elem, panel: &str, files: &Files) {
    let mut next = markup_max_uid(root) + 1;
    let crumbs = files.crumbs();
    let mut crumb_els = Vec::new();
    for (i, (label, _)) in crumbs.iter().enumerate() {
        if i > 0 {
            crumb_els.push(markup::node(&mut next, &["crumb-sep"], None, None, "›"));
        }
        let id = format!("{panel}:{i}");
        crumb_els.push(markup::node(
            &mut next,
            &["crumb"],
            Some("file-crumb"),
            Some(&id),
            label,
        ));
    }
    markup::fill_slot(root, &format!("{panel}-crumbs"), crumb_els);

    let mut place_els = Vec::new();
    place_els.push(markup::node(
        &mut next,
        &["side-head"],
        None,
        None,
        "PLACES",
    ));
    for (i, (label, path)) in files.places.iter().enumerate() {
        let id = format!("{panel}:{i}");
        let mut classes = vec!["file-row"];
        if files.cwd.starts_with(path)
            && files
                .places
                .iter()
                .filter(|(_, p)| files.cwd.starts_with(p))
                .map(|(_, p)| p.components().count())
                .max()
                == Some(path.components().count())
        {
            classes.push("is-active");
        }
        place_els.push(markup::node(
            &mut next,
            &classes,
            Some("file-place"),
            Some(&id),
            label,
        ));
    }
    markup::fill_slot(root, &format!("{panel}-places"), place_els);

    let mut rows = Vec::new();
    if files.entries.is_empty() {
        rows.push(markup::node(
            &mut next,
            &["t-caption"],
            None,
            None,
            "This folder is empty",
        ));
    } else {
        for (i, ent) in files.entries.iter().enumerate() {
            let id = format!("{panel}:{i}");
            let mut classes = vec!["file-row"];
            if files.selected.as_ref() == Some(&ent.path) {
                classes.push("is-active");
            }
            let label = if ent.is_dir {
                format!("{}/", ent.name)
            } else {
                ent.name.clone()
            };
            rows.push(markup::node(
                &mut next,
                &classes,
                Some("file-select"),
                Some(&id),
                &label,
            ));
        }
    }
    markup::fill_slot(root, &format!("{panel}-files"), rows);
}

fn parse_panel_idx(id: &str) -> Option<(&str, usize)> {
    let (panel, n) = id.split_once(':')?;
    Some((panel, n.parse().ok()?))
}

fn field_empty(fields: &HashMap<String, String>, id: &str) -> bool {
    fields.get(id).map(|s| s.is_empty()).unwrap_or(true)
}

fn parse_u8(s: &str) -> Option<u8> {
    s.parse().ok()
}

fn parse_f(s: &str) -> Option<f32> {
    s.parse().ok()
}

impl Files {
    fn open() -> Self {
        Self::new(vec![
            "png".into(),
            "jpg".into(),
            "jpeg".into(),
            "gif".into(),
            "webp".into(),
            "bmp".into(),
        ])
    }

    fn save() -> Self {
        let mut f = Self::new(vec![
            "png".into(),
            "jpg".into(),
            "jpeg".into(),
            "webp".into(),
        ]);
        f.name = "untitled.png".into();
        f
    }

    fn new(filter: Vec<String>) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut places = Vec::new();
        if let Some(h) = home.as_ref() {
            places.push(("Home".into(), h.clone()));
            for (label, name) in [
                ("Desktop", "Desktop"),
                ("Documents", "Documents"),
                ("Downloads", "Downloads"),
                ("Pictures", "Pictures"),
            ] {
                let p = h.join(name);
                if p.is_dir() {
                    places.push((label.into(), p));
                }
            }
        }
        let cwd = places
            .iter()
            .find(|(n, _)| n == "Pictures")
            .or_else(|| places.first())
            .map(|(_, p)| p.clone())
            .unwrap_or_else(|| PathBuf::from("/"));
        let mut me = Self {
            cwd,
            selected: None,
            entries: Vec::new(),
            name: String::new(),
            places,
            filter,
        };
        me.reload();
        me
    }

    fn cd(&mut self, path: PathBuf) {
        self.cwd = path;
        self.selected = None;
        self.reload();
    }

    fn reload(&mut self) {
        self.entries = list_dir(&self.cwd, &self.filter);
    }

    fn crumbs(&self) -> Vec<(String, PathBuf)> {
        let home = self
            .places
            .iter()
            .find(|(n, _)| n == "Home")
            .map(|(_, p)| p);
        if let Some(home) = home {
            if self.cwd.starts_with(home) {
                let mut out = vec![("Home".into(), home.clone())];
                if let Ok(rel) = self.cwd.strip_prefix(home) {
                    let mut acc = home.clone();
                    for c in rel.components() {
                        acc.push(c);
                        out.push((c.as_os_str().to_string_lossy().into_owned(), acc.clone()));
                    }
                }
                return out;
            }
        }
        let mut out = Vec::new();
        let mut acc = PathBuf::new();
        for c in self.cwd.components() {
            acc.push(c);
            let label = if acc.as_os_str() == "/" {
                "/".into()
            } else {
                c.as_os_str().to_string_lossy().into_owned()
            };
            out.push((label, acc.clone()));
        }
        out
    }
}

fn list_dir(path: &Path, filter: &[String]) -> Vec<FileEnt> {
    let rd = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = p.is_dir();
        if !is_dir {
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !filter.is_empty() && !filter.iter().any(|f| f == &ext) {
                continue;
            }
        }
        out.push(FileEnt {
            name,
            path: p,
            is_dir,
        });
        if out.len() >= 200 {
            break;
        }
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Click {
    None,
    Close,
    Drag,
    Select,
}

fn nav_items(selected: &str) -> Vec<SidebarItem> {
    let mut rows = Vec::new();
    let mut last_section = "";
    for page in PAGES {
        if page.section != last_section {
            last_section = page.section;
            rows.push(SidebarItem::header(page.section.to_uppercase()));
        }
        rows.push(SidebarItem::new(page.id, page.label).active(page.id == selected));
    }
    rows
}

fn load_sheet(path: &std::path::Path) -> (Sheet, Option<std::time::SystemTime>) {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
            (parse_sheet(&s), mtime)
        }
        Err(_) => (parse_sheet(CSS), None),
    }
}

fn load_html(path: &std::path::Path) -> (String, Option<std::time::SystemTime>) {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
            (s, mtime)
        }
        Err(_) => (HTML.to_string(), None),
    }
}

fn reload_if_newer(
    path: &std::path::Path,
    prev: Option<std::time::SystemTime>,
) -> Option<(String, Option<std::time::SystemTime>)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok();
    if mtime == prev {
        return None;
    }
    let s = std::fs::read_to_string(path).ok()?;
    Some((s, mtime))
}

fn rgba(c: crate::css::Rgba) -> [f32; 4] {
    [
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        c.a as f32 / 255.0,
    ]
}

fn mix_white_u8(c: crate::css::Rgba, amount: f32) -> crate::css::Rgba {
    let t = amount.clamp(0.0, 1.0);
    let k = 1.0 - t;
    crate::css::Rgba {
        r: (c.r as f32 * k + 255.0 * t).round().clamp(0.0, 255.0) as u8,
        g: (c.g as f32 * k + 255.0 * t).round().clamp(0.0, 255.0) as u8,
        b: (c.b as f32 * k + 255.0 * t).round().clamp(0.0, 255.0) as u8,
        a: c.a,
    }
}

pub(crate) fn chrome_quads(
    items: &[PaintItem],
    scale: f32,
    sw: u32,
    sh: u32,
    hue: f32,
    picker: crate::css::Rgba,
) -> Vec<Quad> {
    let s = scale.max(0.01);
    let screen = [0.0, 0.0, sw as f32, sh as f32];
    let mut out = Vec::new();
    for item in items {
        if item.hidden || item.w < 0.5 || item.h < 0.5 {
            continue;
        }
        let clip = item
            .clip
            .map(|(x, y, w, h)| [x * s, y * s, w * s, h * s])
            .unwrap_or(screen);
        let xywh = [item.x * s, item.y * s, item.w * s, item.h * s];
        let radius = item.radius * s;
        let spec = if item.classes.iter().any(|c| c == "sv-square") {
            Some(3.0)
        } else if item.classes.iter().any(|c| c == "hue-rail") {
            Some(4.0)
        } else if item.classes.iter().any(|c| c == "alpha-rail") {
            Some(5.0)
        } else {
            None
        };
        let sides = item.border;
        let all = sides[0]
            .filter(|a| sides[1] == Some(*a) && sides[2] == Some(*a) && sides[3] == Some(*a));
        let hair = all.map(|(bw, _)| (bw * s).max(1.0)).unwrap_or(0.0);
        if let Some(bg) = item.bg {
            let c2 = item.bg2.unwrap_or(bg);
            let mode = spec.unwrap_or(item.gradient as f32);
            let color = if spec == Some(5.0) { picker } else { bg };
            out.push(Quad {
                xywh,
                color: rgba(color),
                clip,
                extra: [radius, hair, mode, hue],
                color2: rgba(c2),
            });
        } else if let Some(mode) = spec {
            out.push(Quad {
                xywh,
                color: rgba(picker),
                clip,
                extra: [radius, 0.0, mode, hue],
                color2: rgba(picker),
            });
        }
        if all.is_none() {
            let [x, y, w, h] = xywh;
            let base = item.bg.unwrap_or(crate::css::Rgba::rgb(0x15, 0x19, 0x22));
            let stroke = |box_: [f32; 4]| Quad {
                xywh: box_,
                color: rgba(mix_white_u8(base, 0.14)),
                clip,
                extra: [0.0, 0.0, 0.0, 0.0],
                color2: rgba(mix_white_u8(base, 0.14)),
            };
            if let Some((bw, _)) = sides[0] {
                let t = (bw * s).max(1.0);
                out.push(stroke([x, y, w, t]));
            }
            if let Some((bw, _)) = sides[1] {
                let t = (bw * s).max(1.0);
                out.push(stroke([x + w - t, y, t, h]));
            }
            if let Some((bw, _)) = sides[2] {
                let t = (bw * s).max(1.0);
                out.push(stroke([x, y + h - t, w, t]));
            }
            if let Some((bw, _)) = sides[3] {
                let t = (bw * s).max(1.0);
                out.push(stroke([x, y, t, h]));
            }
        }
    }
    out
}

impl crate::host::Surface for App {
    fn set_view(&mut self, w: f32, h: f32, scale: f32) {
        self.css_w = w;
        self.css_h = h;
        self.scale = scale;
    }
    fn tick(&mut self, dt: f32) {
        App::tick(self, dt);
    }
    fn time(&self) -> f32 {
        App::time(self)
    }
    fn needs_frame(&self) -> bool {
        App::needs_frame(self)
    }
    fn has_overlay(&self) -> bool {
        App::has_overlay(self)
    }
    fn has_focus(&self) -> bool {
        App::has_focus(self)
    }
    fn blur(&mut self) {
        App::blur(self);
    }
    fn dismiss_overlays(&mut self) -> bool {
        App::dismiss_overlays(self)
    }
    fn type_text(&mut self, s: &str) {
        App::type_text(self, s);
    }
    fn backspace(&mut self) {
        App::backspace(self);
    }
    fn tab(&mut self, back: bool) {
        App::tab(self, back);
    }
    fn arrow(&mut self, up: bool) {
        App::arrow(self, up);
    }
    fn enter(&mut self) {
        App::enter(self);
    }
    fn mouse_up(&mut self) {
        App::mouse_up(self);
    }
    fn buffer_size(&self) -> (u32, u32) {
        App::buffer_size(self)
    }
    fn reload_if_changed(&mut self) -> bool {
        App::reload_if_changed(self)
    }
    fn live_layers(&mut self) -> (Vec<Quad>, Option<Vec<u32>>) {
        App::live_layers(self)
    }
    fn wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        App::wheel(self, x, y, dy)
    }
    fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        App::mouse_move(self, x, y)
    }
    fn right_click(&mut self, x: f32, y: f32) -> bool {
        App::right_click(self, x, y)
    }
    fn click(&mut self, x: f32, y: f32) -> Click {
        App::click(self, x, y)
    }
    fn poll(&mut self) -> bool {
        false
    }
    fn cursor_at(&self, x: f32, y: f32) -> crate::host::CursorKind {
        let hit = self
            .last_items
            .iter()
            .rev()
            .find(|i| crate::layout::point_in_item(i, x, y));
        let Some(hit) = hit else {
            return crate::host::CursorKind::Default;
        };
        if hit.classes.iter().any(|c| c == "input") {
            crate::host::CursorKind::Text
        } else if hit.classes.iter().any(|c| c == "btn") {
            crate::host::CursorKind::Pointer
        } else {
            crate::host::CursorKind::Default
        }
    }
}
