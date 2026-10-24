//! sola-paint application: a window per open image (Next; see
//! [`crate::windows`]) + graphite edit stage.

use std::path::PathBuf;
use std::time::Duration;

use iced::event;
use iced::keyboard;
use iced::keyboard::key::Named as NamedKey;
use iced::widget::tooltip::Position as TooltipPosition;
use iced::widget::{Space, column, container, row, text, tooltip};
use iced::{
    Alignment, Background, Border, Color, Element, Event, Length, Padding, Subscription, Task,
    Theme, window,
};

use sola_bus::topics::{MenuDefinition, MenuItem, PaintSession, Topic, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup};
use sola_kit::components::button as kit_btn;
use sola_kit::components::file_picker::{FilePicker, Message as PickerMsg, Outcome};
use sola_kit::components::icon::icon_handle;
use sola_kit::components::popover;
use sola_kit::components::style::{
    CHROME_SURFACE, HAIRLINE_A, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, mix_white,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::toolbar::toolbar_icon;
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use crate::Msg;
use crate::doc::{Doc, Loaded};
use crate::geom;
use crate::stage::{self, CropGesture};
use crate::windows::{EMPTY, PaintWindows};

pub const APP_ID: &str = "sola-paint";
const HEADER_H: f32 = 52.0;
const MAX_DOCS: usize = 32;
/// How long boot waits for the saved session before showing the empty
/// window.
const BOOT_SETTLE: Duration = Duration::from_millis(400);

pub fn run() -> iced::Result {
    startup(APP_ID);

    let argv: Vec<PathBuf> = std::env::args()
        .skip(1)
        .filter(|a| !a.is_empty())
        .map(|a| abs_path(PathBuf::from(a)))
        .collect();
    if crate::instance::claim() == crate::instance::Claim::Handoff {
        if let Err(e) = crate::instance::handoff(&argv) {
            tracing::warn!(error = %e, "handoff to existing paint failed");
        }
        return Ok(());
    }

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu("Paint", [("quit", "Quit Paint", KeyCode::Q.meta())])
        .app_menu_definition(MenuDefinition {
            label: "File".into(),
            items: vec![
                item("open", "Open…", Some(KeyCode::O.meta())),
                item("save", "Save", Some(KeyCode::S.meta())),
                item("save_as", "Save As…", Some(KeyCode::S.meta().shift())),
                MenuItem::Divider,
                item("set_background", "Set as Background", None),
                MenuItem::Divider,
                item("close_tab", "Close Window", Some(KeyCode::W.meta())),
            ],
        })
        .app_menu(
            "Edit",
            [
                ("undo", "Undo", KeyCode::Z.meta()),
                ("crop", "Crop", KeyCode::K.meta_shift()),
                ("rotate_cw", "Rotate Right", KeyCode::R.meta()),
                ("rotate_ccw", "Rotate Left", KeyCode::R.meta().shift()),
            ],
        )
        .app_menu(
            "View",
            [
                ("zoom_in", "Zoom In", KeyCode::EQUAL.meta()),
                ("zoom_out", "Zoom Out", KeyCode::MINUS.meta()),
                ("zoom_fit", "Fit", KeyCode::KEY_0.meta()),
            ],
        )
        .install();

    iced::daemon(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .run()
}

fn item(id: &str, label: &str, shortcut: Option<sola_core::KeyChord>) -> MenuItem {
    MenuItem::Action {
        id: id.into(),
        label: label.into(),
        shortcut,
        disabled: false,
        checked: false,
    }
}

struct Icons {
    crop: iced::widget::svg::Handle,
    rotate_cw: iced::widget::svg::Handle,
    rotate_ccw: iced::widget::svg::Handle,
    flip_h: iced::widget::svg::Handle,
    flip_v: iced::widget::svg::Handle,
    undo: iced::widget::svg::Handle,
    save: iced::widget::svg::Handle,
    folder: iced::widget::svg::Handle,
}

impl Icons {
    fn new() -> Self {
        Self {
            crop: icon_handle("lucide/crop"),
            rotate_cw: icon_handle("lucide/rotate-cw"),
            rotate_ccw: icon_handle("lucide/rotate-ccw"),
            flip_h: icon_handle("lucide/flip-horizontal-2"),
            flip_v: icon_handle("lucide/flip-vertical-2"),
            undo: icon_handle("lucide/undo-2"),
            save: icon_handle("lucide/save"),
            folder: icon_handle("lucide/folder-open"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerKind {
    Open,
    SaveAs,
}

pub struct App {
    docs: Vec<Doc>,
    /// The focused window's image: the one the tools, keys and menus act
    /// on.
    selected: Option<u64>,
    next_id: u64,
    /// A window per image ([`crate::windows`]).
    windows: PaintWindows,
    cropping: bool,
    crop: Option<CropGesture>,
    /// Pointer at pan start + the pan vector at that moment.
    panning: Option<(iced::Point, iced::Vector)>,
    /// An open file dialog, in the window it was opened from.
    picker: Option<(PickerKind, FilePicker, window::Id)>,
    last_dir: Option<PathBuf>,
    status: Option<String>,
    theme: Theme,
    float: sola_kit::FloatState,
    icons: Icons,
    /// True after the first sticky `PaintSession` (restore or our emit).
    restored: bool,
    /// True while a restore batch is decoding — don't persist mid-flight.
    restoring: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            docs: Vec::new(),
            selected: None,
            next_id: 1,
            windows: PaintWindows::new(APP_ID),
            cropping: false,
            crop: None,
            panning: None,
            picker: None,
            last_dir: None,
            status: None,
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            icons: Icons::new(),
            restored: false,
            restoring: false,
        }
    }
}

impl App {
    /// Open a window for each image on the command line (the first in
    /// front); with none, wait a moment for the saved session before
    /// showing the empty window.
    pub fn boot() -> (Self, Task<Msg>) {
        let mut app = Self::default();
        let mut tasks = Vec::new();
        for arg in std::env::args().skip(1) {
            if arg.is_empty() {
                continue;
            }
            tasks.push(app.open_path_sync(abs_path(PathBuf::from(arg))));
        }
        tasks.push(Task::perform(tokio::time::sleep(BOOT_SETTLE), |()| {
            Msg::BootSettled
        }));
        (app, Task::batch(tasks))
    }

    /// A window's title: its image's file name (the shell's bar row).
    pub fn title(&self, window: window::Id) -> String {
        self.windows
            .doc_of(window)
            .and_then(|id| self.docs.iter().find(|d| d.id == id))
            .map_or_else(|| "Paint".into(), Doc::label)
    }

    pub fn theme(&self, window: window::Id) -> Theme {
        sola_kit::theme_for(self.chrome(window), &self.theme)
    }

    fn chrome(&self, window: window::Id) -> sola_kit::Chrome {
        self.windows
            .river_id(window)
            .map_or(sola_kit::Chrome::Tiled, |id| self.float.chrome(id))
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            event::listen_with(|event, _status, id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => Some(
                    Msg::At(id, Box::new(Msg::KeyPressed(key, modifiers))),
                ),
                Event::Window(
                    e @ (window::Event::Focused
                    | window::Event::CloseRequested
                    | window::Event::Closed),
                ) => Some(Msg::Window(id, e)),
                _ => None,
            }),
        ])
    }

    /// `window` is the one acted on from now on: its image is selected. A
    /// crop in progress elsewhere is dropped.
    fn focus(&mut self, window: window::Id) {
        let Some(doc) = self.windows.doc_of(window) else {
            return;
        };
        self.windows.focused = Some(window);
        let selected = (doc != EMPTY).then_some(doc);
        if selected != self.selected {
            self.selected = selected;
            self.cancel_crop();
            // Not before the saved session is read: it would be replaced.
            if self.restored {
                self.persist_session();
            }
        }
    }

    /// The focused window's stage.
    fn stage(&self) -> Option<&crate::windows::Win> {
        self.windows.get(self.windows.focused?)
    }

    fn stage_mut(&mut self) -> Option<&mut crate::windows::Win> {
        self.windows.get_mut(self.windows.focused?)
    }

    fn selected_doc(&self) -> Option<&Doc> {
        let id = self.selected?;
        self.docs.iter().find(|d| d.id == id)
    }

    fn selected_doc_mut(&mut self) -> Option<&mut Doc> {
        let id = self.selected?;
        self.docs.iter_mut().find(|d| d.id == id)
    }

    fn invalidate_stage(&mut self) {
        if let Some(win) = self.stage_mut() {
            win.cache.clear();
        }
    }

    /// Add `doc` and open its window (`background`: behind, unfocused).
    /// The no-image window goes; past [`MAX_DOCS`] the oldest close.
    fn insert_doc(&mut self, doc: Doc, background: bool) -> Task<Msg> {
        let mut tasks = Vec::new();
        if let Some(empty) = self.windows.of_doc(EMPTY) {
            self.windows.forget(empty);
            tasks.push(window::close(empty));
        }
        let (window, open) = self
            .windows
            .open(APP_ID, doc.id, doc.path.as_deref(), background);
        tasks.push(open.discard());
        self.docs.insert(0, doc);
        while self.docs.len() > MAX_DOCS {
            let Some(oldest) = self.docs.pop() else {
                break;
            };
            if let Some(w) = self.windows.of_doc(oldest.id) {
                self.windows.forget(w);
                tasks.push(window::close(w));
            }
        }
        if !background {
            self.focus(window);
        }
        self.invalidate_stage();
        self.status = None;
        Task::batch(tasks)
    }

    /// With no window open, the no-image window (it offers Open).
    fn ensure_window(&mut self) -> Task<Msg> {
        if !self.windows.is_empty() {
            return Task::none();
        }
        let (_, open) = self.windows.open(APP_ID, EMPTY, None, false);
        open.discard()
    }

    /// Bring image `id`'s window up and act on it.
    fn show_doc(&mut self, id: u64) {
        if let Some(window) = self.windows.of_doc(id) {
            self.focus(window);
            self.windows.show(window);
        }
    }

    /// Sync open — boot only, before iced is pumping frames.
    fn open_path_sync(&mut self, path: PathBuf) -> Task<Msg> {
        if let Some(id) = self.doc_at(&path) {
            self.show_doc(id);
            return Task::none();
        }
        match Doc::load(self.next_id, path.clone()) {
            Ok(doc) => {
                self.next_id += 1;
                tracing::info!(path = %path.display(), "opened");
                let background = !self.docs.is_empty();
                self.insert_doc(doc, background)
            }
            Err(e) => {
                self.status = Some(e);
                Task::none()
            }
        }
    }

    /// The open image at `path`.
    fn doc_at(&self, path: &PathBuf) -> Option<u64> {
        self.docs
            .iter()
            .find(|d| d.path.as_ref() == Some(path))
            .map(|d| d.id)
    }

    /// Decode off the UI thread so Open / MIME / bus opens don't hitch chrome.
    fn open_path(&mut self, path: PathBuf) -> Task<Msg> {
        if let Some(id) = self.doc_at(&path) {
            self.show_doc(id);
            return Task::none();
        }
        let id = self.next_id;
        self.next_id += 1;
        self.status = Some("Opening…".into());
        Task::perform(
            async move {
                match tokio::task::spawn_blocking(move || Doc::load_pixels(id, path)).await {
                    Ok(r) => r,
                    Err(e) => Err(e.to_string()),
                }
            },
            Msg::DocLoaded,
        )
    }

    fn on_loaded(&mut self, result: Result<Loaded, String>) -> Task<Msg> {
        match result {
            Ok(loaded) => {
                if let Some(id) = self.doc_at(&loaded.path) {
                    self.show_doc(id);
                    return Task::none();
                }
                tracing::info!(path = %loaded.path.display(), "opened");
                let open = self.insert_doc(Doc::from_loaded(loaded), false);
                self.persist_session();
                open
            }
            Err(e) => {
                self.set_err(e);
                Task::none()
            }
        }
    }

    fn persist_session(&mut self) {
        if self.restoring {
            return;
        }
        self.restored = true;
        let session = PaintSession {
            paths: self.docs.iter().filter_map(|d| d.path.clone()).collect(),
            selected: self.selected_doc().and_then(|d| d.path.clone()),
        };
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            if let Err(e) = bus.emit(Topic::PaintSession(session)) {
                tracing::warn!(error = %e, "persist paint session failed");
            }
        }
    }

    fn on_paint_session(&mut self, session: PaintSession) -> Task<Msg> {
        if self.restored {
            return Task::none();
        }
        self.restored = true;
        let have: std::collections::HashSet<PathBuf> =
            self.docs.iter().filter_map(|d| d.path.clone()).collect();
        let keep_sel = self.selected.is_some();
        let select = if keep_sel {
            None
        } else {
            session.selected.clone()
        };
        let jobs: Vec<(u64, PathBuf)> = session
            .paths
            .into_iter()
            .filter(|p| !have.contains(p))
            .map(|path| {
                let id = self.next_id;
                self.next_id += 1;
                (id, path)
            })
            .collect();
        if jobs.is_empty() {
            if let Some(id) = select.and_then(|p| self.doc_at(&p)) {
                self.show_doc(id);
            }
            self.persist_session();
            return self.ensure_window();
        }
        self.restoring = true;
        self.status = Some("Opening…".into());
        Task::perform(
            async move {
                match tokio::task::spawn_blocking(move || {
                    jobs.into_iter()
                        .filter_map(|(id, path)| {
                            path.is_file()
                                .then(|| Doc::load_pixels(id, path).ok())
                                .flatten()
                        })
                        .collect::<Vec<_>>()
                })
                .await
                {
                    Ok(loaded) => loaded,
                    Err(e) => {
                        tracing::warn!(error = %e, "paint session restore failed");
                        Vec::new()
                    }
                }
            },
            move |loaded| Msg::SessionLoaded { loaded, select },
        )
    }

    /// The saved session decoded: a window each, the one that was
    /// selected in front (else the first), the rest behind.
    fn on_session_loaded(&mut self, loaded: Vec<Loaded>, select: Option<PathBuf>) -> Task<Msg> {
        let front = select
            .filter(|p| loaded.iter().any(|l| l.path == *p))
            .or_else(|| (self.docs.is_empty()).then(|| loaded.first().map(|l| l.path.clone())).flatten());
        let mut tasks = Vec::new();
        // Oldest first, so the list keeps the saved order (newest first).
        for item in loaded.into_iter().rev() {
            if self.doc_at(&item.path).is_some() {
                continue;
            }
            let background = front.as_ref() != Some(&item.path);
            tasks.push(self.insert_doc(Doc::from_loaded(item), background));
        }
        self.restoring = false;
        self.status = None;
        self.persist_session();
        // Every saved image gone: still a window to open one in.
        tasks.push(self.ensure_window());
        Task::batch(tasks)
    }

    /// Close image `id` and its window. The last window closing quits.
    fn close_doc(&mut self, id: u64) -> Task<Msg> {
        self.docs.retain(|d| d.id != id);
        if self.selected == Some(id) {
            self.selected = None;
        }
        self.cancel_crop();
        let Some(window) = self.windows.of_doc(id) else {
            return Task::none();
        };
        self.close_window(window)
    }

    /// Close `window` (its image is gone, or it is the no-image window).
    /// Another window is acted on from now on; the last one closing quits.
    fn close_window(&mut self, window: window::Id) -> Task<Msg> {
        self.windows.forget(window);
        if let Some(next) = self.windows.focused {
            self.focus(next);
        }
        self.persist_session();
        if self.windows.is_empty() {
            tracing::info!("last paint window closed; quitting");
            return Task::batch([window::close(window), iced::exit()]);
        }
        window::close(window)
    }

    fn cancel_crop(&mut self) {
        if self.cropping {
            self.status = None;
        }
        self.cropping = false;
        self.crop = None;
        self.panning = None;
    }

    fn apply_crop(&mut self) -> Result<(), String> {
        let Some(g) = self.crop else {
            return Err("Draw a crop first".into());
        };
        let doc = self.selected_doc().ok_or("No image open")?;
        let stage = self.stage().map_or(iced::Size::new(800.0, 600.0), |w| w.stage);
        let dest = geom::dest_rect(
            iced::Size::new(doc.pixels.width() as f32, doc.pixels.height() as f32),
            stage,
            doc.zoom,
            doc.pan,
        );
        let sel = geom::norm_rect(g.origin, g.current, dest);
        let (x, y, w, h) = geom::crop_pixels(sel, dest, doc.pixels.width(), doc.pixels.height())
            .ok_or("Crop is too small")?;
        let doc = self.selected_doc_mut().ok_or("No image open")?;
        doc.crop(x, y, w, h)?;
        self.cancel_crop();
        Ok(())
    }

    /// A second launch: bring the focused window up.
    fn raise_self(&self) {
        if let Some(window) = self.windows.focused {
            self.windows.show(window);
        }
    }

    /// The focused window's stage was laid out at `size`, the pointer at
    /// `cursor`.
    fn note_stage(&mut self, cursor: iced::Point, size: iced::Size) {
        if let Some(win) = self.stage_mut() {
            win.stage = size;
            win.cursor = Some(cursor);
        }
    }

    fn apply_zoom(&mut self, cursor: iced::Point, size: iced::Size, factor: f32) {
        self.note_stage(cursor, size);
        let Some(doc) = self.selected_doc_mut() else {
            return;
        };
        let img = iced::Size::new(doc.pixels.width() as f32, doc.pixels.height() as f32);
        let (zoom, pan) = geom::zoom_at(img, size, doc.zoom, doc.pan, cursor, factor);
        doc.zoom = zoom;
        doc.pan = pan;
    }

    fn zoom_from_key(&mut self, factor: f32) {
        let Some((size, cursor)) = self.stage().map(|w| (w.stage, w.cursor)) else {
            return;
        };
        let cursor = cursor.unwrap_or(iced::Point::new(size.width * 0.5, size.height * 0.5));
        self.apply_zoom(cursor, size, factor);
    }

    fn zoom_fit(&mut self) {
        if let Some(doc) = self.selected_doc_mut() {
            doc.reset_view();
        }
        self.panning = None;
    }

    fn with_doc<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Doc),
    {
        if let Some(doc) = self.selected_doc_mut() {
            f(doc);
            self.status = None;
        }
    }

    /// Make the focused image the desktop's background: Sola keeps a copy
    /// (`sola_core::background`) and sola-river draws it.
    fn set_background(&mut self) {
        let Some(doc) = self.selected_doc() else {
            self.set_err("No image open");
            return;
        };
        let path = match doc.background_copy() {
            Ok(path) => path,
            Err(e) => {
                tracing::warn!(error = %e, "set as background failed");
                self.set_err(e);
                return;
            }
        };
        let set_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let topic = Topic::Background(sola_bus::topics::Background {
            path: path.to_string_lossy().into_owned(),
            set_at,
        });
        let sent = sola_kit::app::bus()
            .lock()
            .map_err(|_| "bus unavailable".to_string())
            .and_then(|mut bus| bus.emit(topic).map_err(|e| e.to_string()));
        match sent {
            Ok(()) => self.set_ok("Set as background"),
            Err(e) => {
                tracing::warn!(error = %e, "emit background failed");
                self.set_err(format!("Could not set the background: {e}"));
            }
        }
    }

    fn set_ok(&mut self, msg: impl Into<String>) {
        self.status = Some(msg.into());
    }

    fn set_err(&mut self, msg: impl Into<String>) {
        self.status = Some(msg.into());
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => {
                self.float.update(&message);
                self.windows.update(&message);
                // The tab bar's stoplight: as the window's own close.
                if let Some(window) = self.windows.close_request(&message) {
                    return self.update(Msg::Window(window, window::Event::CloseRequested));
                }
                if apply_theme_update(&message, &mut self.theme) {
                    for win in self.windows.iter_mut() {
                        win.cache.clear();
                    }
                }
                if is_self_quit(&message, APP_ID) {
                    return iced::exit();
                }
                match Topic::parse(&message) {
                    Some(Topic::PaintSession(session)) => {
                        return self.on_paint_session(session);
                    }
                    Some(Topic::OpenImage(req)) if req.for_app(APP_ID) => {
                        tracing::info!(
                            path = %req.path.display(),
                            activate = req.activate,
                            "OpenImage"
                        );
                        if req.activate {
                            self.raise_self();
                        }
                        if !req.path.as_os_str().is_empty() {
                            return self.open_path(req.path);
                        }
                    }
                    Some(Topic::MenuAction(p)) if p.app_id == APP_ID => {
                        return self.on_menu(&p.action_id);
                    }
                    _ => {}
                }
            }
            Msg::At(window, msg) => {
                self.focus(window);
                return self.update(*msg);
            }
            Msg::Window(window, event) => match event {
                window::Event::Focused => self.focus(window),
                window::Event::CloseRequested => {
                    return match self.windows.doc_of(window) {
                        Some(doc) if doc != EMPTY => self.close_doc(doc),
                        _ => self.close_window(window),
                    };
                }
                window::Event::Closed => {
                    // Closed under us (not through close_window).
                    if let Some(doc) = self.windows.forget(window) {
                        self.docs.retain(|d| d.id != doc);
                        self.persist_session();
                    }
                    if self.windows.is_empty() {
                        return iced::exit();
                    }
                }
                _ => {}
            },
            Msg::BootSettled => {
                if !self.restoring {
                    return self.ensure_window();
                }
            }
            Msg::DocLoaded(result) => return self.on_loaded(result),
            Msg::SessionLoaded { loaded, select } => {
                return self.on_session_loaded(loaded, select);
            }

            Msg::OpenDialog => self.open_picker(),
            Msg::SaveAsDialog => self.save_picker(),
            Msg::Picker(m) => return self.on_picker(m),
            Msg::Save => match self.selected_doc_mut() {
                Some(doc) if doc.path.is_some() => match doc.save() {
                    Ok(()) => self.set_ok("Saved"),
                    Err(e) => self.set_err(e),
                },
                Some(_) => {
                    return self.update(Msg::SaveAsDialog);
                }
                None => self.set_err("No image open"),
            },
            Msg::ToggleCrop => {
                if self.selected_doc().is_none() {
                    self.set_err("Open an image first");
                } else if self.cropping {
                    self.cancel_crop();
                    self.invalidate_stage();
                } else {
                    self.cropping = true;
                    self.crop = None;
                    self.status = Some("Drag to crop · Enter applies · Esc cancels".into());
                    self.invalidate_stage();
                }
            }
            Msg::StagePress(pt, size) => {
                self.note_stage(pt, size);
                if self.cropping {
                    self.panning = None;
                    self.crop = Some(CropGesture {
                        origin: pt,
                        current: pt,
                    });
                } else if let Some(doc) = self.selected_doc() {
                    self.panning = Some((pt, doc.pan));
                }
            }
            Msg::StageMove(pt, size) => {
                self.note_stage(pt, size);
                if let Some(g) = self.crop.as_mut() {
                    g.current = pt;
                    self.invalidate_stage();
                } else if let Some((origin, start_pan)) = self.panning {
                    if let Some(doc) = self.selected_doc_mut() {
                        let img =
                            iced::Size::new(doc.pixels.width() as f32, doc.pixels.height() as f32);
                        let pan = iced::Vector::new(
                            start_pan.x + (pt.x - origin.x),
                            start_pan.y + (pt.y - origin.y),
                        );
                        doc.pan = geom::clamp_pan(img, size, doc.zoom, pan);
                        self.invalidate_stage();
                    }
                }
            }
            Msg::StageRelease => {
                self.panning = None;
            }
            Msg::ZoomAt {
                cursor,
                size,
                factor,
            } => {
                self.apply_zoom(cursor, size, factor);
                self.invalidate_stage();
            }
            Msg::ZoomFit => {
                self.zoom_fit();
                self.invalidate_stage();
            }
            Msg::ZoomIn => {
                self.zoom_from_key(geom::zoom_factor(1.0));
                self.invalidate_stage();
            }
            Msg::ZoomOut => {
                self.zoom_from_key(geom::zoom_factor(-1.0));
                self.invalidate_stage();
            }
            Msg::ApplyCrop => {
                if let Err(e) = self.apply_crop() {
                    self.set_err(e);
                } else {
                    self.invalidate_stage();
                }
            }
            Msg::CancelCrop => {
                self.cancel_crop();
                self.invalidate_stage();
            }
            Msg::RotateCw => {
                self.with_doc(|d| d.rotate_cw());
                self.invalidate_stage();
            }
            Msg::RotateCcw => {
                self.with_doc(|d| d.rotate_ccw());
                self.invalidate_stage();
            }
            Msg::FlipH => {
                self.with_doc(|d| d.flip_h());
                self.invalidate_stage();
            }
            Msg::FlipV => {
                self.with_doc(|d| d.flip_v());
                self.invalidate_stage();
            }
            Msg::Undo => {
                if let Some(doc) = self.selected_doc_mut() {
                    if doc.can_undo() {
                        doc.undo();
                        self.status = None;
                        self.invalidate_stage();
                    }
                }
            }
            Msg::KeyPressed(key, mods) => return self.on_key(key, mods),
            Msg::TitleDrag => return sola_kit::drag(self.windows.focused),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.windows.focused, dir),
            Msg::TitleClose => return self.close_focused(),
        }
        Task::none()
    }

    /// Close the focused window (its image, or the no-image window).
    fn close_focused(&mut self) -> Task<Msg> {
        match self.selected {
            Some(id) => self.close_doc(id),
            None => match self.windows.focused {
                Some(window) => self.close_window(window),
                None => Task::none(),
            },
        }
    }

    fn on_menu(&mut self, action: &str) -> Task<Msg> {
        match action {
            "open" => self.update(Msg::OpenDialog),
            "save" => self.update(Msg::Save),
            "save_as" => self.update(Msg::SaveAsDialog),
            "set_background" => {
                self.set_background();
                Task::none()
            }
            "close_tab" => self.close_focused(),
            "undo" => self.update(Msg::Undo),
            "crop" => self.update(Msg::ToggleCrop),
            "rotate_cw" => self.update(Msg::RotateCw),
            "rotate_ccw" => self.update(Msg::RotateCcw),
            "zoom_in" => self.update(Msg::ZoomIn),
            "zoom_out" => self.update(Msg::ZoomOut),
            "zoom_fit" => self.update(Msg::ZoomFit),
            _ => Task::none(),
        }
    }

    fn on_key(&mut self, key: keyboard::Key, mods: keyboard::Modifiers) -> Task<Msg> {
        if matches!(key, keyboard::Key::Named(NamedKey::Escape)) {
            if self.picker.is_some() {
                return self.on_picker(PickerMsg::Cancel);
            }
            if self.cropping {
                self.cancel_crop();
                return Task::none();
            }
        }
        if matches!(key, keyboard::Key::Named(NamedKey::Enter)) {
            if self.picker.is_some() {
                return self.on_picker(PickerMsg::Confirm);
            }
            if self.cropping {
                return self.update(Msg::ApplyCrop);
            }
        }
        if !mods.command() {
            return Task::none();
        }
        match key.as_ref() {
            keyboard::Key::Character("o") => self.update(Msg::OpenDialog),
            keyboard::Key::Character("s") if mods.shift() => self.update(Msg::SaveAsDialog),
            keyboard::Key::Character("s") => self.update(Msg::Save),
            keyboard::Key::Character("w") => self.close_focused(),
            keyboard::Key::Character("z") => self.update(Msg::Undo),
            keyboard::Key::Character("k") if mods.shift() => self.update(Msg::ToggleCrop),
            keyboard::Key::Character("r") if mods.shift() => self.update(Msg::RotateCcw),
            keyboard::Key::Character("r") => self.update(Msg::RotateCw),
            keyboard::Key::Character("0") => self.update(Msg::ZoomFit),
            keyboard::Key::Character("=") | keyboard::Key::Character("+") => {
                self.update(Msg::ZoomIn)
            }
            keyboard::Key::Character("-") => self.update(Msg::ZoomOut),
            _ => Task::none(),
        }
    }

    fn start_dir(&self) -> PathBuf {
        self.last_dir
            .clone()
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Pictures")))
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    fn open_picker(&mut self) {
        let picker = FilePicker::open()
            .title("Open image")
            .filter("Images", sola_core::open_image::IMAGE_EXTENSIONS)
            .start_dir(self.start_dir());
        if let Some(window) = self.windows.focused {
            self.picker = Some((PickerKind::Open, picker, window));
        }
    }

    fn save_picker(&mut self) {
        let seed = self
            .selected_doc()
            .and_then(|d| d.path.as_ref())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled.png".into());
        let dir = self
            .selected_doc()
            .and_then(|d| d.path.as_ref())
            .and_then(|p| p.parent())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.start_dir());
        let picker = FilePicker::save()
            .title("Save image")
            .filter("Images", sola_core::open_image::IMAGE_EXTENSIONS)
            .start_dir(dir)
            .suggested_name(seed);
        if let Some(window) = self.windows.focused {
            self.picker = Some((PickerKind::SaveAs, picker, window));
        }
    }

    fn on_picker(&mut self, msg: PickerMsg) -> Task<Msg> {
        let Some((kind, picker, _)) = self.picker.as_mut() else {
            return Task::none();
        };
        let kind = *kind;
        match picker.update(msg) {
            Some(Outcome::Cancelled) => {
                self.picker = None;
            }
            Some(Outcome::Picked(path)) => {
                if let Some(parent) = path.parent() {
                    self.last_dir = Some(parent.to_path_buf());
                }
                self.picker = None;
                match kind {
                    PickerKind::Open => return self.open_path(path),
                    PickerKind::SaveAs => match self.selected_doc_mut() {
                        Some(doc) => match doc.save_to(&path) {
                            Ok(()) => {
                                self.set_ok("Saved");
                                self.persist_session();
                            }
                            Err(e) => self.set_err(e),
                        },
                        None => self.set_err("No image open"),
                    },
                }
            }
            None => {}
        }
        Task::none()
    }

    /// An image's window: the tool strip and its stage (or, with no image,
    /// the way to open one). Every message from it acts on it.
    pub fn view(&self, window: window::Id) -> Element<'_, Msg> {
        let doc = self
            .windows
            .doc_of(window)
            .and_then(|id| self.docs.iter().find(|d| d.id == id));
        let content = column![self.header_bar(doc), self.body_pane(window, doc)]
            .width(Length::Fill)
            .height(Length::Fill);

        let framed = sola_kit::window_frame(
            self.chrome(window),
            self.title(window),
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            content.into(),
        );

        let view = match self.picker.as_ref().filter(|(_, _, w)| *w == window) {
            Some((_, picker, _)) => {
                iced::widget::stack![framed, picker.overlay().map(Msg::Picker)].into()
            }
            None => framed,
        };
        view.map(move |msg| Msg::At(window, Box::new(msg)))
    }

    fn header_bar<'a>(&'a self, doc: Option<&'a Doc>) -> Element<'a, Msg> {
        let has_doc = doc.is_some();
        let can_undo = doc.is_some_and(|d| d.can_undo());
        // A crop and the status line belong to the image acted on.
        let acting = doc.is_some_and(|d| self.selected == Some(d.id));

        let tools = if self.cropping && acting {
            row![
                kit_btn::labeled_sm("Apply crop", kit_btn::primary).on_press(Msg::ApplyCrop),
                kit_btn::labeled_sm("Cancel", kit_btn::secondary).on_press(Msg::CancelCrop),
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center)
        } else {
            row![
                tool_btn(&self.icons.folder, "Open · ⌘O", Some(Msg::OpenDialog)),
                tool_btn(
                    &self.icons.crop,
                    "Crop · ⌘⇧K",
                    has_doc.then_some(Msg::ToggleCrop),
                ),
                tool_btn(
                    &self.icons.rotate_ccw,
                    "Rotate left · ⌘⇧R",
                    has_doc.then_some(Msg::RotateCcw),
                ),
                tool_btn(
                    &self.icons.rotate_cw,
                    "Rotate right · ⌘R",
                    has_doc.then_some(Msg::RotateCw),
                ),
                tool_btn(
                    &self.icons.flip_h,
                    "Flip horizontal",
                    has_doc.then_some(Msg::FlipH),
                ),
                tool_btn(
                    &self.icons.flip_v,
                    "Flip vertical",
                    has_doc.then_some(Msg::FlipV),
                ),
                tool_btn(&self.icons.undo, "Undo · ⌘Z", can_undo.then_some(Msg::Undo)),
                tool_btn(&self.icons.save, "Save · ⌘S", has_doc.then_some(Msg::Save)),
            ]
            .spacing(SPACE_XS_LOCAL)
            .align_y(Alignment::Center)
        };

        let meta: Element<'_, Msg> = match doc {
            Some(doc) => {
                let title = text(doc.label()).font(fonts::ui_medium()).size(14);
                let sub = text(format!("{} · {}", doc.dims_label(), doc.zoom_label()))
                    .size(11)
                    .style(kit_text::muted);
                column![title, sub]
                    .spacing(SPACE_SM)
                    .width(Length::Fill)
                    .into()
            }
            None => column![
                text("Paint").font(fonts::ui_medium()).size(14),
                text("No image open").size(11).style(kit_text::muted),
            ]
            .spacing(SPACE_SM)
            .width(Length::Fill)
            .into(),
        };

        let status: Element<'_, Msg> = match self.status.as_deref().filter(|_| acting || !has_doc) {
            Some(s) => text(s).size(11).style(kit_text::muted).into(),
            None => Space::new().width(0).into(),
        };

        container(
            row![meta, tools, status]
                .spacing(SPACE_LG)
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .padding(Padding {
                    top: SPACE_MD,
                    right: SPACE_LG,
                    bottom: SPACE_MD,
                    left: SPACE_LG,
                }),
        )
        .width(Length::Fill)
        .height(Length::Fixed(HEADER_H))
        .style(header_style)
        .into()
    }

    fn body_pane<'a>(&'a self, window: window::Id, doc: Option<&'a Doc>) -> Element<'a, Msg> {
        let stage = doc.zip(self.windows.get(window));
        match stage {
            Some((doc, win)) => {
                let acting = self.selected == Some(doc.id);
                stage::view(
                    doc,
                    self.cropping && acting,
                    self.crop.filter(|_| acting),
                    self.panning.is_some() && acting,
                    &self.theme,
                    &win.cache,
                )
            }
            None => container(
                column![
                    kit_text::heading("Paint"),
                    kit_text::body(
                        "Images open here — from the launcher, xdg-open, \
                         or a path. Scroll to zoom; drag to pan."
                    )
                    .style(kit_text::muted),
                    kit_btn::labeled("Open image", kit_btn::primary).on_press(Msg::OpenDialog),
                ]
                .spacing(SPACE_LG)
                .padding(Padding::new(SPACE_XL)),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
        }
    }
}

fn abs_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    }
}

/// Local 6px gap between icon tools — between SM and MD.
const SPACE_XS_LOCAL: f32 = 6.0;

fn tool_btn<'a>(
    handle: &iced::widget::svg::Handle,
    label: &'static str,
    on_press: Option<Msg>,
) -> Element<'a, Msg> {
    let mut btn = toolbar_icon(handle.clone(), 16);
    if let Some(msg) = on_press {
        btn = btn.on_press(msg);
    }
    let tip = container(text(label).font(fonts::ui()).size(12))
        .padding(Padding {
            top: 5.0,
            right: 8.0,
            bottom: 5.0,
            left: 8.0,
        })
        .style(popover::style);
    tooltip(btn, tip, TooltipPosition::Bottom)
        .gap(6)
        .delay(Duration::from_millis(280))
        .into()
}

fn header_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.96,
            ..CHROME_SURFACE
        })),
        border: Border {
            color: mix_white(CHROME_SURFACE, HAIRLINE_A),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}
