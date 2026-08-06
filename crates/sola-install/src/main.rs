//! sola-install — kit-native installer wizard (visual dogfood build).
//!
//! Product path (freeze): ISO → flower splash → username + disk → install →
//! loginless Sola. This binary is the wizard UI; apply is **dry-run** so it
//! is safe to click through on a dogfood desktop.
//!
//! ```sh
//! cargo make build sola-install
//! target/debug/sola-install
//! ```

mod apply;
mod disks;
mod username;

use std::sync::Arc;
use std::time::Duration;

use iced::widget::{column, container, mouse_area, row, Space};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Subscription, Task, Theme,
};

use sola_bus::Message;
use sola_bus::topics::TopicKind;
use sola_core::KeyCode;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup, window_settings,
};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{
    HAIRLINE_A, RADIUS_LG, RADIUS_MD, RADIUS_XL, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL,
    mix_white,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{field, icon_handle, icon_svg_colored};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

use apply::{HOSTNAME, KEYBOARD, LOCALE, dry_run_steps};
use disks::{Disk, list_disks};

const APP_ID: &str = "sola-install";
const WIN_W: f32 = 920.0;
const WIN_H: f32 = 640.0;
const CARD_W: f32 = 440.0;
const FLOWER_WELCOME: u16 = 88;
const FLOWER_CHROME: u16 = 22;

fn main() -> iced::Result {
    // Install media / cage kiosk: no sola-bus, no sola-river. Kit `startup()`
    // waits up to 20s for River's published socket and would clobber cage's
    // WAYLAND_DISPLAY — that path is a black-screen death spiral under kiosk.
    //
    // Default: standalone. Opt into session dogfood with SOLA_INSTALL_USE_BUS=1.
    let standalone = !bus_enabled();
    if standalone {
        sola_core::log::init(APP_ID);
        tracing::info!("standalone mode (install media / cage) — skip river/bus startup");
        sola_core::env::activate_gpu_env();
        fonts::ensure_system_fonts();
        // Keep WAYLAND_DISPLAY from the compositor that launched us (cage).
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            tracing::warn!("WAYLAND_DISPLAY unset in standalone mode");
        }
    } else {
        startup(APP_ID);
        BusSetup::new(APP_ID)
            .subscribe(TopicKind::ALL)
            .app_menu("Install", [("quit", "Quit Installer", KeyCode::Q.meta())])
            .install();
    }

    let mut settings = window_settings(APP_ID);
    settings.size = iced::Size::new(WIN_W, WIN_H);
    if standalone {
        // Cage is single-client fullscreen; fill the output.
        settings.maximized = true;
        settings.resizable = true;
    } else {
        settings.resizable = false;
    }

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(settings)
        .run()
}

/// True when dogfooding under a live Sola session (bus + river present).
fn bus_enabled() -> bool {
    std::env::var_os("SOLA_INSTALL_USE_BUS").is_some()
        && std::env::var_os("SOLA_INSTALL_STANDALONE").is_none()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Welcome,
    Username,
    Disk,
    Installing,
    Done,
}

struct App {
    step: Step,
    username: String,
    username_error: Option<&'static str>,
    disks: Vec<Disk>,
    selected_disk: Option<usize>,
    /// Index into dry-run progress steps.
    progress_i: usize,
    progress_labels: Vec<&'static str>,
    flower: iced::widget::svg::Handle,
    theme: Theme,
    /// Dry-run banner: never touches real disks in this build.
    dry_run: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            step: Step::Welcome,
            username: String::new(),
            username_error: None,
            disks: list_disks(),
            selected_disk: None,
            progress_i: 0,
            progress_labels: Vec::new(),
            flower: icon_handle("sola/flower"),
            theme: default_theme(),
            dry_run: true,
        }
    }
}

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    Continue,
    Back,
    Username(String),
    SelectDisk(usize),
    StartInstall,
    ProgressTick,
    /// Fake reboot for dogfood — just quit.
    Finish,
}

impl App {
    fn boot() -> (Self, Task<Msg>) {
        let mut app = Self::default();
        if app.disks.len() == 1 {
            app.selected_disk = Some(0);
        }
        (app, Task::none())
    }

    fn title(&self) -> String {
        "Install Sola".into()
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Msg> {
        if bus_enabled() {
            bus_subscription().map(Msg::Bus)
        } else {
            Subscription::none()
        }
    }

    fn selected(&self) -> Option<&Disk> {
        self.selected_disk.and_then(|i| self.disks.get(i))
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => {
                apply_theme_update(&message, &mut self.theme);
                if is_self_quit(&message, APP_ID) {
                    return iced::exit();
                }
            }
            Msg::Continue => match self.step {
                Step::Welcome => self.step = Step::Username,
                Step::Username => {
                    if let Some(err) = username::validate(&self.username) {
                        self.username_error = Some(err);
                    } else {
                        self.username_error = None;
                        self.step = Step::Disk;
                    }
                }
                Step::Disk | Step::Installing | Step::Done => {}
            },
            Msg::Back => match self.step {
                Step::Username => self.step = Step::Welcome,
                Step::Disk => self.step = Step::Username,
                _ => {}
            },
            Msg::Username(s) => {
                // Keep the field tidy: lowercase as they type.
                self.username = s.to_ascii_lowercase();
                self.username_error = None;
            }
            Msg::SelectDisk(i) => {
                if i < self.disks.len() {
                    self.selected_disk = Some(i);
                }
            }
            Msg::StartInstall => {
                let Some(disk) = self.selected().cloned() else {
                    return Task::none();
                };
                if username::validate(&self.username).is_some() {
                    self.step = Step::Username;
                    self.username_error = username::validate(&self.username);
                    return Task::none();
                }
                let steps = dry_run_steps(&self.username, &disk.path);
                self.progress_labels = steps.iter().map(|s| s.label).collect();
                self.progress_i = 0;
                self.step = Step::Installing;
                let dwell = steps.first().map(|s| s.dwell).unwrap_or(Duration::from_millis(500));
                return Task::perform(async move {
                    tokio::time::sleep(dwell).await;
                }, |_| Msg::ProgressTick);
            }
            Msg::ProgressTick => {
                let steps = dry_run_steps(
                    &self.username,
                    self.selected().map(|d| d.path.as_str()).unwrap_or("/dev/?"),
                );
                if self.progress_i + 1 >= steps.len() {
                    self.progress_i = steps.len().saturating_sub(1);
                    self.step = Step::Done;
                    return Task::none();
                }
                self.progress_i += 1;
                let dwell = steps[self.progress_i].dwell;
                return Task::perform(async move {
                    tokio::time::sleep(dwell).await;
                }, |_| Msg::ProgressTick);
            }
            Msg::Finish => {
                // Product path reboots; dogfood quits the window.
                return iced::exit();
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        let stage = self.stage_body();
        let chrome = self.top_chrome();

        let content = column![chrome, stage]
            .spacing(0)
            .width(Length::Fill)
            .height(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|theme: &Theme| {
                let p = theme.extended_palette();
                container::Style {
                    background: Some(Background::Color(p.background.base.color)),
                    text_color: Some(p.background.base.text),
                    ..Default::default()
                }
            })
            .into()
    }

    fn top_chrome(&self) -> Element<'_, Msg> {
        let flower = icon_svg_colored(self.flower.clone(), FLOWER_CHROME, Color::WHITE);
        let label = kit_text::caption("Sola").style(kit_text::muted);

        let left = row![flower, label]
            .spacing(SPACE_MD)
            .align_y(Alignment::Center);

        let dry: Element<'_, Msg> = if self.dry_run {
            kit_text::caption("Preview — no disks will be modified")
                .style(kit_text::muted)
                .into()
        } else {
            Space::new().width(0).height(0).into()
        };

        container(
            row![left, Space::new().width(Length::Fill), dry]
                .align_y(Alignment::Center)
                .width(Length::Fill),
        )
        .padding(Padding::from([14, 20]))
        .width(Length::Fill)
        .into()
    }

    fn stage_body(&self) -> Element<'_, Msg> {
        let card = match self.step {
            Step::Welcome => self.view_welcome(),
            Step::Username => self.view_username(),
            Step::Disk => self.view_disk(),
            Step::Installing => self.view_installing(),
            Step::Done => self.view_done(),
        };

        container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(SPACE_XL)
            .into()
    }

    fn card_shell<'a>(&'a self, body: Element<'a, Msg>) -> Element<'a, Msg> {
        container(body)
            .width(Length::Fixed(CARD_W))
            .padding(Padding::from([36, 32]))
            .style(|theme: &Theme| {
                let p = theme.extended_palette();
                let surface = mix_white(p.background.base.color, 0.03);
                container::Style {
                    background: Some(Background::Color(surface)),
                    border: Border {
                        color: mix_white(p.background.base.color, HAIRLINE_A),
                        width: 1.0,
                        radius: RADIUS_XL.into(),
                    },
                    ..Default::default()
                }
            })
            .into()
    }

    fn view_welcome(&self) -> Element<'_, Msg> {
        let flower = icon_svg_colored(self.flower.clone(), FLOWER_WELCOME, Color::WHITE);
        let title = kit_text::heading("Welcome to Sola");
        let sub = kit_text::body(
            "Set up this machine in a couple of steps.\n\
             We’ll create your user, then install and open Sola — no password.",
        )
        .style(kit_text::muted)
        .center();

        let cont = kit_btn::labeled("Continue", kit_btn::primary)
            .on_press(Msg::Continue)
            .width(Length::Fill);

        self.card_shell(
            column![
                container(flower).center_x(Length::Fill),
                Space::new().height(SPACE_XL + SPACE_MD),
                container(title).center_x(Length::Fill),
                Space::new().height(SPACE_MD),
                container(sub).width(Length::Fill).center_x(Length::Fill),
                Space::new().height(SPACE_XL + SPACE_LG),
                cont,
            ]
            .spacing(0)
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .into(),
        )
    }

    fn view_username(&self) -> Element<'_, Msg> {
        let title = kit_text::heading("Choose a username");
        let sub = kit_text::caption(
            "This is who owns the installed system. After install you’ll go \
             straight into Sola as this user — no login screen. Password optional later.",
        )
        .style(kit_text::muted);

        let input = text_input("username", &self.username)
            .on_input(Msg::Username)
            .on_submit(Msg::Continue)
            .padding([10, 12])
            .size(15)
            .width(Length::Fill);

        let field = field(
            "Username",
            input,
            Some("lowercase letters, numbers, _ and -"),
            self.username_error,
        );

        let back = kit_btn::labeled("Back", kit_btn::ghost).on_press(Msg::Back);
        let cont = kit_btn::labeled("Continue", kit_btn::primary)
            .on_press(Msg::Continue)
            .width(Length::Fill);

        let actions = row![back, cont].spacing(SPACE_MD).width(Length::Fill);

        self.card_shell(
            column![title, sub, Space::new().height(SPACE_LG), field, Space::new().height(SPACE_XL), actions]
                .spacing(SPACE_SM)
                .width(Length::Fill)
                .into(),
        )
    }

    fn view_disk(&self) -> Element<'_, Msg> {
        let title = kit_text::heading("Choose a disk");
        let sub = kit_text::caption("Everything on the selected disk will be erased.")
            .style(kit_text::muted);

        let mut list = column![].spacing(SPACE_SM).width(Length::Fill);
        for (i, disk) in self.disks.iter().enumerate() {
            list = list.push(self.disk_row(i, disk));
        }

        let warn: Element<'_, Msg> = if self.selected().is_some_and(|d| d.demo) {
            kit_text::caption("Demo disks — install is preview-only on this build.")
                .style(kit_text::warning)
                .into()
        } else if self.dry_run {
            kit_text::caption("Preview build: Erase will simulate install only.")
                .style(kit_text::muted)
                .into()
        } else {
            Space::new().height(0).into()
        };

        let back = kit_btn::labeled("Back", kit_btn::ghost).on_press(Msg::Back);
        let erase = {
            let label = match self.selected() {
                Some(d) => format!("Erase {} and install", d.name),
                None => "Select a disk".into(),
            };
            let btn = kit_btn::labeled(label, kit_btn::danger).width(Length::Fill);
            if self.selected_disk.is_some() {
                btn.on_press(Msg::StartInstall)
            } else {
                btn
            }
        };

        let actions = row![back, erase].spacing(SPACE_MD).width(Length::Fill);

        self.card_shell(
            column![
                title,
                sub,
                Space::new().height(SPACE_LG),
                list,
                Space::new().height(SPACE_MD),
                warn,
                Space::new().height(SPACE_LG),
                actions,
            ]
            .spacing(SPACE_SM)
            .width(Length::Fill)
            .into(),
        )
    }

    fn disk_row(&self, index: usize, disk: &Disk) -> Element<'_, Msg> {
        let selected = self.selected_disk == Some(index);
        let title = kit_text::body(format!("{}  ·  {}", disk.name, disk.size));
        let detail = {
            let mut s = disk.path.clone();
            if !disk.model.is_empty() {
                s = format!("{s}  ·  {}", disk.model);
            }
            if disk.demo {
                s = format!("{s}  ·  demo");
            }
            kit_text::caption(s).style(kit_text::muted)
        };

        let inner = column![title, detail].spacing(2).width(Length::Fill);

        mouse_area(
            container(inner)
                .padding(Padding::from([12, 14]))
                .width(Length::Fill)
                .style(move |theme: &Theme| {
                    let p = theme.extended_palette();
                    let base = p.background.base.color;
                    let bg = if selected {
                        mix_white(base, 0.08)
                    } else {
                        mix_white(base, 0.025)
                    };
                    let border = if selected {
                        p.primary.base.color
                    } else {
                        mix_white(base, HAIRLINE_A)
                    };
                    container::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            color: border,
                            width: if selected { 1.5 } else { 1.0 },
                            radius: RADIUS_MD.into(),
                        },
                        ..Default::default()
                    }
                }),
        )
        .on_press(Msg::SelectDisk(index))
        .into()
    }

    fn view_installing(&self) -> Element<'_, Msg> {
        let title = kit_text::heading("Installing");
        let current = self
            .progress_labels
            .get(self.progress_i)
            .copied()
            .unwrap_or("Working…");
        let status = kit_text::body(current).style(kit_text::muted);

        let total = self.progress_labels.len().max(1) as f32;
        let frac = ((self.progress_i + 1) as f32 / total).clamp(0.05, 1.0);

        let bar = self.progress_bar(frac);

        let meta = kit_text::caption(format!(
            "{}@{} · {} · {} · {}",
            self.username,
            HOSTNAME,
            self.selected().map(|d| d.path.as_str()).unwrap_or("?"),
            LOCALE,
            KEYBOARD
        ))
        .style(kit_text::muted);

        self.card_shell(
            column![
                container(icon_svg_colored(self.flower.clone(), 48, Color::WHITE))
                    .center_x(Length::Fill),
                Space::new().height(SPACE_XL),
                container(title).center_x(Length::Fill),
                Space::new().height(SPACE_MD),
                container(status).center_x(Length::Fill),
                Space::new().height(SPACE_XL),
                bar,
                Space::new().height(SPACE_LG),
                container(meta).center_x(Length::Fill),
            ]
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .into(),
        )
    }

    fn progress_bar(&self, frac: f32) -> Element<'_, Msg> {
        let fill_w = ((frac * 1000.0) as u16).max(1);
        let rest_w = ((1.0 - frac) * 1000.0).max(1.0) as u16;

        let fill: Element<'_, Msg> = container(Space::new().width(Length::Fill).height(6.0))
            .width(Length::FillPortion(fill_w))
            .height(6.0)
            .style(|theme: &Theme| {
                let p = theme.extended_palette();
                container::Style {
                    background: Some(Background::Color(p.primary.base.color)),
                    border: Border {
                        radius: RADIUS_LG.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .into();

        let rest: Element<'_, Msg> = Space::new()
            .width(Length::FillPortion(rest_w))
            .height(6.0)
            .into();

        container(row![fill, rest].width(Length::Fill).height(6.0))
            .width(Length::Fill)
            .style(|theme: &Theme| {
                let p = theme.extended_palette();
                container::Style {
                    background: Some(Background::Color(mix_white(p.background.base.color, 0.08))),
                    border: Border {
                        radius: RADIUS_LG.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .into()
    }

    fn view_done(&self) -> Element<'_, Msg> {
        let flower = icon_svg_colored(self.flower.clone(), 64, Color::WHITE);
        let title = kit_text::heading("You're ready");
        let sub = kit_text::body(format!(
            "Sola is set up for “{}”.\nOn real media this would reboot into your desktop.",
            self.username
        ))
        .style(kit_text::muted)
        .center();

        let done = kit_btn::labeled("Close preview", kit_btn::primary)
            .on_press(Msg::Finish)
            .width(Length::Fill);

        self.card_shell(
            column![
                container(flower).center_x(Length::Fill),
                Space::new().height(SPACE_XL),
                container(title).center_x(Length::Fill),
                Space::new().height(SPACE_MD),
                container(sub).width(Length::Fill).center_x(Length::Fill),
                Space::new().height(SPACE_XL + SPACE_MD),
                done,
            ]
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .into(),
        )
    }
}
