//! sola-paint application: tabs of open images + graphite edit stage.

use std::path::PathBuf;

use iced::event;
use iced::keyboard;
use iced::keyboard::key::Named as NamedKey;
use iced::widget::{column, container, row, text, Space};
use iced::{
    Alignment, Background, Border, Color, Element, Event, Length, Padding, Subscription, Task,
    Theme,
};

use sola_bus::topics::{Topic, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{
    apply_theme_update, bus_subscription, is_self_quit, startup, window_settings_transparent,
    BusSetup,
};
use sola_kit::components::button as kit_btn;
use sola_kit::components::icon::icon_handle;
use sola_kit::components::style::{
    mix_white, CHROME_SURFACE, HAIRLINE_A, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL,
};
use sola_kit::components::file_picker::{FilePicker, Message as PickerMsg, Outcome};
use sola_kit::components::text as kit_text;
use sola_kit::components::toolbar::toolbar_icon;
use sola_kit::components::{SidebarDensity, SidebarItem, SidebarPanel, SidebarSection};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use crate::doc::Doc;
use crate::geom;
use crate::stage::{self, CropGesture};
use crate::Msg;

pub const APP_ID: &str = "sola-paint";
const HEADER_H: f32 = 52.0;
const MAX_DOCS: usize = 32;

pub fn run() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu("Paint", [("quit", "Quit Paint", KeyCode::Q.meta())])
        .app_menu(
            "File",
            [
                ("open", "Open…", KeyCode::O.meta()),
                ("save", "Save", KeyCode::S.meta()),
                ("save_as", "Save As…", KeyCode::S.meta().shift()),
                ("close_tab", "Close Tab", KeyCode::W.meta()),
            ],
        )
        .app_menu(
            "Edit",
            [
                ("undo", "Undo", KeyCode::Z.meta()),
                ("crop", "Crop", KeyCode::K.meta()),
                ("rotate_cw", "Rotate Right", KeyCode::R.meta()),
                ("rotate_ccw", "Rotate Left", KeyCode::R.meta().shift()),
            ],
        )
        .install();

    let mut settings = window_settings_transparent(APP_ID);
    settings.size = iced::Size::new(1180.0, 800.0);

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(settings)
        .run()
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
    selected: Option<u64>,
    next_id: u64,
    hovered_tab: Option<String>,
    cropping: bool,
    crop: Option<CropGesture>,
    stage_size: iced::Size,
    picker: Option<(PickerKind, FilePicker)>,
    last_dir: Option<PathBuf>,
    status: Option<String>,
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
    icons: Icons,
}

impl Default for App {
    fn default() -> Self {
        Self {
            docs: Vec::new(),
            selected: None,
            next_id: 1,
            hovered_tab: None,
            cropping: false,
            crop: None,
            stage_size: iced::Size::new(800.0, 600.0),
            picker: None,
            last_dir: None,
            status: None,
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            icons: Icons::new(),
        }
    }
}

impl App {
    pub fn boot() -> (Self, Task<Msg>) {
        let mut app = Self::default();
        for arg in std::env::args().skip(1) {
            if arg.is_empty() {
                continue;
            }
            app.open_path(PathBuf::from(arg));
        }
        (app, sola_kit::window_ready_task(Msg::WindowReady))
    }

    pub fn title(&self) -> String {
        match self.selected_doc() {
            Some(doc) => format!("Paint — {}", doc.label()),
            None => "Paint".into(),
        }
    }

    pub fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    Some(Msg::KeyPressed(key, modifiers))
                }
                _ => None,
            }),
        ])
    }

    fn selected_doc(&self) -> Option<&Doc> {
        let id = self.selected?;
        self.docs.iter().find(|d| d.id == id)
    }

    fn selected_doc_mut(&mut self) -> Option<&mut Doc> {
        let id = self.selected?;
        self.docs.iter_mut().find(|d| d.id == id)
    }

    fn open_path(&mut self, path: PathBuf) {
        if let Some(existing) = self.docs.iter().find(|d| d.path.as_ref() == Some(&path)) {
            self.selected = Some(existing.id);
            self.cancel_crop();
            return;
        }
        match Doc::load(self.next_id, path.clone()) {
            Ok(doc) => {
                self.next_id += 1;
                self.selected = Some(doc.id);
                self.docs.insert(0, doc);
                if self.docs.len() > MAX_DOCS {
                    self.docs.truncate(MAX_DOCS);
                    if let Some(id) = self.selected {
                        if !self.docs.iter().any(|d| d.id == id) {
                            self.selected = self.docs.first().map(|d| d.id);
                        }
                    }
                }
                self.cancel_crop();
                self.status = None;
                tracing::info!(path = %path.display(), "opened");
            }
            Err(e) => {
                self.status = Some(e);
            }
        }
    }

    fn close_tab(&mut self, id: u64) {
        self.docs.retain(|d| d.id != id);
        if self.selected == Some(id) {
            self.selected = self.docs.first().map(|d| d.id);
        }
        self.cancel_crop();
    }

    fn cancel_crop(&mut self) {
        self.cropping = false;
        self.crop = None;
    }

    fn apply_crop(&mut self) -> Result<(), String> {
        let Some(g) = self.crop else {
            return Err("Draw a crop first".into());
        };
        let doc = self.selected_doc().ok_or("No image open")?;
        let dest = geom::contain_rect(
            iced::Size::new(doc.pixels.width() as f32, doc.pixels.height() as f32),
            self.stage_size,
        );
        let sel = geom::norm_rect(g.origin, g.current, dest);
        let (x, y, w, h) = geom::crop_pixels(sel, dest, doc.pixels.width(), doc.pixels.height())
            .ok_or("Crop is too small")?;
        let doc = self.selected_doc_mut().ok_or("No image open")?;
        doc.crop(x, y, w, h)?;
        self.cancel_crop();
        Ok(())
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
                apply_theme_update(&message, &mut self.theme);
                if is_self_quit(&message, APP_ID) {
                    return iced::exit();
                }
                match Topic::parse(&message) {
                    Some(Topic::OpenImage(req)) => {
                        tracing::info!(path = %req.path.display(), "OpenImage");
                        self.open_path(req.path);
                    }
                    Some(Topic::MenuAction(p)) if p.app_id == APP_ID => {
                        return self.on_menu(&p.action_id);
                    }
                    _ => {}
                }
            }
            Msg::Select(id) => {
                self.selected = Some(id);
                self.cancel_crop();
            }
            Msg::Close(id) => self.close_tab(id),
            Msg::HoverTab(id) => self.hovered_tab = id,
            Msg::OpenDialog => self.open_picker(),
            Msg::SaveAsDialog => self.save_picker(),
            Msg::Picker(m) => return self.on_picker(m),
            Msg::Save => {
                match self.selected_doc_mut() {
                    Some(doc) if doc.path.is_some() => match doc.save() {
                        Ok(()) => self.set_ok("Saved"),
                        Err(e) => self.set_err(e),
                    },
                    Some(_) => {
                        return self.update(Msg::SaveAsDialog);
                    }
                    None => self.set_err("No image open"),
                }
            }
            Msg::ToggleCrop => {
                if self.selected_doc().is_none() {
                    self.set_err("Open an image first");
                } else if self.cropping {
                    self.cancel_crop();
                } else {
                    self.cropping = true;
                    self.crop = None;
                    self.status = Some("Drag to crop · Enter applies · Esc cancels".into());
                }
            }
            Msg::CropPress(pt, size) => {
                self.stage_size = size;
                if self.cropping {
                    self.crop = Some(CropGesture {
                        origin: pt,
                        current: pt,
                    });
                }
            }
            Msg::StageMove(pt, size) => {
                self.stage_size = size;
                if let Some(g) = self.crop.as_mut() {
                    g.current = pt;
                }
            }
            Msg::CropRelease => {
                // Keep the selection; Apply commits it.
            }
            Msg::ApplyCrop => {
                if let Err(e) = self.apply_crop() {
                    self.set_err(e);
                }
            }
            Msg::CancelCrop => self.cancel_crop(),
            Msg::RotateCw => self.with_doc(|d| d.rotate_cw()),
            Msg::RotateCcw => self.with_doc(|d| d.rotate_ccw()),
            Msg::FlipH => self.with_doc(|d| d.flip_h()),
            Msg::FlipV => self.with_doc(|d| d.flip_v()),
            Msg::Undo => {
                if let Some(doc) = self.selected_doc_mut() {
                    if doc.can_undo() {
                        doc.undo();
                        self.status = None;
                    }
                }
            }
            Msg::KeyPressed(key, mods) => return self.on_key(key, mods),
            Msg::WindowReady(id) => self.window_id = id,
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => sola_kit::close_app(APP_ID),
        }
        Task::none()
    }

    fn on_menu(&mut self, action: &str) -> Task<Msg> {
        match action {
            "open" => self.update(Msg::OpenDialog),
            "save" => self.update(Msg::Save),
            "save_as" => self.update(Msg::SaveAsDialog),
            "close_tab" => {
                if let Some(id) = self.selected {
                    self.close_tab(id);
                }
                Task::none()
            }
            "undo" => self.update(Msg::Undo),
            "crop" => self.update(Msg::ToggleCrop),
            "rotate_cw" => self.update(Msg::RotateCw),
            "rotate_ccw" => self.update(Msg::RotateCcw),
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
            keyboard::Key::Character("w") => {
                if let Some(id) = self.selected {
                    self.close_tab(id);
                }
                Task::none()
            }
            keyboard::Key::Character("z") => self.update(Msg::Undo),
            keyboard::Key::Character("k") => self.update(Msg::ToggleCrop),
            keyboard::Key::Character("r") if mods.shift() => self.update(Msg::RotateCcw),
            keyboard::Key::Character("r") => self.update(Msg::RotateCw),
            _ => Task::none(),
        }
    }

    fn start_dir(&self) -> PathBuf {
        self.last_dir
            .clone()
            .or_else(|| {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Pictures"))
            })
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    fn open_picker(&mut self) {
        let picker = FilePicker::open()
            .title("Open image")
            .filter("Images", sola_core::open_image::IMAGE_EXTENSIONS)
            .start_dir(self.start_dir());
        self.picker = Some((PickerKind::Open, picker));
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
        self.picker = Some((PickerKind::SaveAs, picker));
    }

    fn on_picker(&mut self, msg: PickerMsg) -> Task<Msg> {
        let Some((kind, picker)) = self.picker.as_mut() else {
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
                    PickerKind::Open => self.open_path(path),
                    PickerKind::SaveAs => match self.selected_doc_mut() {
                        Some(doc) => match doc.save_to(&path) {
                            Ok(()) => self.set_ok("Saved"),
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

    pub fn view(&self) -> Element<'_, Msg> {
        let nav = self.sidebar();
        let main = column![self.header_bar(), self.body_pane()]
            .width(Length::Fill)
            .height(Length::Fill);

        let content: Element<'_, Msg> = row![nav, main]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let framed = sola_kit::wrap_if_floating(
            self.float.is_floating_any(),
            "Paint",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            content,
        );

        if let Some((_, picker)) = self.picker.as_ref() {
            iced::widget::stack![framed, picker.overlay().map(Msg::Picker)].into()
        } else {
            framed
        }
    }

    fn sidebar(&self) -> Element<'_, Msg> {
        let items: Vec<SidebarItem<'_, Msg>> = self
            .docs
            .iter()
            .map(|doc| {
                SidebarItem::new(doc.label(), Msg::Select(doc.id))
                    .active(self.selected == Some(doc.id))
                    .id(doc.id.to_string())
                    .subtitle(doc.dims_label())
                    .on_close(Msg::Close(doc.id))
            })
            .collect();

        let sections = vec![SidebarSection::unlabeled(items).fill()];
        let open = kit_btn::labeled_sm("Open…", kit_btn::secondary).on_press(Msg::OpenDialog);
        let footer = container(open)
            .width(Length::Fill)
            .padding(Padding {
                top: SPACE_SM,
                right: SPACE_MD,
                bottom: SPACE_MD,
                left: SPACE_MD,
            })
            .into();

        SidebarPanel::new(sections)
            .density(SidebarDensity::Large)
            .item_hover(self.hovered_tab.clone(), Msg::HoverTab)
            .footer(footer)
            .build()
    }

    fn header_bar(&self) -> Element<'_, Msg> {
        let has_doc = self.selected_doc().is_some();
        let can_undo = self.selected_doc().is_some_and(|d| d.can_undo());

        let tools = if self.cropping {
            row![
                kit_btn::labeled_sm("Apply crop", kit_btn::primary).on_press(Msg::ApplyCrop),
                kit_btn::labeled_sm("Cancel", kit_btn::secondary).on_press(Msg::CancelCrop),
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center)
        } else {
            row![
                tool_btn(&self.icons.folder, "Open", Some(Msg::OpenDialog)),
                tool_btn(
                    &self.icons.crop,
                    "Crop",
                    has_doc.then_some(Msg::ToggleCrop),
                ),
                tool_btn(
                    &self.icons.rotate_ccw,
                    "Rotate left",
                    has_doc.then_some(Msg::RotateCcw),
                ),
                tool_btn(
                    &self.icons.rotate_cw,
                    "Rotate right",
                    has_doc.then_some(Msg::RotateCw),
                ),
                tool_btn(&self.icons.flip_h, "Flip H", has_doc.then_some(Msg::FlipH)),
                tool_btn(&self.icons.flip_v, "Flip V", has_doc.then_some(Msg::FlipV)),
                tool_btn(&self.icons.undo, "Undo", can_undo.then_some(Msg::Undo)),
                tool_btn(&self.icons.save, "Save", has_doc.then_some(Msg::Save)),
            ]
            .spacing(SPACE_XS_LOCAL)
            .align_y(Alignment::Center)
        };

        let meta: Element<'_, Msg> = match self.selected_doc() {
            Some(doc) => {
                let title = text(doc.label()).font(fonts::ui_medium()).size(14);
                let sub = text(doc.dims_label())
                    .size(11)
                    .style(kit_text::muted);
                column![title, sub].spacing(SPACE_SM).width(Length::Fill).into()
            }
            None => column![
                text("Paint").font(fonts::ui_medium()).size(14),
                text("No image open").size(11).style(kit_text::muted),
            ]
            .spacing(SPACE_SM)
            .width(Length::Fill)
            .into(),
        };

        let status: Element<'_, Msg> = match self.status.as_deref() {
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

    fn body_pane(&self) -> Element<'_, Msg> {
        match self.selected_doc() {
            Some(doc) => stage::view(doc, self.cropping, self.crop, &self.theme),
            None => container(
                column![
                    kit_text::heading("Paint"),
                    kit_text::body(
                        "Images open here — from the launcher, xdg-open, \
                         screenshots, or a path."
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

/// Local 6px gap between icon tools — between SM and MD.
const SPACE_XS_LOCAL: f32 = 6.0;

fn tool_btn<'a>(
    handle: &iced::widget::svg::Handle,
    _label: &'static str,
    on_press: Option<Msg>,
) -> Element<'a, Msg> {
    let mut btn = toolbar_icon(handle.clone(), 16);
    if let Some(msg) = on_press {
        btn = btn.on_press(msg);
    }
    btn.into()
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
