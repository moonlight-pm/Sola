//! sola-settings — iced port. Single window, sidebar + main pane,
//! two panels: Applications and Mail. State is bus-replayed: the
//! sticky `Application` and `MailConfig` topics seed our view on
//! connect and re-sync on every external edit.
//!
//! Window chrome is off — sola-shell frames + decorates every app
//! itself via its menubar; the app surface is just content.

use std::sync::Arc;

use iced::widget::{column, container, row, scrollable, text};
use iced::{Element, Length, Padding, Subscription, Task, Theme};

use sola_bus::Message;
use sola_bus::topics::{ApplicationsConfig, MailConfig, Topic, TopicKind, Window as BusWindow};
use sola_core::KeyCode;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup, window_settings,
};
use sola_kit::components::{SidebarItem, SidebarSection, sidebar};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

mod applications;
mod mail;
mod procfs;

use applications::{AppsMsg, AppsState};
use mail::{MailMsg, MailState};

const APP_ID: &str = "sola-settings";

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu("Settings", [("quit", "Quit Settings", KeyCode::Q.meta())])
        .install();

    let app = iced::application(App::default, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings(APP_ID));
    app.run()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Applications,
    Mail,
}

struct App {
    panel: Panel,
    /// Canonical configured launcher entries, replayed from
    /// `Topic::Application` (persistent, keyed by `app_id`).
    applications: ApplicationsConfig,
    /// Canonical mail config, replayed from `Topic::MailConfig`.
    mail: MailConfig,
    /// Currently-open windows on the compositor — drives the
    /// "running, not configured" candidate list under Applications.
    running: Vec<BusWindow>,
    /// Live iced theme — replaced on every `Topic::Theme` delivery
    /// via [`apply_theme_update`] (theme + fonts + selection).
    /// Initialized to the kit's default so the first frame renders
    /// before the bus replay arrives.
    theme: Theme,
    /// Per-panel local UI state (drafts, edit buffers, errors).
    apps_ui: AppsState,
    mail_ui: MailState,
}

impl Default for App {
    fn default() -> Self {
        Self {
            panel: Panel::Applications,
            applications: ApplicationsConfig::default(),
            mail: MailConfig::default(),
            running: Vec::new(),
            theme: default_theme(),
            apps_ui: AppsState::default(),
            mail_ui: MailState::default(),
        }
    }
}

#[derive(Debug, Clone)]
enum Msg {
    BusMessage(Arc<Message>),
    SelectPanel(Panel),
    Apps(AppsMsg),
    Mail(MailMsg),
}

impl App {
    fn title(&self) -> String {
        "Sola Settings".into()
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::BusMessage(message) => {
                // Live theme reload: Theme + fonts + selection atoms.
                apply_theme_update(&message, &mut self.theme);

                if is_self_quit(&message, APP_ID) {
                    return iced::exit();
                }

                match Topic::parse(&message) {
                    Some(Topic::Application(app)) => {
                        // Persistent topic: sticky=true is an upsert,
                        // sticky=false is a retract. The host normally
                        // sends the retract form when an entry is
                        // dropped; we treat both shapes idempotently
                        // so a stale duplicate replay can't double-add.
                        self.applications.remove(&app.app_id);
                        if message.sticky {
                            self.applications.apps.push(app);
                        }
                    }
                    Some(Topic::MailConfig(cfg)) => {
                        self.mail = cfg;
                        self.mail_ui.sync_from_canonical(&self.mail);
                    }
                    Some(Topic::Windows(windows)) => {
                        self.running = windows;
                    }
                    _ => {}
                }
            }
            Msg::SelectPanel(p) => self.panel = p,
            Msg::Apps(m) => {
                return applications::update(m, &mut self.applications, &mut self.apps_ui)
                    .map(Msg::Apps);
            }
            Msg::Mail(m) => {
                return mail::update(m, &mut self.mail, &mut self.mail_ui).map(Msg::Mail);
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        let nav = sidebar(vec![SidebarSection::new(
            "Settings",
            vec![
                SidebarItem::new("Applications", Msg::SelectPanel(Panel::Applications))
                    .active(self.panel == Panel::Applications),
                SidebarItem::new("Mail", Msg::SelectPanel(Panel::Mail))
                    .active(self.panel == Panel::Mail),
            ],
        )]);

        let title_text = match self.panel {
            Panel::Applications => "Applications",
            Panel::Mail => "Mail",
        };

        let body: Element<'_, Msg> = match self.panel {
            Panel::Applications => {
                applications::view(&self.applications, &self.running, &self.apps_ui)
                    .map(Msg::Apps)
            }
            Panel::Mail => mail::view(&self.mail, &self.mail_ui).map(Msg::Mail),
        };

        let main_pane = container(
            scrollable(
                column![
                    text(title_text).font(fonts::ui_medium()).size(28),
                    body,
                ]
                .spacing(24)
                .padding(Padding::new(24.0)),
            )
            .height(Length::Fill)
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        // The kit's sidebar is fixed-width; pair it with the main
        // pane in a plain row (no draggable divider — settings has
        // a stable two-pane shape, not the monitor's resizable one).
        row![nav, main_pane]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn subscription(&self) -> Subscription<Msg> {
        bus_subscription().map(Msg::BusMessage)
    }
}

