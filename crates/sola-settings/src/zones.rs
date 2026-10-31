//! Settings → Zones (`docs/specs/2026-10-30-zone-layout-design.md`): a
//! drawing of the screen, the grid's rows and columns, and the zones drawn
//! on its cells, each with its key.
//!
//! The layout is `Topic::ZoneLayout` (Settings → shell). Keys are the
//! keybinding system's: each zone is a system action (`zone.full`,
//! `zone.cinema`, `zone.float`, `zone.<id>`), its key one key with Super
//! added, kept in `Topic::Keybindings` like every other change.

use iced::keyboard::Modifiers;
use iced::keyboard::key::Physical;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::widget::{column, container, row, text};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Task, Theme};
use sola_bus::topics::{GridZone, Topic, Zone, ZoneLayout};
use sola_core::{KeyChord, KeyCode};
use sola_kit::app::bus;
use sola_kit::components::button;
use sola_kit::components::style::{SPACE_MD, SPACE_SM};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;
use sola_kit::keys::{self, SYSTEM};

use crate::keybindings::{self, KeysState};
use crate::page;

/// The shell's chrome, as `sola-shell`'s `zoning::content_rect` lays it
/// out: one gap around the edges and between tiles, the top row, the bar.
const GAP: f32 = 10.0;
const TOP_ROW: f32 = 38.0;
const BAR: f32 = 240.0;

/// A zone's row on the page: a built-in or a grid zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Builtin(Zone),
    Grid(u32),
}

impl Row {
    fn zone(self) -> Zone {
        match self {
            Row::Builtin(z) => z,
            Row::Grid(id) => Zone::Grid(id),
        }
    }
}

/// The system action that moves a tab to `zone`.
pub fn action_id(zone: Zone) -> String {
    match zone {
        Zone::Fullscreen => "zone.full".into(),
        Zone::Cinema => "zone.cinema".into(),
        Zone::Float => "zone.float".into(),
        Zone::Grid(id) => format!("zone.{id}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    Idle,
    /// The drawing waits for cells: a new zone (`None`) or `Some(id)`'s.
    Choosing {
        editing: Option<u32>,
        anchor: Option<(u8, u8)>,
        over: Option<(u8, u8)>,
    },
    /// Waiting for a zone's key.
    Keying(Zone),
}

/// What − would take away, waiting for "Delete".
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shrink {
    rows: bool,
    ids: Vec<u32>,
}

#[derive(Default)]
pub struct ZonesState {
    /// The layout as the bus has it (`None`: the shell's default).
    pub layout: Option<ZoneLayout>,
    /// The output's size, for the drawing's aspect.
    pub output: Option<(i32, i32)>,
    mode: Mode,
    open: Option<Row>,
    shrink: Option<Shrink>,
    /// A key another action holds, for the zone waiting for one.
    pending: Option<(Zone, KeyChord)>,
    note: Option<String>,
}

impl ZonesState {
    pub fn layout(&self) -> ZoneLayout {
        self.layout.clone().unwrap_or_default()
    }

    /// Whether the page waits for a key (fold
    /// [`keybindings::capture_keys`] in).
    pub fn keying(&self) -> bool {
        matches!(self.mode, Mode::Keying(_))
    }

    /// Whether the drawing waits for cells (Esc cancels).
    pub fn choosing(&self) -> bool {
        matches!(self.mode, Mode::Choosing { .. })
    }
}

#[derive(Debug, Clone)]
pub enum ZonesMsg {
    Rows(i8),
    Columns(i8),
    DeleteShrink,
    CancelShrink,
    AddZone,
    EditCells(u32),
    Delete(u32),
    Toggle(Row),
    CellDown(u8, u8),
    CellOver(u8, u8),
    CellUp,
    ChangeKey(Zone),
    ClearKey(Zone),
    Pressed(Physical, Modifiers),
    MoveKey,
    Cancel,
}

fn send(layout: &ZoneLayout) {
    let layout = layout.clone();
    let sent = bus()
        .lock()
        .map_err(|_| "bus unavailable".to_string())
        .and_then(|mut client| {
            client
                .emit(Topic::ZoneLayout(layout))
                .map_err(|e| e.to_string())
        });
    if let Err(e) = sent {
        tracing::warn!(error = %e, "zones: bus write failed");
    }
}

pub fn update(msg: ZonesMsg, state: &mut ZonesState, keys: &mut KeysState) -> Task<ZonesMsg> {
    match msg {
        ZonesMsg::Rows(d) | ZonesMsg::Columns(d) if d > 0 => {
            let rows = matches!(msg, ZonesMsg::Rows(_));
            let mut layout = state.layout();
            let n = if rows { &mut layout.rows } else { &mut layout.columns };
            if *n < ZoneLayout::MAX {
                *n += 1;
                state.shrink = None;
                commit(state, layout);
            }
        }
        ZonesMsg::Rows(_) | ZonesMsg::Columns(_) => {
            let rows = matches!(msg, ZonesMsg::Rows(_));
            let layout = state.layout();
            if (if rows { layout.rows } else { layout.columns }) <= 1 {
                return Task::none();
            }
            let ids = layout.cut_by_shrink(rows);
            if ids.is_empty() {
                shrink(state, keys, rows);
            } else {
                state.shrink = Some(Shrink { rows, ids });
            }
        }
        ZonesMsg::DeleteShrink => {
            if let Some(s) = state.shrink.take() {
                shrink(state, keys, s.rows);
            }
        }
        ZonesMsg::CancelShrink => state.shrink = None,
        ZonesMsg::AddZone => {
            state.shrink = None;
            state.note = None;
            state.mode = Mode::Choosing {
                editing: None,
                anchor: None,
                over: None,
            };
        }
        ZonesMsg::EditCells(id) => {
            state.note = None;
            state.mode = Mode::Choosing {
                editing: Some(id),
                anchor: None,
                over: None,
            };
        }
        ZonesMsg::Delete(id) => {
            let mut layout = state.layout();
            layout.zones.retain(|z| z.id != id);
            forget_key(keys, Zone::Grid(id));
            state.open = None;
            commit(state, layout);
        }
        ZonesMsg::Toggle(r) => {
            end_keying(state);
            state.pending = None;
            state.note = None;
            state.open = (state.open != Some(r)).then_some(r);
        }
        ZonesMsg::CellDown(r, c) => {
            if let Mode::Choosing { editing, .. } = state.mode {
                state.mode = Mode::Choosing {
                    editing,
                    anchor: Some((r, c)),
                    over: Some((r, c)),
                };
            }
        }
        ZonesMsg::CellOver(r, c) => {
            if let Mode::Choosing {
                editing,
                anchor: Some(a),
                ..
            } = state.mode
            {
                state.mode = Mode::Choosing {
                    editing,
                    anchor: Some(a),
                    over: Some((r, c)),
                };
            }
        }
        ZonesMsg::CellUp => {
            let Mode::Choosing {
                editing,
                anchor: Some(a),
                over: Some(o),
            } = state.mode
            else {
                return Task::none();
            };
            return chosen(state, keys, editing, cells_of(a, o));
        }
        ZonesMsg::ChangeKey(zone) => {
            state.pending = None;
            state.note = None;
            state.mode = Mode::Keying(zone);
            keybindings::set_capture(true);
        }
        ZonesMsg::ClearKey(zone) => {
            assign_key(keys, zone, None);
        }
        ZonesMsg::Pressed(physical, modifiers) => {
            if state.choosing() {
                if keys::chord_from_iced(&physical, modifiers).is_some_and(|c| c.keycode == KeyCode::ESCAPE) {
                    state.mode = Mode::Idle;
                }
                return Task::none();
            }
            let Mode::Keying(zone) = state.mode else {
                return Task::none();
            };
            let Some(chord) = keys::chord_from_iced(&physical, modifiers) else {
                return Task::none();
            };
            end_keying(state);
            if chord.keycode == KeyCode::ESCAPE {
                return Task::none();
            }
            // One key; Super is added.
            let chord = chord.keycode.meta();
            let bindings = keys.bindings();
            let id = action_id(zone);
            if keys::holder(&bindings, SYSTEM, &id, chord).is_some() {
                state.pending = Some((zone, chord));
            } else {
                assign_key(keys, zone, Some(chord));
            }
        }
        ZonesMsg::MoveKey => {
            if let Some((zone, chord)) = state.pending.take() {
                let bindings = keys.bindings();
                if let Some(h) = keys::holder(&bindings, SYSTEM, &action_id(zone), chord).cloned() {
                    keys::set(&mut keys.keys, &h.owner, &h.action, h.default, None);
                }
                assign_key(keys, zone, Some(chord));
            }
        }
        ZonesMsg::Cancel => {
            end_keying(state);
            state.mode = Mode::Idle;
            state.pending = None;
            state.shrink = None;
        }
    }
    Task::none()
}

/// Cells were chosen: a new zone (then its key), or a zone's new cells.
fn chosen(
    state: &mut ZonesState,
    _keys: &mut KeysState,
    editing: Option<u32>,
    cells: (u8, u8, u8, u8),
) -> Task<ZonesMsg> {
    let mut layout = state.layout();
    let (row, column, rows, columns) = cells;
    let zone = GridZone {
        id: editing.unwrap_or_else(|| layout.next_id()),
        row,
        column,
        rows,
        columns,
    };
    if rows == layout.rows && columns == layout.columns {
        state.mode = Mode::Idle;
        state.note = Some("Every cell is Full already.".into());
        return Task::none();
    }
    if let Some(same) = layout
        .zones
        .iter()
        .find(|z| z.id != zone.id && z.same_cells(&zone))
    {
        state.mode = Mode::Idle;
        state.note = Some(format!("{} has those cells already.", layout.label(same.id)));
        return Task::none();
    }
    match layout.zones.iter_mut().find(|z| z.id == zone.id) {
        Some(z) => *z = zone,
        None => layout.zones.push(zone),
    }
    state.note = None;
    commit(state, layout);
    state.open = Some(Row::Grid(zone.id));
    if editing.is_some() {
        state.mode = Mode::Idle;
    } else {
        // A new zone asks for its key at once.
        state.mode = Mode::Keying(Zone::Grid(zone.id));
        keybindings::set_capture(true);
    }
    Task::none()
}

/// Take the bottom row (or the right column) away, and the zones on it.
fn shrink(state: &mut ZonesState, keys: &mut KeysState, rows: bool) {
    let mut layout = state.layout();
    for id in layout.cut_by_shrink(rows) {
        forget_key(keys, Zone::Grid(id));
    }
    layout.shrink(rows);
    commit(state, layout);
}

fn commit(state: &mut ZonesState, layout: ZoneLayout) {
    send(&layout);
    state.layout = Some(layout);
}

fn assign_key(keys: &mut KeysState, zone: Zone, key: Option<KeyChord>) {
    let id = action_id(zone);
    let default = keys
        .decls
        .iter()
        .find(|d| d.owner == SYSTEM)
        .and_then(|d| d.actions.iter().find(|a| a.id == id))
        .and_then(|a| a.default);
    keys::set(&mut keys.keys, SYSTEM, &id, default, key);
    keybindings::save(&keys.keys);
}

/// A deleted zone's key goes with it.
fn forget_key(keys: &mut KeysState, zone: Zone) {
    keys::reset(&mut keys.keys, SYSTEM, &action_id(zone));
    keybindings::save(&keys.keys);
}

fn end_keying(state: &mut ZonesState) {
    if matches!(state.mode, Mode::Keying(_)) {
        state.mode = Mode::Idle;
        keybindings::set_capture(false);
    }
}

/// Leave no capture behind (the page or the app is going away).
pub fn leave(state: &mut ZonesState) {
    end_keying(state);
    state.mode = Mode::Idle;
}

/// The rectangle of cells between two corners: (row, column, rows, columns).
fn cells_of(a: (u8, u8), b: (u8, u8)) -> (u8, u8, u8, u8) {
    let (r0, r1) = (a.0.min(b.0), a.0.max(b.0));
    let (c0, c1) = (a.1.min(b.1), a.1.max(b.1));
    (r0, c0, r1 - r0 + 1, c1 - c0 + 1)
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

fn aspect(state: &ZonesState) -> f32 {
    match state.output {
        Some((w, h)) if w > 0 && h > 0 => h as f32 / w as f32,
        _ => 9.0 / 16.0,
    }
}

pub fn view<'a>(state: &'a ZonesState, keys: &'a KeysState) -> Element<'a, ZonesMsg> {
    let layout = state.layout();
    let bindings = keys.bindings();
    let key_of = |z: Zone| keys::key_of(&bindings, SYSTEM, &action_id(z));

    let selection = match state.mode {
        Mode::Choosing {
            anchor: Some(a),
            over: Some(o),
            ..
        } => Some(cells_of(a, o)),
        _ => None,
    };
    let highlight = state.open.map(Row::zone);
    let labels: Vec<(u32, String)> = layout
        .zones
        .iter()
        .map(|z| (z.id, key_of(Zone::Grid(z.id)).map_or(String::new(), key_label)))
        .collect();
    let drawing = canvas::Canvas::new(Drawing {
        layout: layout.clone(),
        output: state.output,
        selection,
        highlight,
        labels,
        choosing: state.choosing(),
        doomed: state.shrink.as_ref().map_or(Vec::new(), |s| s.ids.clone()),
        editing: match state.mode {
            Mode::Choosing { editing, .. } => editing,
            _ => None,
        },
    })
    .width(Length::Fill)
    .height(Length::Fixed(page::PAGE_WIDTH * aspect(state)));

    let stepper = |label: &'a str, n: u8, minus: ZonesMsg, plus: ZonesMsg| -> Element<'a, ZonesMsg> {
        row![
            kit_text::body(label),
            page::action("−", button::secondary, minus),
            text(n.to_string()).font(fonts::mono()).size(13),
            page::action("+", button::secondary, plus),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center)
        .into()
    };
    let mut screen = column![
        drawing,
        row![
            stepper("Rows", layout.rows, ZonesMsg::Rows(-1), ZonesMsg::Rows(1)),
            iced::widget::Space::new().width(Length::Fixed(SPACE_MD * 2.0)),
            stepper("Columns", layout.columns, ZonesMsg::Columns(-1), ZonesMsg::Columns(1)),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(SPACE_MD);
    if let Some(s) = &state.shrink {
        let what = if s.rows { "the bottom row" } else { "the right column" };
        let n = s.ids.len();
        screen = screen.push(column![
            kit_text::body(format!(
                "Taking away {what} deletes {n} zone{} on it (marked).",
                if n == 1 { "" } else { "s" }
            )),
            page::footer(
                vec![page::action(
                    if n == 1 { "Delete 1 Zone".to_string() } else { format!("Delete {n} Zones") },
                    button::danger,
                    ZonesMsg::DeleteShrink,
                )],
                vec![page::action("Cancel", button::secondary, ZonesMsg::CancelShrink)],
            ),
        ]
        .spacing(SPACE_SM));
    }
    let hint = match state.mode {
        Mode::Choosing { editing: None, .. } => {
            Some("Drag across cells, or click one, for the new zone. Esc cancels.".to_string())
        }
        Mode::Choosing { editing: Some(id), .. } => Some(format!(
            "Drag across cells, or click one, for {}. Esc cancels.",
            layout.label(id)
        )),
        _ => state.note.clone(),
    };
    let caption = hint.or_else(|| {
        Some("A zone is a rectangle of cells. Full covers them all; Cinema the whole screen.".into())
    });

    let mut rows: Vec<Element<'a, ZonesMsg>> = Vec::new();
    for zone in [Zone::Fullscreen, Zone::Cinema, Zone::Float] {
        rows.extend(zone_row(state, &layout, Row::Builtin(zone), key_of(zone)));
    }
    for z in &layout.zones {
        rows.extend(zone_row(state, &layout, Row::Grid(z.id), key_of(Zone::Grid(z.id))));
    }

    page::page(vec![
        page::section("Screen", None, screen.into(), caption),
        page::section(
            "Zones",
            Some(page::add("Add Zone", ZonesMsg::AddZone)),
            page::group(rows),
            Some("Each key comes with Super. Keybindings lists them too.".into()),
        ),
    ])
}

fn key_label(chord: KeyChord) -> String {
    chord.keycode.display().to_string()
}

fn zone_row<'a>(
    state: &'a ZonesState,
    layout: &ZoneLayout,
    r: Row,
    key: Option<KeyChord>,
) -> Vec<Element<'a, ZonesMsg>> {
    let open = state.open == Some(r);
    let title = match r {
        Row::Builtin(Zone::Fullscreen) => "Full".to_string(),
        Row::Builtin(Zone::Cinema) => "Cinema".to_string(),
        Row::Builtin(Zone::Float) => "Floating".to_string(),
        Row::Builtin(_) => String::new(),
        Row::Grid(id) => layout.label(id),
    };
    let subtitle = match r {
        Row::Builtin(Zone::Fullscreen) => "Every cell; new workspaces start here",
        Row::Builtin(Zone::Cinema) => "The whole screen, nothing of the desk",
        Row::Builtin(Zone::Float) => "The window's own size",
        _ => "",
    };
    let chip: Element<'a, ZonesMsg> = match key {
        Some(c) => text(c.display()).font(fonts::ui()).size(13).into(),
        None => kit_text::body("No key").style(kit_text::muted).into(),
    };
    let mini = canvas::Canvas::new(Mini {
        layout: layout.clone(),
        zone: r.zone(),
    })
    .width(Length::Fixed(36.0))
    .height(Length::Fixed(36.0 * aspect(state)));
    let mut out = vec![
        page::item(title)
            .subtitle(subtitle)
            .leading(mini)
            .trailing(chip)
            .open(open)
            .on_press(ZonesMsg::Toggle(r))
            .into(),
    ];
    if open {
        out.push(page::editor(zone_editor(state, layout, r, key)));
    }
    out
}

fn zone_editor<'a>(
    state: &'a ZonesState,
    layout: &ZoneLayout,
    r: Row,
    key: Option<KeyChord>,
) -> Element<'a, ZonesMsg> {
    let zone = r.zone();
    if state.mode == Mode::Keying(zone) {
        return column![
            kit_text::body("Press a key for this zone (Super is added). Esc leaves it without one."),
            page::footer(vec![], vec![page::action("Cancel", button::secondary, ZonesMsg::Cancel)]),
        ]
        .spacing(SPACE_SM)
        .into();
    }
    if let Some((z, chord)) = state.pending.filter(|(z, _)| *z == zone) {
        let _ = z;
        return column![
            kit_text::body(format!("{} is taken. Move it to this zone?", chord.display())),
            page::footer(
                vec![page::action("Move It Here", button::primary, ZonesMsg::MoveKey)],
                vec![page::action("Cancel", button::secondary, ZonesMsg::Cancel)],
            ),
        ]
        .spacing(SPACE_SM)
        .into();
    }
    let mut left = vec![page::action(
        if key.is_some() { "Change Key" } else { "Set Key" },
        button::primary,
        ZonesMsg::ChangeKey(zone),
    )];
    if key.is_some() {
        left.push(page::action("Clear Key", button::secondary, ZonesMsg::ClearKey(zone)));
    }
    let mut right = Vec::new();
    if let Row::Grid(id) = r {
        left.push(page::action("Edit Cells", button::secondary, ZonesMsg::EditCells(id)));
        right.push(page::action("Delete", button::danger_outline, ZonesMsg::Delete(id)));
    }
    let _ = layout;
    container(page::footer(left, right)).width(Length::Fill).into()
}

// ---------------------------------------------------------------------------
// The drawing
// ---------------------------------------------------------------------------

/// Where the content area sits in a screen drawn in `bounds`, and the
/// drawing's scale (drawn pixels per output pixel).
fn placed(bounds: Size, output: Option<(i32, i32)>) -> (Rectangle, Rectangle, f32) {
    let (ow, oh) = output
        .filter(|(w, h)| *w > 0 && *h > 0)
        .map_or((1920.0, 1080.0), |(w, h)| (w as f32, h as f32));
    let s = (bounds.width / ow).min(bounds.height / oh);
    let screen = Rectangle::new(Point::ORIGIN, Size::new(ow * s, oh * s));
    let x = (GAP + BAR + GAP) * s;
    let y = (TOP_ROW + GAP) * s;
    let content = Rectangle::new(
        Point::new(x, y),
        Size::new((ow - GAP - BAR - GAP - GAP) * s, (oh - TOP_ROW - GAP - GAP) * s),
    );
    (screen, content, s)
}

/// A rectangle of cells in the content area, one gap between neighbours
/// (as the shell frames them).
fn cell_rect(content: Rectangle, s: f32, layout: &ZoneLayout, cells: (u8, u8, u8, u8)) -> Rectangle {
    let (row, column, rows, columns) = cells;
    let gap = GAP * s;
    let edge = |n: u8, count: u8, from: f32, len: f32| -> f32 {
        let avail = len - gap * (f32::from(count) - 1.0);
        from + avail * f32::from(n) / f32::from(count) + f32::from(n) * gap
    };
    let x0 = edge(column, layout.columns, content.x, content.width);
    let x1 = edge(column + columns, layout.columns, content.x, content.width) - gap;
    let y0 = edge(row, layout.rows, content.y, content.height);
    let y1 = edge(row + rows, layout.rows, content.y, content.height) - gap;
    Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0))
}

fn rounded(r: Rectangle, radius: f32) -> Path {
    Path::new(|p| p.rounded_rectangle(r.position(), r.size(), radius.into()))
}

struct Drawing {
    layout: ZoneLayout,
    output: Option<(i32, i32)>,
    selection: Option<(u8, u8, u8, u8)>,
    highlight: Option<Zone>,
    labels: Vec<(u32, String)>,
    choosing: bool,
    /// The zones − would delete, marked while it asks.
    doomed: Vec<u32>,
    editing: Option<u32>,
}

impl Drawing {
    fn cell_at(&self, bounds: Size, p: Point) -> Option<(u8, u8)> {
        let (_, content, _) = placed(bounds, self.output);
        if !content.contains(p) {
            return None;
        }
        let c = ((p.x - content.x) / content.width * f32::from(self.layout.columns)) as u8;
        let r = ((p.y - content.y) / content.height * f32::from(self.layout.rows)) as u8;
        Some((r.min(self.layout.rows - 1), c.min(self.layout.columns - 1)))
    }
}

/// Whether the left button is down on the drawing (a drag of cells).
#[derive(Default)]
struct Press(bool);

impl canvas::Program<ZonesMsg> for Drawing {
    type State = Press;

    fn update(
        &self,
        state: &mut Press,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<ZonesMsg>> {
        if !self.choosing {
            return None;
        }
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let (r, c) = self.cell_at(bounds.size(), cursor.position_in(bounds)?)?;
                state.0 = true;
                Some(canvas::Action::publish(ZonesMsg::CellDown(r, c)).and_capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) if state.0 => {
                let (r, c) = self.cell_at(bounds.size(), cursor.position_in(bounds)?)?;
                Some(canvas::Action::publish(ZonesMsg::CellOver(r, c)).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.0 => {
                state.0 = false;
                Some(canvas::Action::publish(ZonesMsg::CellUp).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Press,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let p = theme.extended_palette();
        let accent = p.primary.base.color;
        let danger = p.danger.base.color;
        let ink = p.background.base.text;
        let mut f = Frame::new(renderer, bounds.size());
        let (screen, content, s) = placed(bounds.size(), self.output);
        f.fill(&rounded(screen, 6.0), Color::from_rgb(0.06, 0.06, 0.07));
        // The top row and the bar, as fixed chrome.
        let chrome = Color { a: 0.10, ..ink };
        f.fill(
            &rounded(
                Rectangle::new(Point::new(GAP * s, GAP * s * 0.5), Size::new(screen.width - 2.0 * GAP * s, (TOP_ROW - GAP) * s)),
                2.0,
            ),
            chrome,
        );
        f.fill(
            &rounded(
                Rectangle::new(
                    Point::new(GAP * s, (TOP_ROW + GAP) * s),
                    Size::new(BAR * s, screen.height - (TOP_ROW + 2.0 * GAP) * s),
                ),
                3.0,
            ),
            chrome,
        );
        // The cells.
        let faint = Stroke::default().with_color(Color { a: 0.14, ..ink }).with_width(1.0);
        for r in 0..self.layout.rows {
            for c in 0..self.layout.columns {
                let cell = cell_rect(content, s, &self.layout, (r, c, 1, 1));
                f.stroke(&rounded(cell, 3.0), faint);
            }
        }
        // The zones, the open one filled.
        for z in &self.layout.zones {
            if self.editing == Some(z.id) {
                continue;
            }
            let rect = cell_rect(content, s, &self.layout, (z.row, z.column, z.rows, z.columns));
            let lit = self.highlight == Some(Zone::Grid(z.id));
            let doomed = self.doomed.contains(&z.id);
            let edge = if doomed { danger } else { accent };
            if lit || doomed {
                f.fill(&rounded(rect, 4.0), Color { a: 0.28, ..edge });
            }
            f.stroke(
                &rounded(rect, 4.0),
                Stroke::default()
                    .with_color(Color { a: if lit || doomed { 0.95 } else { 0.55 }, ..edge })
                    .with_width(if lit || doomed { 2.0 } else { 1.5 }),
            );
            let label = self
                .labels
                .iter()
                .find(|(id, _)| *id == z.id)
                .map_or(String::new(), |(_, l)| l.clone());
            if !label.is_empty() {
                f.fill_text(Text {
                    content: label,
                    position: rect.center(),
                    color: Color { a: 0.9, ..ink },
                    size: 13.0.into(),
                    font: fonts::mono(),
                    align_x: iced::alignment::Horizontal::Center.into(),
                    align_y: iced::alignment::Vertical::Center,
                    ..Text::default()
                });
            }
        }
        match self.highlight {
            Some(Zone::Fullscreen) => f.fill(&rounded(content, 4.0), Color { a: 0.22, ..accent }),
            Some(Zone::Cinema) => f.fill(&rounded(screen, 6.0), Color { a: 0.22, ..accent }),
            _ => {}
        }
        if let Some(cells) = self.selection {
            let rect = cell_rect(content, s, &self.layout, cells);
            f.fill(&rounded(rect, 4.0), Color { a: 0.40, ..accent });
            f.stroke(&rounded(rect, 4.0), Stroke::default().with_color(accent).with_width(2.0));
        }
        vec![f.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Press,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let on_cells = cursor
            .position_in(bounds)
            .and_then(|p| self.cell_at(bounds.size(), p))
            .is_some();
        if self.choosing && on_cells {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

/// A zone's little drawing: the screen, the zone filled.
struct Mini {
    layout: ZoneLayout,
    zone: Zone,
}

impl canvas::Program<ZonesMsg> for Mini {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let p = theme.extended_palette();
        let ink = p.background.base.text;
        let accent = p.primary.base.color;
        let mut f = Frame::new(renderer, bounds.size());
        let screen = Rectangle::new(Point::ORIGIN, bounds.size());
        f.stroke(
            &rounded(screen, 2.0),
            Stroke::default().with_color(Color { a: 0.35, ..ink }).with_width(1.0),
        );
        // Content area, by the drawing's proportions (bar a sliver).
        let inset = 2.0;
        let content = Rectangle::new(
            Point::new(bounds.width * 0.08 + inset, bounds.height * 0.08 + inset),
            Size::new(bounds.width * 0.92 - 2.0 * inset, bounds.height * 0.92 - 2.0 * inset),
        );
        let fill = Color { a: 0.75, ..accent };
        match self.zone {
            Zone::Cinema => f.fill(&rounded(Rectangle::new(Point::new(inset, inset), Size::new(bounds.width - 2.0 * inset, bounds.height - 2.0 * inset)), 1.5), fill),
            Zone::Fullscreen => f.fill(&rounded(content, 1.5), fill),
            Zone::Float => {
                let w = content.width * 0.5;
                let h = content.height * 0.5;
                f.fill(
                    &rounded(Rectangle::new(Point::new(content.center_x() - w / 2.0, content.center_y() - h / 2.0), Size::new(w, h)), 1.5),
                    fill,
                );
            }
            Zone::Grid(id) => {
                if let Some(z) = self.layout.zones.iter().find(|z| z.id == id) {
                    let rect = cell_rect(content, 0.0, &self.layout, (z.row, z.column, z.rows, z.columns));
                    f.fill(&rounded(rect, 1.5), fill);
                }
            }
        }
        vec![f.into_geometry()]
    }
}
