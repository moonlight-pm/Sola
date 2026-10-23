//! sola-oath — kit client of `oath --json` (packages first).
//!
//! The shell lists it as **Oath**, only on an Oath machine.

mod catalog;
mod graph;

use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};
use std::sync::Arc;

use iced::event::{self, Event};
use iced::mouse;
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{button, column, container, mouse_area, row, scrollable, stack, toggler, Space};
use iced::{Alignment, Background, Element, Length, Padding, Subscription, Task, Theme};

use sola_bus::topics::SplitDir;
use sola_bus::topics::TopicKind;
use sola_bus::Message;
use sola_core::KeyCode;
use sola_kit::app::{
    apply_theme_update, bus_subscription, is_self_quit, startup, window_settings_transparent,
    BusSetup,
};
use sola_kit::components::button as kit_btn;
use sola_kit::components::divider::DividerColors;
use sola_kit::components::field::field;
use sola_kit::components::form::{form_row, toggle_style};
use sola_kit::components::split::split_with;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{badge, card, Tone};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use catalog::{CatalogError, Pkg};
use graph::ChangeRole;

const APP_ID: &str = "sola-oath";
const LIST_RATIO: f32 = 0.26;
const CHANGE_RATIO: f32 = 0.42;
/// Overlay scrollbar must not cover a control. Kit density, not a second chrome.
const SCROLL_GUTTER: f32 = 16.0;
const ROW_PAD: Padding = Padding {
    top: 8.0,
    bottom: 8.0,
    left: 12.0,
    right: 8.0,
};

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu("Oath", [("quit", "Quit Oath", KeyCode::Q.meta())])
        .install();

    let app = iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings_transparent(APP_ID));
    app.run()
}

struct App {
    pkgs: Vec<Pkg>,
    /// Overlay of Present toggles not yet applied (`id` → present).
    staged: BTreeMap<String, bool>,
    /// Packs the operator started a change tree from. Nested unused
    /// toggles do not steal the root.
    anchors: BTreeSet<String>,
    selected: Option<String>,
    query: String,
    loading: bool,
    busy: Busy,
    error: Option<String>,
    notice: Option<String>,
    theme: Theme,
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
    window_w: f32,
    list_ratio: f32,
    change_ratio: f32,
    dragging: Drag,
    last_cursor: Option<(f32, f32)>,
    drag_anchor: Option<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Busy {
    #[default]
    Idle,
    Apply,
    Undo,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Drag {
    #[default]
    None,
    List,
    Change,
}

impl Default for App {
    fn default() -> Self {
        Self {
            pkgs: Vec::new(),
            staged: BTreeMap::new(),
            anchors: BTreeSet::new(),
            selected: None,
            query: String::new(),
            loading: true,
            busy: Busy::Idle,
            error: None,
            notice: None,
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
            window_w: 0.0,
            list_ratio: LIST_RATIO,
            change_ratio: CHANGE_RATIO,
            dragging: Drag::None,
            last_cursor: None,
            drag_anchor: None,
        }
    }
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    Loaded(Result<Vec<Pkg>, String>),
    Select(String),
    Query(String),
    Toggle {
        id: String,
        present: bool,
        anchor: bool,
    },
    Apply,
    Applied(Result<u64, String>),
    Undo,
    Undone(Result<u64, String>),
    Reload,
    Open(String),
    ListSplitPress,
    ChangeSplitPress,
    CursorMoved(f32, f32),
    CursorReleased,
    WindowResized(f32),
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
    BusyIgnore,
}

impl App {
    fn boot() -> (Self, Task<Msg>) {
        (
            Self::default(),
            Task::batch([sola_kit::window_ready_task(Msg::WindowReady), reload()]),
        )
    }

    fn title(&self) -> String {
        "Oath".into()
    }

    fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.is_floating_any(), &self.theme)
    }

    fn is_busy(&self) -> bool {
        self.busy != Busy::Idle
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => {
                self.float.update(&message);
                apply_theme_update(&message, &mut self.theme);
                if is_self_quit(&message, APP_ID) {
                    return iced::exit();
                }
            }
            Msg::Loaded(Ok(pkgs)) => {
                self.pkgs = pkgs;
                self.staged.clear();
                self.anchors.clear();
                self.loading = false;
                self.busy = Busy::Idle;
                self.error = None;
                if let Some(id) = &self.selected {
                    if !self.pkgs.iter().any(|p| &p.id == id) {
                        self.selected = self.pkgs.first().map(|p| p.id.clone());
                    }
                } else {
                    self.selected = self.pkgs.first().map(|p| p.id.clone());
                }
            }
            Msg::Loaded(Err(e)) => {
                self.loading = false;
                self.busy = Busy::Idle;
                self.error = Some(e);
            }
            Msg::Select(id) => {
                if self.is_busy() {
                    return Task::none();
                }
                self.selected = Some(id);
            }
            Msg::Query(q) => {
                if self.is_busy() {
                    return Task::none();
                }
                self.query = q;
            }
            Msg::Toggle {
                id,
                present,
                anchor,
            } => {
                if self.is_busy() {
                    return Task::none();
                }
                match graph::cascade(&self.pkgs, &self.staged, &id, present) {
                    Ok(next) => {
                        if anchor {
                            if next.contains_key(&id) {
                                self.anchors.insert(id.clone());
                            } else {
                                self.anchors.remove(&id);
                            }
                        }
                        self.anchors.retain(|a| next.contains_key(a));
                        if self.anchors.is_empty() {
                            self.anchors.extend(next.keys().cloned());
                        }
                        let extras: Vec<String> =
                            next.keys().filter(|k| *k != &id).cloned().collect();
                        self.staged = next;
                        self.error = None;
                        self.notice = if extras.is_empty() {
                            None
                        } else if present {
                            Some(format!("Also turning on {}.", extras.join(", ")))
                        } else {
                            Some(format!(
                                "{} needs this, so it turns off too.",
                                extras.join(", ")
                            ))
                        };
                    }
                    Err(e) => {
                        self.error = Some(e);
                        self.notice = None;
                    }
                }
            }
            Msg::Apply => {
                if self.is_busy() || self.staged.is_empty() {
                    return Task::none();
                }
                let changes: Vec<(String, bool)> =
                    self.staged.iter().map(|(k, v)| (k.clone(), *v)).collect();
                self.busy = Busy::Apply;
                self.error = None;
                self.notice = None;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || catalog::apply_present(&changes))
                            .await
                            .unwrap_or_else(|e| {
                                Err(catalog::CatalogError {
                                    message: e.to_string(),
                                    hint: None,
                                    confirm: false,
                                })
                            })
                            .map_err(err_string)
                    },
                    Msg::Applied,
                );
            }
            Msg::Applied(Ok(generation)) => {
                self.notice = Some(format!("Applied generation {generation}"));
                self.loading = true;
                return reload();
            }
            Msg::Applied(Err(e)) | Msg::Undone(Err(e)) => {
                self.busy = Busy::Idle;
                self.error = Some(e);
            }
            Msg::Undo => {
                if self.is_busy() {
                    return Task::none();
                }
                self.busy = Busy::Undo;
                self.error = None;
                self.notice = None;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(catalog::undo)
                            .await
                            .unwrap_or_else(|e| {
                                Err(catalog::CatalogError {
                                    message: e.to_string(),
                                    hint: None,
                                    confirm: false,
                                })
                            })
                            .map_err(err_string)
                    },
                    Msg::Undone,
                );
            }
            Msg::Undone(Ok(generation)) => {
                self.notice = Some(format!("Undid to generation {generation}"));
                self.loading = true;
                return reload();
            }
            Msg::Reload => {
                if self.is_busy() {
                    return Task::none();
                }
                self.loading = true;
                self.error = None;
                return reload();
            }
            Msg::Open(url) => {
                if self.is_busy() {
                    return Task::none();
                }
                match open_home(&url) {
                    Ok(()) => {
                        self.error = None;
                        self.notice = Some(format!("Opening {url}"));
                    }
                    Err(e) => {
                        self.error = Some(e);
                        self.notice = None;
                    }
                }
            }
            Msg::WindowReady(id) => {
                self.window_id = id;
                return match id {
                    Some(id) => iced::window::size(id).map(|s| Msg::WindowResized(s.width)),
                    None => Task::none(),
                };
            }
            Msg::WindowResized(w) => self.window_w = w,
            Msg::ListSplitPress => {
                if self.is_busy() {
                    return Task::none();
                }
                self.dragging = Drag::List;
                if let Some((x, _)) = self.last_cursor {
                    self.drag_anchor = Some((x, self.list_ratio));
                }
            }
            Msg::ChangeSplitPress => {
                if self.is_busy() {
                    return Task::none();
                }
                self.dragging = Drag::Change;
                if let Some((x, _)) = self.last_cursor {
                    self.drag_anchor = Some((x, self.change_ratio));
                }
            }
            Msg::CursorMoved(x, y) => {
                self.last_cursor = Some((x, y));
                let win = self.window_w.max(1.0);
                match (self.dragging, self.drag_anchor) {
                    (Drag::List, Some((anchor, ratio))) => {
                        self.list_ratio = (ratio + (x - anchor) / win).clamp(0.16, 0.42);
                    }
                    (Drag::Change, Some((anchor, ratio))) => {
                        let right = (win * (1.0 - self.list_ratio)).max(1.0);
                        self.change_ratio = (ratio + (anchor - x) / right).clamp(0.28, 0.62);
                    }
                    _ => {}
                }
            }
            Msg::CursorReleased => {
                self.dragging = Drag::None;
                self.drag_anchor = None;
            }
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => sola_kit::close_app(APP_ID),
            Msg::BusyIgnore => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        let page_pad = SPACE_XL + SPACE_MD;
        let mut header = row![kit_text::heading("Packages")]
            .spacing(SPACE_MD)
            .align_y(Alignment::Center)
            .width(Length::Fill);
        header = header.push(Space::new().width(Length::Fill));
        if self.busy == Busy::Apply {
            header = header.push(kit_btn::labeled("Applying…", kit_btn::primary));
        } else if !self.staged.is_empty() && !self.is_busy() {
            header = header.push(kit_btn::labeled("Apply", kit_btn::primary).on_press(Msg::Apply));
        } else {
            header = header.push(kit_btn::labeled("Apply", kit_btn::primary));
        }
        if self.is_busy() {
            header = header.push(kit_btn::labeled("Undo", kit_btn::ghost));
            header = header.push(kit_btn::labeled("Reload", kit_btn::ghost));
        } else {
            header = header.push(kit_btn::labeled("Undo", kit_btn::ghost).on_press(Msg::Undo));
            header = header.push(kit_btn::labeled("Reload", kit_btn::ghost).on_press(Msg::Reload));
        }

        let mut stack = column![header].spacing(SPACE_MD).width(Length::Fill);

        if self.busy == Busy::Apply {
            stack = stack.push(
                kit_text::caption("Applying — writing the catalog, then linking /bin.")
                    .style(kit_text::muted),
            );
        } else if self.busy == Busy::Undo {
            stack = stack.push(
                kit_text::caption("Reverting — restoring the last apply.").style(kit_text::muted),
            );
        } else if let Some(err) = self.error.as_deref() {
            stack = stack.push(kit_text::caption(err).style(kit_text::danger));
        } else if let Some(n) = self.notice.as_deref() {
            stack = stack.push(kit_text::caption(n).style(kit_text::success));
        } else if !self.staged.is_empty() {
            let n = self.staged.len();
            let unused = graph::unused_after(&self.pkgs, &self.staged);
            let mut copy = if n == 1 {
                "1 package staged — Apply writes desired, then converges.".to_string()
            } else {
                format!("{n} packages staged — Apply writes desired, then converges.")
            };
            if !unused.is_empty() {
                copy.push_str(&format!(" Nothing else needs {}.", unused.join(", ")));
            }
            stack = stack.push(kit_text::caption(copy).style(kit_text::muted));
        }

        let body: Element<'_, Msg> = if !sola_core::env::on_oath() {
            empty(
                "Oath catalog not found",
                "This app reads /oath. It does nothing off an Oath seat.",
            )
        } else if self.loading && self.pkgs.is_empty() {
            empty("Reading catalog…", "oath ls --kind pkg")
        } else if self.pkgs.is_empty() {
            empty("No packages", "oath ls --kind pkg returned none.")
        } else {
            self.workspace()
        };

        let page = column![stack, body]
            .spacing(page_pad)
            .padding(Padding::new(page_pad))
            .width(Length::Fill)
            .height(Length::Fill);
        let content: Element<'_, Msg> = if self.is_busy() {
            stack![page, self.busy_scrim()].into()
        } else {
            page.into()
        };

        sola_kit::window_frame(
            self.float.chrome_any(),
            "Oath",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            content.into(),
        )
    }

    fn list_card(&self) -> Element<'_, Msg> {
        let visible: Vec<&Pkg> = self
            .pkgs
            .iter()
            .filter(|p| catalog::matches_query(p, &self.query))
            .collect();
        let list_body: Element<'_, Msg> = if visible.is_empty() {
            container(kit_text::caption("No packages match.").style(kit_text::muted))
                .padding(SPACE_MD)
                .width(Length::Fill)
                .into()
        } else {
            let mut rows = column![].spacing(0).width(Length::Fill);
            for pkg in visible {
                rows = rows.push(self.pkg_row(pkg));
            }
            scrollable(container(rows).padding(Padding {
                top: 0.0,
                right: SCROLL_GUTTER,
                bottom: 0.0,
                left: 0.0,
            }))
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
        };
        card(
            column![
                {
                    let input = text_input("Filter", &self.query).width(Length::Fill);
                    if self.is_busy() {
                        input
                    } else {
                        input.on_input(Msg::Query)
                    }
                },
                list_body,
            ]
            .spacing(SPACE_MD)
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn pkg_row<'a>(&'a self, pkg: &'a Pkg) -> Element<'a, Msg> {
        let selected = self.selected.as_deref() == Some(pkg.id.as_str());
        let present = self
            .staged
            .get(&pkg.id)
            .copied()
            .unwrap_or(pkg.desired_present);
        let mut title = row![kit_text::body(catalog::short_name(&pkg.id))]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center);
        if !present {
            title = title.push(badge("off", Tone::Neutral));
        }
        if !pkg.removable {
            title = title.push(badge("req", Tone::Neutral));
        }
        if self.staged.contains_key(&pkg.id) {
            title = title.push(badge("staged", Tone::Accent));
        }
        if pkg.advisories.as_ref().is_some_and(|a| !a.is_empty()) {
            title = title.push(badge("advisory", Tone::Danger));
        }
        let row = button(title)
            .padding(ROW_PAD)
            .width(Length::Fill)
            .style(kit_btn::list_item(selected));
        if self.is_busy() {
            row.into()
        } else {
            row.on_press(Msg::Select(pkg.id.clone())).into()
        }
    }

    fn detail_card(&self) -> Element<'_, Msg> {
        let inner: Element<'_, Msg> = match self
            .selected
            .as_ref()
            .and_then(|id| self.pkgs.iter().find(|p| &p.id == id))
        {
            None => container(
                column![
                    kit_text::subheading("No selection").style(kit_text::muted),
                    kit_text::caption("Select a package.").style(kit_text::muted),
                ]
                .spacing(SPACE_SM)
                .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
            Some(pkg) => {
                let present = self
                    .staged
                    .get(&pkg.id)
                    .copied()
                    .unwrap_or(pkg.desired_present);
                let mut toggle = toggler(present).style(toggle_style);
                if pkg.removable && !self.is_busy() {
                    let id = pkg.id.clone();
                    toggle = toggle.on_toggle(move |present| Msg::Toggle {
                        id: id.clone(),
                        present,
                        anchor: true,
                    });
                }
                let mut chips = row![].spacing(SPACE_SM).align_y(Alignment::Center);
                if pkg.actual_present {
                    chips = chips.push(badge("present", Tone::Success));
                } else {
                    chips = chips.push(badge("absent", Tone::Neutral));
                }
                if !pkg.removable {
                    chips = chips.push(badge("required", Tone::Neutral));
                }
                if !pkg.status.is_empty() && pkg.status != "pkg" {
                    chips = chips.push(badge(pkg.status.clone(), Tone::Neutral));
                }
                if pkg.advisories.as_ref().is_some_and(|a| !a.is_empty()) {
                    chips = chips.push(badge("advisory", Tone::Danger));
                }

                let mut pinned = column![
                    iced::widget::text(pkg.id.clone())
                        .font(fonts::ui_medium())
                        .size(16),
                    chips,
                ]
                .spacing(SPACE_SM)
                .width(Length::Fill);
                if !pkg.description.is_empty() {
                    pinned = pinned.push(
                        kit_text::prose(pkg.description.clone())
                            .wrapping(Wrapping::Word)
                            .line_height(LineHeight::Relative(1.4)),
                    );
                }
                if !pkg.home.is_empty() {
                    let url = pkg.home.clone();
                    pinned = pinned.push(
                        button(kit_text::caption(pkg.home.clone()).style(kit_text::accent))
                            .on_press(Msg::Open(url))
                            .padding(0)
                            .style(kit_btn::ghost),
                    );
                }
                pinned = pinned.push(form_row("Present", toggle));
                pinned = pinned.push(
                    kit_text::caption(if pkg.removable {
                        "Store stays on disk either way."
                    } else {
                        "This pack is not removable."
                    })
                    .style(kit_text::muted),
                );

                let pin_value: Element<'_, Msg> = if pkg.hash.is_empty() {
                    kit_text::caption("unpinned").style(kit_text::muted).into()
                } else {
                    kit_text::code(pkg.hash.clone())
                        .wrapping(Wrapping::Glyph)
                        .line_height(LineHeight::Relative(1.35))
                        .into()
                };
                let links_value: Element<'_, Msg> = if pkg.links.is_empty() {
                    kit_text::caption("none").style(kit_text::muted).into()
                } else {
                    kit_text::body(pkg.links.join("  "))
                        .wrapping(Wrapping::Word)
                        .into()
                };

                let mut rest = column![].spacing(SPACE_LG).width(Length::Fill);
                rest = rest.push(field("Pin", pin_value, None, None));
                rest = rest.push(field("Links", links_value, None, None));
                if !pkg.components.is_empty() {
                    let lines: Vec<String> = pkg
                        .components
                        .iter()
                        .map(|c| format!("{} {}", c.name, c.version))
                        .collect();
                    let up_value: Element<'_, Msg> = kit_text::body(lines.join("  "))
                        .wrapping(Wrapping::Word)
                        .into();
                    rest = rest.push(field("Upstream", up_value, None, None));
                }
                if let Some(advs) = &pkg.advisories {
                    let adv_value: Element<'_, Msg> = if advs.is_empty() {
                        kit_text::caption("none at last fetch")
                            .style(kit_text::muted)
                            .into()
                    } else {
                        let mut col = column![].spacing(SPACE_SM);
                        for a in advs {
                            let label = if a.summary.is_empty() {
                                a.id.clone()
                            } else {
                                format!("{} — {}", a.id, a.summary)
                            };
                            if a.url.starts_with("http://") || a.url.starts_with("https://") {
                                let url = a.url.clone();
                                col = col.push(
                                    button(kit_text::caption(label).style(kit_text::accent))
                                        .on_press(Msg::Open(url))
                                        .padding(0)
                                        .style(kit_btn::ghost),
                                );
                            } else {
                                col = col.push(kit_text::body(label).wrapping(Wrapping::Word));
                            }
                        }
                        col.into()
                    };
                    rest = rest.push(field("Advisories", adv_value, None, None));
                }
                rest = rest.push(self.tree_section(
                    "Needs",
                    graph::needs_tree(&self.pkgs, &pkg.id),
                    false,
                ));
                rest = rest.push(self.tree_section(
                    "Used by",
                    graph::used_by_tree(&self.pkgs, &pkg.id),
                    true,
                ));

                column![
                    pinned,
                    scrollable(container(rest).padding(Padding {
                        top: SPACE_MD,
                        right: SCROLL_GUTTER,
                        bottom: 0.0,
                        left: 0.0,
                    }),)
                    .height(Length::Fill)
                    .width(Length::Fill),
                ]
                .spacing(SPACE_MD)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
        };
        card(inner).width(Length::Fill).height(Length::Fill).into()
    }

    fn busy_scrim(&self) -> Element<'_, Msg> {
        let (title, hint) = match self.busy {
            Busy::Apply => (
                "Applying",
                "Writing the catalog, then linking /bin. This can take a while.",
            ),
            Busy::Undo => (
                "Reverting",
                "Restoring the last apply. This can take a while.",
            ),
            Busy::Idle => ("Working", ""),
        };
        let card_body = column![
            kit_text::subheading(title),
            kit_text::caption(hint).style(kit_text::muted),
        ]
        .spacing(SPACE_SM)
        .align_x(Alignment::Center);
        mouse_area(
            container(card(card_body).width(Length::Shrink))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|theme: &Theme| {
                    let mut c = theme.palette().background;
                    c.a = 0.64;
                    container::Style {
                        background: Some(Background::Color(c)),
                        ..container::Style::default()
                    }
                }),
        )
        .on_press(Msg::BusyIgnore)
        .into()
    }

    fn workspace(&self) -> Element<'_, Msg> {
        let palette = self.theme.extended_palette();
        let line = palette.background.stronger.color;
        let chrome = palette.background.weak.color;
        let colors = DividerColors {
            a: chrome,
            line,
            b: chrome,
        };
        let right: Element<'_, Msg> = if self.staged.is_empty() {
            self.detail_card()
        } else {
            split_with(
                SplitDir::Vertical,
                self.detail_card(),
                (1.0 - self.change_ratio).clamp(0.35, 0.75),
                Msg::ChangeSplitPress,
                self.change_card(),
                colors,
            )
        };
        split_with(
            SplitDir::Vertical,
            self.list_card(),
            self.list_ratio,
            Msg::ListSplitPress,
            right,
            colors,
        )
    }

    fn change_card(&self) -> Element<'_, Msg> {
        let forest = graph::change_forest(&self.pkgs, &self.staged, &self.anchors);
        let mut tree = column![].spacing(SPACE_SM).width(Length::Fill);
        if forest.is_empty() {
            tree = tree.push(kit_text::caption("Nothing staged.").style(kit_text::muted));
        } else {
            for node in &forest {
                tree = tree.push(self.change_node(node, 0, true));
            }
        }
        let mut changing = column![kit_text::caption("Changing").style(kit_text::muted)]
            .spacing(SPACE_SM)
            .width(Length::Fill);
        let mut ids: Vec<String> = self.staged.keys().cloned().collect();
        ids.sort();
        for id in &ids {
            let on = self.staged.get(id).copied().unwrap_or(true);
            let mut row = row![kit_text::body(catalog::short_name(id).to_string())]
                .spacing(SPACE_SM)
                .align_y(Alignment::Center);
            row = row.push(badge(
                if on { "turning on" } else { "turning off" },
                if on { Tone::Success } else { Tone::Neutral },
            ));
            changing = changing.push(
                button(row)
                    .on_press(Msg::Select(id.clone()))
                    .padding(ROW_PAD)
                    .width(Length::Fill)
                    .style(kit_btn::list_item(
                        self.selected.as_deref() == Some(id.as_str()),
                    )),
            );
        }

        card(
            column![
                kit_text::subheading("This apply"),
                changing,
                kit_text::caption("Related").style(kit_text::muted),
                kit_text::caption(
                    "Depends on this: turns off with it. Unused: nothing else needs it, optional."
                )
                .style(kit_text::muted),
                scrollable(container(tree).padding(Padding {
                    top: SPACE_SM,
                    right: SCROLL_GUTTER,
                    bottom: 0.0,
                    left: 0.0,
                }))
                .height(Length::Fill)
                .width(Length::Fill),
            ]
            .spacing(SPACE_MD)
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn change_node(
        &self,
        node: &graph::ChangeNode,
        depth: u32,
        allow_toggle: bool,
    ) -> Element<'_, Msg> {
        let present = self
            .pkgs
            .iter()
            .find(|p| p.id == node.id)
            .map(|p| {
                self.staged
                    .get(&node.id)
                    .copied()
                    .unwrap_or(p.desired_present)
            })
            .unwrap_or(true);
        let name = catalog::short_name(&node.id);
        let mut label = row![kit_text::body(name.to_string())]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center);
        match node.role {
            ChangeRole::RootOff => {
                label = label.push(badge("turning off", Tone::Neutral));
            }
            ChangeRole::RootOn => {
                label = label.push(badge("turning on", Tone::Success));
            }
            ChangeRole::MustOff => {
                label = label.push(badge("depends on this", Tone::Danger));
            }
            ChangeRole::MustOn => {
                label = label.push(badge("required", Tone::Success));
            }
            ChangeRole::CanOff => {
                label = label.push(badge("unused", Tone::Accent));
                if allow_toggle && !self.is_busy() {
                    let id = node.id.clone();
                    label = label.push(toggler(present).style(toggle_style).on_toggle(
                        move |present| Msg::Toggle {
                            id: id.clone(),
                            present,
                            anchor: false,
                        },
                    ));
                }
            }
            ChangeRole::Stays => {
                label = label.push(badge("still used", Tone::Neutral));
            }
        }
        let pad = Padding {
            top: 4.0,
            bottom: 4.0,
            left: 8.0 + depth as f32 * 14.0,
            right: 4.0,
        };
        let ident = container(label).padding(pad).width(Length::Fill);
        let mut col = column![ident].spacing(0).width(Length::Fill);
        let child_toggle =
            matches!(node.role, ChangeRole::CanOff | ChangeRole::RootOff) && !present;
        for child in &node.children {
            col = col.push(self.change_node(child, depth + 1, child_toggle));
        }
        col.into()
    }

    fn tree_section<'a>(
        &'a self,
        title: &'static str,
        rows: Vec<(u32, String, String)>,
        pin_is_of_selected: bool,
    ) -> Element<'a, Msg> {
        let mut col = column![kit_text::caption(title).style(kit_text::muted)]
            .spacing(SPACE_SM)
            .width(Length::Fill);
        if rows.is_empty() {
            col = col.push(kit_text::caption("none").style(kit_text::muted));
        } else {
            for (depth, id, pin) in rows {
                let staged = self.staged.contains_key(&id);
                let pkg = self.pkgs.iter().find(|p| p.id == id);
                let present = pkg
                    .map(|p| self.staged.get(&id).copied().unwrap_or(p.desired_present))
                    .unwrap_or(false);
                let live = if pin_is_of_selected {
                    self.selected
                        .as_ref()
                        .and_then(|s| self.pkgs.iter().find(|p| &p.id == s))
                        .map(|p| p.hash.as_str())
                        .unwrap_or("")
                } else {
                    pkg.map(|p| p.hash.as_str()).unwrap_or("")
                };
                let mut label = row![kit_text::body(id.clone())]
                    .spacing(SPACE_SM)
                    .align_y(Alignment::Center);
                if !pin.is_empty() {
                    label = label.push(
                        kit_text::caption(shorten_hash(&pin))
                            .font(fonts::mono())
                            .style(kit_text::muted),
                    );
                }
                if present {
                    label = label.push(badge("present", Tone::Success));
                }
                if !pin.is_empty() && !live.is_empty() && pin != live {
                    label = label.push(badge("pin mismatch", Tone::Danger));
                }
                if staged {
                    label = label.push(badge("staged", Tone::Accent));
                }
                let pad = Padding {
                    top: 4.0,
                    bottom: 4.0,
                    left: 8.0 + (depth as f32) * 14.0,
                    right: 4.0,
                };
                col = col.push(
                    button(label)
                        .on_press(Msg::Select(id))
                        .padding(pad)
                        .width(Length::Fill)
                        .style(kit_btn::list_item(false)),
                );
            }
        }
        col.into()
    }

    fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            bus_subscription().map(Msg::Bus),
            event::listen_with(|event, _, _| match event {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Msg::CursorMoved(position.x, position.y))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Msg::CursorReleased)
                }
                Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Msg::WindowResized(size.width))
                }
                _ => None,
            }),
        ])
    }
}

fn reload() -> Task<Msg> {
    Task::perform(
        async {
            tokio::task::spawn_blocking(catalog::list_packages)
                .await
                .unwrap_or_else(|e| {
                    Err(CatalogError {
                        message: e.to_string(),
                        hint: None,
                        confirm: false,
                    })
                })
                .map_err(err_string)
        },
        Msg::Loaded,
    )
}

fn err_string(e: CatalogError) -> String {
    match e.hint {
        Some(h) => format!("{}\n{h}", e.message),
        None => e.message,
    }
}

fn empty<'a>(title: &'a str, caption: &'a str) -> Element<'a, Msg> {
    container(
        column![
            kit_text::subheading(title).style(kit_text::muted),
            kit_text::caption(caption).style(kit_text::muted),
        ]
        .spacing(SPACE_SM)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn open_home(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("home is not an http(s) URL".into());
    }
    Command::new("/bin/xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("xdg-open: {e}"))?;
    Ok(())
}

fn shorten_hash(h: &str) -> String {
    let hex = h.strip_prefix("sha256-").unwrap_or(h);
    if hex.len() <= 16 {
        return h.to_string();
    }
    format!("sha256-{}…{}", &hex[..8], &hex[hex.len() - 8..])
}
