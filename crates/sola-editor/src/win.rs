//! One window: a file being edited and how it is showing.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use iced::widget::canvas;
use iced::{Size, widget};
use sola_kit::fonts;

use crate::buffer::Buffer;
use crate::editor::Editor;
use crate::files::{self, Stamp};
use crate::overlay::{Mini, Palette, Prompt};
use crate::session;
use crate::syntax::Highlight;
use crate::view::{Colors, SCROLLBAR};

/// Zed's `buffer_font_size`, as he had it.
const FONT_SIZE: f32 = 15.0;

/// The monospace grid text sits on. The cell is as wide as the font says,
/// fractions and all (7.5 for Iosevka at 15): a long run lays its glyphs out
/// at that width, so a rounder cell leaves a gap wherever a color starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub cell_w: f32,
    pub cell_h: f32,
    pub font_size: f32,
}

impl Grid {
    pub fn current() -> Self {
        let metrics = fonts::mono_metrics();
        Self {
            cell_w: (metrics.advance_per_em * FONT_SIZE).max(1.0),
            cell_h: (metrics.line_per_em * FONT_SIZE).ceil().max(1.0),
            font_size: FONT_SIZE,
        }
    }

    /// The line numbers' width: room for the last line, and a cell each side.
    pub fn gutter(&self, lines: usize) -> f32 {
        let digits = lines.to_string().len().max(3);
        (digits + 2) as f32 * self.cell_w
    }
}

/// How the view should move to keep up with the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follow {
    Reveal,
    Center,
}

/// What the text drawing depends on. When it changes, redraw it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawKey {
    pub revision: u64,
    pub top: usize,
    pub left: usize,
    pub size: Size,
    pub highlight: u64,
    pub colors: Colors,
    pub grid: Grid,
}

pub struct Win {
    pub ed: Editor,
    pub path: Option<PathBuf>,
    /// Its tab: the path, or `untitled-<n>`.
    pub key: String,
    pub stamp: Option<Stamp>,
    pub hl: Highlight,
    pub grid: Grid,

    /// The first line and column showing.
    pub top: usize,
    pub left: usize,
    /// What's left over of a scroll smaller than a line.
    pub wheel: f32,
    /// The drawing area's size at the last draw.
    pub viewport: Cell<Size>,
    /// The drawing is short of parsed lines: the app should parse on.
    pub hl_behind: Cell<bool>,
    pub text: canvas::Cache,
    pub drawn: Cell<Option<DrawKey>>,

    pub focused: bool,
    pub mini: Option<Mini>,
    pub palette: Option<Palette>,
    pub prompt: Option<Prompt>,
    /// The line at the bottom: `C-x-`, "Wrote …".
    pub note: Option<String>,
    /// On disk it is no longer what we loaded, and we have unsaved edits.
    pub changed_on_disk: bool,
    /// The buffer revision last written to the recovery file.
    pub recovered: Option<u64>,
}

impl Win {
    pub fn new(text: &str, path: Option<PathBuf>, key: String, stamp: Option<Stamp>) -> Self {
        let ed = Editor::new(text, path.as_deref());
        let first = ed.buf.line(0);
        let hl = Highlight::new(path.as_deref(), first);
        Self {
            ed,
            path,
            key,
            stamp,
            hl,
            grid: Grid::current(),
            top: 0,
            left: 0,
            wheel: 0.0,
            viewport: Cell::new(Size::new(1000.0, 760.0)),
            hl_behind: Cell::new(false),
            text: canvas::Cache::new(),
            drawn: Cell::new(None),
            focused: false,
            mini: None,
            palette: None,
            prompt: None,
            note: None,
            changed_on_disk: false,
            recovered: None,
        }
    }

    /// The name of the file, or "untitled".
    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "untitled".to_string(), |n| n.to_string_lossy().into_owned())
    }

    /// A window's title, and so its row in the bar.
    pub fn title(&self) -> String {
        if self.ed.buf.is_dirty() {
            format!("• {}", self.name())
        } else {
            self.name()
        }
    }

    /// Nothing in it and nothing to lose: a file opening can take its place.
    pub fn is_pristine(&self) -> bool {
        self.path.is_none() && !self.ed.buf.is_dirty() && self.ed.buf.text().is_empty()
    }

    /// Whole lines that fit.
    pub fn rows(&self) -> usize {
        ((self.viewport.get().height / self.grid.cell_h).floor() as usize).max(1)
    }

    /// Whole columns that fit.
    pub fn columns(&self) -> usize {
        let gutter = self.grid.gutter(self.ed.buf.line_count());
        (((self.viewport.get().width - gutter - SCROLLBAR) / self.grid.cell_w).floor() as usize).max(1)
    }

    /// A page down moves by this: a window less two lines kept for context.
    pub fn page(&self) -> usize {
        self.rows().saturating_sub(2).max(1)
    }

    /// Bring the point into view.
    pub fn follow(&mut self, follow: Follow) {
        let rows = self.rows();
        let line = self.ed.point.line;
        match follow {
            Follow::Center => self.top = line.saturating_sub(rows / 2),
            Follow::Reveal => {
                if line < self.top {
                    self.top = line;
                } else if line >= self.top + rows {
                    self.top = line + 1 - rows;
                }
            }
        }
        let column = self.ed.point_column();
        let columns = self.columns();
        if column < self.left {
            self.left = column.saturating_sub(8);
        } else if column >= self.left + columns {
            self.left = column + 8 - columns.min(column + 8);
        }
    }

    /// Scroll by whole lines and columns (a wheel, a trackpad).
    pub fn scroll(&mut self, lines: f32, columns: f32) {
        self.wheel += lines;
        let whole = self.wheel.trunc();
        self.wheel -= whole;
        let last = self.ed.buf.line_count().saturating_sub(1) as isize;
        self.top = (self.top as isize + whole as isize).clamp(0, last) as usize;
        self.left = (self.left as isize + columns as isize).max(0) as usize;
    }

    /// The file on disk replaced what we had (no unsaved edits to lose).
    pub fn reload(&mut self, loaded: files::Loaded) {
        let point = self.ed.point;
        self.ed.buf = Buffer::from_text(&loaded.text);
        self.ed.read_only = loaded.read_only;
        self.ed.set_point(point);
        self.stamp = loaded.stamp;
        self.changed_on_disk = false;
        self.recovered = None;
        self.rebuild_highlight();
    }

    pub fn rebuild_highlight(&mut self) {
        self.hl = Highlight::new(self.path.as_deref(), self.ed.buf.line(0));
        self.ed.buf.take_changed();
    }

    /// Saved to `path` as of `stamp`.
    pub fn mark_saved(&mut self, path: &Path, stamp: Stamp) {
        let renamed = self.path.as_deref() != Some(path);
        self.path = Some(path.to_path_buf());
        self.stamp = Some(stamp);
        self.ed.buf.mark_saved();
        self.changed_on_disk = false;
        session::remove_recover(&self.recover_name());
        self.recovered = None;
        if renamed {
            self.ed.set_path(path);
            self.rebuild_highlight();
        }
    }

    pub fn recover_name(&self) -> String {
        session::recover_name(&self.key)
    }

    /// Unsaved text newer than the recovery file.
    pub fn needs_recover(&self) -> bool {
        self.ed.buf.is_dirty() && self.recovered != Some(self.ed.buf.revision())
    }

    /// Put unsaved text where a restart can find it.
    pub fn write_recover(&mut self) {
        if self.needs_recover() {
            session::write_recover(&self.recover_name(), &self.ed.buf.text());
            self.recovered = Some(self.ed.buf.revision());
        }
    }

    pub fn entry(&self) -> session::Entry {
        session::Entry {
            path: self.path.clone(),
            line: self.ed.point.line,
            col: self.ed.point.col,
            top: self.top,
            recover: self.ed.buf.is_dirty().then(|| self.recover_name()),
        }
    }
}

/// The field of the find and go-to line at a window's foot.
pub fn mini_id() -> widget::Id {
    widget::Id::new("editor-mini")
}

/// The field of the command palette.
pub fn palette_id() -> widget::Id {
    widget::Id::new("editor-palette")
}
