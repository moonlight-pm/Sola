//! Storybook store → HTML/CSS → layout → GPU layers. Rust events, no JS.

use std::collections::HashMap;

use crate::css::{Sheet, parse_sheet};
use crate::gpu::Quad;
use crate::icons::Icons;
use crate::layout::{PaintItem, hit_test, hover_at, layout_tree};
use crate::markup::{self, RowSpec};
use crate::paint::{Fonts, PaintPass, paint_glyphs};

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
        lede: "Fonts and seed atoms. Edits ride the bus to every kit app — not in this spike.",
    },
    Page {
        id: "shell",
        label: "Shell",
        section: "System",
        heading: "Shell",
        lede: "Shell chrome tokens. Colors carry alpha. The running shell restyles as you edit.",
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
        lede: "Seed atoms. Spectrum editing stays on the iced kit for now.",
    },
    Page {
        id: "file_picker",
        label: "FilePicker",
        section: "Components",
        heading: "FilePicker",
        lede: "Open/Save panel on a desk. Static composition in this spike.",
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
        lede: "Kit primitive.",
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
        lede: "List etch: muted idle, reserved lip so selected text does not shift, inset active.",
    },
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
    popover_open: bool,
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
    picker_h: f32,
    picker_s: f32,
    picker_v: f32,
    drag: Drag,
    focused: Option<String>,
    fields: HashMap<String, String>,
    prose: Option<(u32, f32, f32)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Drag {
    None,
    Split,
    Prose,
    Sv,
    Hue,
}

impl App {
    pub fn new(css_w: f32, css_h: f32, scale: f32) -> Self {
        let assets = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let css_path = assets.join("kit.css");
        let html_path = assets.join("kit.html");
        let (sheet, css_mtime) = load_sheet(&css_path);
        let (html, html_mtime) = load_html(&html_path);
        Self {
            css_w,
            css_h,
            scale: scale.max(0.01),
            sheet,
            fonts: Fonts::new(),
            hover: None,
            selected: PAGES[0].id,
            theme: "Default".into(),
            theme_open: false,
            popover_open: false,
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
            picker_h: 0.52,
            picker_s: 0.55,
            picker_v: 0.96,
            drag: Drag::None,
            focused: None,
            fields: HashMap::from([
                ("user".into(), "naturalethic".into()),
                ("display".into(), "Joshua".into()),
                ("radius".into(), "8".into()),
                ("opacity".into(), "80".into()),
            ]),
            prose: None,
        }
    }

    pub fn tick(&mut self, dt: f32) {
        self.time += dt;
    }

    pub fn time(&self) -> f32 {
        self.time
    }

    pub fn needs_frame(&self) -> bool {
        self.selected == "sidebar" || self.selected == "icon" || self.focused.is_some()
    }

    pub fn has_focus(&self) -> bool {
        self.focused.is_some()
    }

    pub fn blur(&mut self) {
        self.focused = None;
    }

    pub fn type_text(&mut self, s: &str) {
        let Some(id) = self.focused.clone() else {
            return;
        };
        self.fields.entry(id).or_default().push_str(s);
    }

    pub fn backspace(&mut self) {
        let Some(id) = self.focused.clone() else {
            return;
        };
        self.fields.entry(id).or_default().pop();
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

    pub fn live_layers(&mut self) -> (Vec<Quad>, Vec<u32>) {
        self.reload_if_changed();
        self.rebuild_items();
        let (bw, bh) = self.buffer_size();
        let quads = chrome_quads(&self.last_items, self.scale, bw, bh, self.picker_h);
        let sel = self.prose.map(|(uid, a, b)| (uid, a, b));
        let caret = self.caret_px();
        let pix = paint_glyphs(
            &self.last_items,
            &mut self.fonts,
            self.css_w,
            self.css_h,
            self.scale,
            &mut PaintPass {
                time: self.time,
                sel,
                caret,
                icons: &mut self.icons,
            },
        );
        (quads, pix)
    }

    fn caret_px(&self) -> Option<(u32, f32)> {
        let id = self.focused.as_deref()?;
        if (self.time * 2.0).fract() > 0.5 {
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
        let family = run
            .map(|r| r.family.as_str())
            .unwrap_or("SF Pro Text");
        // measure_width needs &mut fonts — skip precise caret if we can't.
        let _ = (text, size, weight, family);
        Some((item.uid, item.x + item.pad[3] + text.len() as f32 * 7.0))
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        let mut dirty = false;
        match self.drag {
            Drag::Split => {
                if let Some(box_) = self
                    .last_items
                    .iter()
                    .find(|i| i.classes.iter().any(|c| c == "split"))
                {
                    let t = ((x - box_.x) / box_.w.max(1.0)).clamp(0.2, 0.8);
                    if (t - self.split).abs() > 0.002 {
                        self.split = t;
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

    /// `close` / `drag` / `select:<id>` / none.
    pub fn click(&mut self, x: f32, y: f32) -> Click {
        let Some(hit) = hit_test(&self.last_items, x, y) else {
            if self.theme_open {
                self.theme_open = false;
                return Click::Select;
            }
            return Click::None;
        };
        let action = hit.data_action.clone();
        let id = hit.data_id.clone();
        match action.as_deref() {
            Some("close") => return Click::Close,
            Some("drag") => return Click::Drag,
            Some("theme-toggle") => {
                self.theme_open = !self.theme_open;
                return Click::Select;
            }
            Some("theme-pick") => {
                if let Some(name) = id {
                    self.theme = name;
                }
                self.theme_open = false;
                return Click::Select;
            }
            Some("split-drag") => {
                self.drag = Drag::Split;
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
                    let n: i32 = self.fields.get(&id).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let n = if down { n - 1 } else { n + 1 };
                    self.fields.insert(id, n.to_string());
                }
                return Click::Select;
            }
            Some("popover-toggle") => {
                self.popover_open = !self.popover_open;
                return Click::Select;
            }
            Some("prose") => {
                self.drag = Drag::Prose;
                self.prose = Some((hit.uid, x, x));
                return Click::Select;
            }
            _ => {}
        }
        if let Some(id) = id.as_deref() {
            if PAGES.iter().any(|p| p.id == id) && self.selected != id {
                self.selected = PAGES.iter().find(|p| p.id == id).unwrap().id;
                self.page_mtime = None;
                self.theme_open = false;
                return Click::Select;
            }
        }
        if self.theme_open {
            self.theme_open = false;
            return Click::Select;
        }
        Click::None
    }

    fn rebuild_items(&mut self) {
        let page = PAGES
            .iter()
            .find(|p| p.id == self.selected)
            .unwrap_or(&PAGES[0]);
        let rows = nav_rows(self.selected);
        let demo_path = self.assets.join("pages").join(format!("{}.html", page.id));
        let demo = std::fs::read_to_string(&demo_path).unwrap_or_default();
        let mut root = markup::expand(
            &self.html,
            &rows,
            crate::WINDOW_TITLE,
            page.heading,
            page.lede,
            &demo,
            &self.theme,
            self.theme_open,
        );
        markup::apply_split(&mut root, self.split);
        markup::apply_fields(&mut root, &self.fields);
        markup::apply_focus(&mut root, self.focused.as_deref());
        markup::apply_toggles(&mut root, &self.toggles);
        markup::hide_slot(&mut root, "popover-menu", !self.popover_open);
        self.last_items = layout_tree(
            &root,
            &self.sheet,
            self.hover,
            self.css_w,
            self.css_h,
            &mut self.fonts,
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Click {
    None,
    Close,
    Drag,
    Select,
}

fn nav_rows(selected: &str) -> Vec<RowSpec> {
    let mut rows = Vec::new();
    let mut last_section = "";
    for page in PAGES {
        if page.section != last_section {
            last_section = page.section;
            rows.push(RowSpec {
                id: format!("section-{}", page.section.to_lowercase()),
                kind: "header".into(),
                label: page.section.to_uppercase(),
                classes: vec!["is-header".into()],
            });
        }
        let mut classes = Vec::new();
        if page.id == selected {
            classes.push("is-active".into());
        }
        rows.push(RowSpec {
            id: page.id.into(),
            kind: "item".into(),
            label: page.label.into(),
            classes,
        });
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

fn chrome_quads(items: &[PaintItem], scale: f32, sw: u32, sh: u32, hue: f32) -> Vec<Quad> {
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
        if let Some(bg) = item.bg {
            let c2 = item.bg2.unwrap_or(bg);
            let mode = spec.unwrap_or(item.gradient as f32);
            out.push(Quad {
                xywh,
                color: rgba(bg),
                clip,
                extra: [radius, 0.0, mode, hue],
                color2: rgba(c2),
            });
        } else if let Some(mode) = spec {
            let bg = crate::css::Rgba::rgb(0x3d, 0xd6, 0xf5);
            out.push(Quad {
                xywh,
                color: rgba(bg),
                clip,
                extra: [radius, 0.0, mode, hue],
                color2: rgba(bg),
            });
        }
        let sides = item.border;
        let all = sides[0]
            .filter(|a| sides[1] == Some(*a) && sides[2] == Some(*a) && sides[3] == Some(*a));
        if let Some((bw, col)) = all {
            let t = (bw * s).max(1.0);
            if radius > 0.5 {
                out.push(Quad {
                    xywh,
                    color: rgba(col),
                    clip,
                    extra: [radius, t, 0.0, 0.0],
                    color2: rgba(col),
                });
            } else {
                let [x, y, w, h] = xywh;
                let stroke = |box_: [f32; 4], c: crate::css::Rgba| Quad {
                    xywh: box_,
                    color: rgba(c),
                    clip,
                    extra: [0.0, 0.0, 0.0, 0.0],
                    color2: rgba(c),
                };
                out.push(stroke([x, y, w, t], col));
                out.push(stroke([x, y + h - t, w, t], col));
                out.push(stroke([x, y, t, h], col));
                out.push(stroke([x + w - t, y, t, h], col));
            }
        } else {
            let [x, y, w, h] = xywh;
            let stroke = |box_: [f32; 4], c: crate::css::Rgba, _t: f32| Quad {
                xywh: box_,
                color: rgba(c),
                clip,
                extra: [0.0, 0.0, 0.0, 0.0],
                color2: rgba(c),
            };
            if let Some((bw, col)) = sides[0] {
                let t = (bw * s).max(1.0);
                out.push(stroke([x, y, w, t], col, t));
            }
            if let Some((bw, col)) = sides[1] {
                let t = (bw * s).max(1.0);
                out.push(stroke([x + w - t, y, t, h], col, t));
            }
            if let Some((bw, col)) = sides[2] {
                let t = (bw * s).max(1.0);
                out.push(stroke([x, y + h - t, w, t], col, t));
            }
            if let Some((bw, col)) = sides[3] {
                let t = (bw * s).max(1.0);
                out.push(stroke([x, y, t, h], col, t));
            }
        }
    }
    out
}
