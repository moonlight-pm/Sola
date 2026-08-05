//! sola-preview — kit image viewer for screenshots (and path opens).
//!
//! Session history in a left sidebar; main pane fits the selected PNG.
//! Shell opens this after Super+Shift+3/4/5; subsequent captures use
//! `Topic::OpenImage` when the process is already running.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::widget::{column, container, image, row, text};
use iced::{Element, Length, Padding, Subscription, Task, Theme};

use sola_bus::Message;
use sola_bus::topics::{Topic, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup, window_settings,
};
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL};
use sola_kit::components::text as kit_text;
use sola_kit::components::{SidebarItem, SidebarSection, sidebar};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

const APP_ID: &str = "sola-preview";
/// Cap in-memory history so a long session can't grow without bound.
const MAX_HISTORY: usize = 64;

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu("Preview", [("quit", "Quit Preview", KeyCode::Q.meta())])
        .install();

    let app = iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings(APP_ID));
    app.run()
}

struct App {
    /// MRU-front list of opened image paths this session.
    history: Vec<PathBuf>,
    /// Currently displayed path (must be in `history` when Some).
    selected: Option<PathBuf>,
    theme: Theme,
}

impl Default for App {
    fn default() -> Self {
        Self {
            history: Vec::new(),
            selected: None,
            theme: default_theme(),
        }
    }
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    Select(PathBuf),
}

impl App {
    fn boot() -> (Self, Task<Msg>) {
        let mut app = Self::default();
        for arg in std::env::args().skip(1) {
            let path = PathBuf::from(arg);
            if path.as_os_str().is_empty() {
                continue;
            }
            app.open_path(path);
        }
        (app, Task::none())
    }

    fn title(&self) -> String {
        match self.selected.as_ref().and_then(|p| p.file_name()) {
            Some(name) => format!("Preview — {}", name.to_string_lossy()),
            None => "Preview".into(),
        }
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Msg> {
        bus_subscription().map(Msg::Bus)
    }

    /// Push `path` to the front of history (dedupe) and select it.
    fn open_path(&mut self, path: PathBuf) {
        self.history.retain(|p| p != &path);
        self.history.insert(0, path.clone());
        if self.history.len() > MAX_HISTORY {
            self.history.truncate(MAX_HISTORY);
        }
        self.selected = Some(path);
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => {
                apply_theme_update(&message, &mut self.theme);

                if is_self_quit(&message, APP_ID) {
                    return iced::exit();
                }

                if let Some(Topic::OpenImage(req)) = Topic::parse(&message) {
                    tracing::info!(path = %req.path.display(), "OpenImage");
                    self.open_path(req.path);
                }
            }
            Msg::Select(path) => {
                if self.history.iter().any(|p| p == &path) {
                    self.selected = Some(path);
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        let items: Vec<SidebarItem<'_, Msg>> = self
            .history
            .iter()
            .map(|path| {
                let label = display_label(path);
                let active = self.selected.as_ref() == Some(path);
                SidebarItem::new(label, Msg::Select(path.clone())).active(active)
            })
            .collect();

        let nav = sidebar(vec![SidebarSection::new("Recent", items)]);

        let body: Element<'_, Msg> = match self.selected.as_ref() {
            Some(path) if path.exists() => {
                let handle = image::Handle::from_path(path.clone());
                container(
                    image(handle)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .content_fit(iced::ContentFit::Contain),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
            }
            Some(path) => container(
                column![
                    kit_text::heading("Missing file"),
                    kit_text::body(path.display().to_string()),
                ]
                .spacing(SPACE_SM)
                .padding(Padding::new(SPACE_XL)),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            None => container(
                column![
                    kit_text::heading("Preview"),
                    kit_text::body(
                        "Screenshots open here automatically. \
                         Or launch with a path: sola-preview /path/to.png",
                    )
                    .style(kit_text::muted),
                ]
                .spacing(SPACE_MD)
                .padding(Padding::new(SPACE_XL)),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        };

        let caption: Element<'_, Msg> = match self.selected.as_ref() {
            Some(path) => container(
                text(path.display().to_string())
                    .font(fonts::mono())
                    .size(12),
            )
            .padding(Padding {
                top: SPACE_SM,
                right: SPACE_LG,
                bottom: SPACE_SM,
                left: SPACE_LG,
            })
            .width(Length::Fill)
            .into(),
            None => text("").into(),
        };

        let main_pane = column![body, caption]
            .width(Length::Fill)
            .height(Length::Fill);

        row![nav, main_pane]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn display_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
