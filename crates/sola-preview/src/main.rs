//! sola-preview — kit image viewer for screenshots (and path opens).
//!
//! Session history in a left sidebar; main pane fits the selected PNG.
//! Shell opens this after Super+Shift+3/4/5; subsequent captures use
//! `Topic::OpenImage` when the process is already running.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iced::widget::{column, container, image, row, text, Space};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Subscription, Task, Theme,
};

use sola_bus::Message;
use sola_bus::topics::{Topic, TopicKind};
use sola_core::KeyCode;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup, window_settings,
};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{
    HAIRLINE_A, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, mix_white,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::{SidebarItem, SidebarSection, sidebar};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

const APP_ID: &str = "sola-preview";
/// Cap in-memory history so a long session can't grow without bound.
const MAX_HISTORY: usize = 64;
/// How long the header button shows “Copied” after a path click.
const COPY_FEEDBACK_MS: u64 = 1500;
/// Top chrome strip height (matches agent/settings toolbar density).
const HEADER_H: f32 = 52.0;

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
    /// Header button shows “Copied” until this token is cleared.
    path_copied: bool,
    /// Bumps on each copy so late clears from earlier clicks are ignored.
    path_copied_gen: u64,
    theme: Theme,
}

impl Default for App {
    fn default() -> Self {
        Self {
            history: Vec::new(),
            selected: None,
            path_copied: false,
            path_copied_gen: 0,
            theme: default_theme(),
        }
    }
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    Select(PathBuf),
    /// Copy the selected image’s absolute path to the clipboard.
    CopyPath,
    /// Dismiss copy feedback if `token` still matches.
    ClearPathCopied(u64),
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
        self.path_copied = false;
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
                    self.path_copied = false;
                }
            }
            Msg::CopyPath => {
                let Some(path) = self.selected.as_ref() else {
                    return Task::none();
                };
                let s = path.display().to_string();
                self.path_copied_gen = self.path_copied_gen.wrapping_add(1);
                let token = self.path_copied_gen;
                self.path_copied = true;
                tracing::debug!(%s, "copied image path");
                return Task::batch([
                    iced::clipboard::write(s),
                    Task::perform(
                        async move {
                            tokio::time::sleep(Duration::from_millis(COPY_FEEDBACK_MS)).await;
                            token
                        },
                        Msg::ClearPathCopied,
                    ),
                ]);
            }
            Msg::ClearPathCopied(token) => {
                if self.path_copied_gen == token {
                    self.path_copied = false;
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

        let header = self.header_bar();
        let body = self.body_pane();

        let main_pane = column![header, body]
            .width(Length::Fill)
            .height(Length::Fill);

        row![nav, main_pane]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Top chrome: filename + path meta on the left, Copy path on the right.
    fn header_bar(&self) -> Element<'_, Msg> {
        let content: Element<'_, Msg> = match self.selected.as_ref() {
            Some(path) => {
                let name = display_label(path);
                let dir = parent_display(path);

                let title = text(name)
                    .font(fonts::ui_medium())
                    .size(14);

                let subtitle: Element<'_, Msg> = if dir.is_empty() {
                    Space::new().height(0.0).into()
                } else {
                    text(dir)
                        .font(fonts::mono())
                        .size(11)
                        .style(kit_text::muted)
                        .into()
                };

                let meta = column![title, subtitle]
                    .spacing(SPACE_SM)
                    .width(Length::Fill);

                let copy_btn = if self.path_copied {
                    kit_btn::labeled_sm("Copied", kit_btn::secondary)
                } else {
                    kit_btn::labeled_sm("Copy path", kit_btn::secondary).on_press(Msg::CopyPath)
                };

                row![meta, copy_btn]
                    .spacing(SPACE_LG)
                    .align_y(Alignment::Center)
                    .width(Length::Fill)
                    .into()
            }
            None => column![
                text("Preview")
                    .font(fonts::ui_medium())
                    .size(14),
                text("No image open")
                    .size(11)
                    .style(kit_text::muted),
            ]
            .spacing(SPACE_SM)
            .into(),
        };

        container(
            container(content)
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
        .center_y(Length::Fixed(HEADER_H))
        .style(header_style)
        .into()
    }

    fn body_pane(&self) -> Element<'_, Msg> {
        match self.selected.as_ref() {
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
        }
    }
}

fn display_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn parent_display(path: &Path) -> String {
    path.parent()
        .map(|p| p.display().to_string())
        .filter(|s| !s.is_empty() && s != ".")
        .unwrap_or_default()
}

/// Raised strip with a soft bottom edge — same family as agent toolbar.
fn header_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let surface = p.background.weaker.color;
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.96,
            ..surface
        })),
        border: Border {
            // Bottom-only hairline: iced can't do per-side, so a full
            // 1px edge on a solid strip reads as a quiet separator.
            color: mix_white(surface, HAIRLINE_A),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}
