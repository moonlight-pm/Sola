//! sola-settings — iced port. Single window, sidebar + main pane.
//! Panels: Applications, Mail, Calendar. State is bus-replayed: the
//! sticky `Application`, `MailConfig`, and `CalendarConfig` topics
//! seed our view on connect and re-sync on every external edit.
//!
//! Zoned: content-only. Floating: kit titlebar + rounded frame.

use std::sync::Arc;

use iced::widget::{column, container, row};
use iced::{Element, Length, Padding, Subscription, Task, Theme};

use sola_bus::Message;
use sola_bus::topics::{
    ApplicationsConfig, CalendarConfig, MailConfig, Topic, TopicKind, Window as BusWindow,
};
use sola_core::KeyCode;
use sola_kit::app::{
    BusSetup, apply_theme_update, bus_subscription, is_self_quit, startup,
    window_settings_transparent,
};
use sola_kit::components::style::{SPACE_MD, SPACE_XL};
use sola_kit::components::text as kit_text;
use sola_kit::components::{SidebarItem, SidebarSection, sidebar};
use sola_kit::fonts;
use sola_kit::theme::default_theme;

mod applications;
mod appearance;
mod calendar;
mod calendar_oauth;
mod edit;
mod keybindings;
mod login;
mod mail;
mod mail_discover;
mod mail_from_api;
mod page;
mod procfs;
mod zones;

use appearance::{AppearanceMsg, AppearanceState};
use login::{LoginMsg, LoginState};
use applications::{AppsMsg, AppsState};
use calendar::{CalMsg, CalendarState};
use keybindings::{KeysMsg, KeysState};
use zones::{ZonesMsg, ZonesState};
use mail::{MailMsg, MailState};

pub(crate) const APP_ID: &str = "sola-settings";

fn main() -> iced::Result {
    startup(APP_ID);

    BusSetup::new(APP_ID)
        .subscribe(TopicKind::ALL)
        .app_menu("Settings", [("quit", "Quit Settings", KeyCode::Q.meta())])
        .app_menu(
            "Edit",
            [
                ("cut", "Cut", KeyCode::X.meta()),
                ("copy", "Copy", KeyCode::C.meta()),
                ("paste", "Paste", KeyCode::V.meta()),
                ("select_all", "Select All", KeyCode::A.meta()),
            ],
        )
        .install();

    let app = iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .default_font(fonts::ui())
        .window(window_settings_transparent(APP_ID));
    app.run()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Applications,
    Appearance,
    Login,
    Mail,
    Calendar,
    Zones,
    Keybindings,
}

struct App {
    panel: Panel,
    /// Canonical configured launcher entries, replayed from
    /// `Topic::Application` (persistent, keyed by `app_id`).
    applications: ApplicationsConfig,
    /// Canonical mail config, replayed from `Topic::MailConfig`.
    mail: MailConfig,
    /// Canonical calendar accounts, replayed from `Topic::CalendarConfig`.
    calendar: CalendarConfig,
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
    calendar_ui: CalendarState,
    /// The desk's password and auto-login (Oath's login service).
    login_ui: LoginState,
    /// The background picture and the appearance settings.
    appearance: AppearanceState,
    /// Every action's key (Settings → Keybindings).
    keys: KeysState,
    /// The zone layout (Settings → Zones).
    zones: ZonesState,
    /// Float tracker + iced window id for CSD while floating.
    float: sola_kit::FloatState,
    window_id: Option<iced::window::Id>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            panel: Panel::Applications,
            applications: ApplicationsConfig::default(),
            mail: MailConfig::default(),
            calendar: CalendarConfig::default(),
            running: Vec::new(),
            theme: default_theme(),
            apps_ui: AppsState::default(),
            mail_ui: MailState::default(),
            calendar_ui: CalendarState::default(),
            login_ui: LoginState::default(),
            appearance: AppearanceState::default(),
            keys: KeysState::default(),
            zones: ZonesState::default(),
            float: sola_kit::FloatState::new(APP_ID),
            window_id: None,
        }
    }
}

#[derive(Debug, Clone)]
enum Msg {
    BusMessage(Arc<Message>),
    SelectPanel(Panel),
    Apps(AppsMsg),
    Mail(MailMsg),
    Calendar(CalMsg),
    Appearance(AppearanceMsg),
    Login(LoginMsg),
    Keys(KeysMsg),
    Zones(ZonesMsg),
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
    ClipboardPasted(Option<String>),
    EditCopy(Option<iced::widget::Id>),
    EditCut(Option<iced::widget::Id>),
    EditPaste {
        id: Option<iced::widget::Id>,
        text: String,
    },
    EditSelectAll(Option<iced::widget::Id>),
}

impl App {
    fn boot() -> (Self, Task<Msg>) {
        (
            Self::default(),
            sola_kit::window_ready_task(Msg::WindowReady),
        )
    }

    fn title(&self) -> String {
        "Settings".into()
    }

    fn theme(&self) -> Theme {
        sola_kit::theme_for(self.float.chrome_any(), &self.theme)
    }

    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::BusMessage(message) => {
                self.float.update(&message);
                // Live theme reload: Theme + fonts + selection atoms.
                apply_theme_update(&message, &mut self.theme);

                if is_self_quit(&message, APP_ID) {
                    keybindings::leave(&mut self.keys);
                    zones::leave(&mut self.zones);
                    return iced::exit();
                }

                if let Some(Topic::MenuAction(p)) = Topic::parse(&message) {
                    if p.app_id == APP_ID {
                        return self.on_edit_action(&p.action_id);
                    }
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
                        applications::on_apps_changed(&self.applications, &mut self.apps_ui);
                    }
                    Some(Topic::MailConfig(cfg)) => {
                        self.mail = cfg;
                        self.mail_ui.sync_from_canonical(&self.mail);
                    }
                    Some(Topic::CalendarConfig(cfg)) => {
                        merge_calendar_config(&mut self.calendar, cfg);
                        self.calendar_ui.sync_from_canonical(&self.calendar);
                    }
                    Some(Topic::Windows(windows)) => {
                        self.running = windows;
                    }
                    Some(Topic::Background(b)) => {
                        // Persistent: sticky=false is the retract.
                        self.appearance.picture = message.sticky.then_some(b);
                        // A new picture joins the kept ones.
                        if self.panel == Panel::Appearance {
                            return appearance::refresh(&mut self.appearance).map(Msg::Appearance);
                        }
                    }
                    Some(Topic::Appearance(a)) => {
                        self.appearance.look = if message.sticky { a } else { Default::default() };
                        appearance::settle(&mut self.appearance.look);
                    }
                    Some(Topic::Theme(t)) => {
                        self.appearance.theme = Some(t);
                    }
                    Some(Topic::ThemeVariations(v)) => {
                        self.appearance.variations = message.sticky.then_some(v);
                    }
                    Some(Topic::KeyActions(d)) => {
                        let owner = d.owner.clone();
                        self.keys.declared(&owner, message.sticky.then_some(d));
                    }
                    Some(Topic::Keybindings(k)) => {
                        self.keys.keys = if message.sticky { k } else { Default::default() };
                    }
                    Some(Topic::ZoneLayout(l)) => {
                        self.zones.layout = message.sticky.then_some(l);
                    }
                    Some(Topic::OutputGeometry(g)) => {
                        self.zones.output = Some((g.width, g.height));
                    }
                    _ => {}
                }
            }
            Msg::SelectPanel(p) => {
                if p != Panel::Keybindings {
                    keybindings::leave(&mut self.keys);
                }
                if p != Panel::Zones {
                    zones::leave(&mut self.zones);
                }
                self.panel = p;
                if p == Panel::Login {
                    return login::update(LoginMsg::Refresh, &mut self.login_ui).map(Msg::Login);
                }
                if p == Panel::Appearance {
                    return appearance::refresh(&mut self.appearance).map(Msg::Appearance);
                }
            }
            Msg::Login(m) => return login::update(m, &mut self.login_ui).map(Msg::Login),
            Msg::Keys(KeysMsg::OpenZones) => {
                return self.update(Msg::SelectPanel(Panel::Zones));
            }
            Msg::Keys(m) => return keybindings::update(m, &mut self.keys).map(Msg::Keys),
            Msg::Zones(m) => {
                return zones::update(m, &mut self.zones, &mut self.keys).map(Msg::Zones);
            }
            Msg::Apps(m) => {
                return applications::update(m, &mut self.applications, &mut self.apps_ui)
                    .map(Msg::Apps);
            }
            Msg::Mail(m) => {
                return mail::update(m, &mut self.mail, &mut self.mail_ui).map(Msg::Mail);
            }
            Msg::Calendar(m) => {
                return calendar::update(m, &mut self.calendar, &mut self.calendar_ui)
                    .map(Msg::Calendar);
            }
            Msg::Appearance(m) => {
                return appearance::update(m, &mut self.appearance).map(Msg::Appearance);
            }
            Msg::WindowReady(id) => self.window_id = id,
            Msg::TitleDrag => return sola_kit::drag(self.window_id),
            Msg::TitleResize(dir) => return sola_kit::drag_resize(self.window_id, dir),
            Msg::TitleClose => {
                keybindings::leave(&mut self.keys);
                zones::leave(&mut self.zones);
                sola_kit::close_app(APP_ID);
            }
            Msg::ClipboardPasted(text) => {
                let Some(text) = text.filter(|t| !t.is_empty()) else {
                    return Task::none();
                };
                return edit::find_focused_id().map(move |id| Msg::EditPaste {
                    id,
                    text: text.clone(),
                });
            }
            Msg::EditCopy(id) => {
                if let Some(value) = self.focused_value(id.as_ref()) {
                    if !value.is_empty() {
                        return iced::clipboard::write(value);
                    }
                }
            }
            Msg::EditCut(id) => {
                if let Some(value) = self.focused_value(id.as_ref()) {
                    self.set_focused_value(id.as_ref(), String::new());
                    if !value.is_empty() {
                        return iced::clipboard::write(value);
                    }
                }
            }
            Msg::EditPaste { id, text } => {
                self.set_focused_value(id.as_ref(), text);
            }
            Msg::EditSelectAll(id) => {
                if let Some(id) = id {
                    return edit::select_all(id).discard();
                }
            }
        }
        Task::none()
    }

    fn on_edit_action(&self, action_id: &str) -> Task<Msg> {
        match action_id {
            "copy" => edit::find_focused_id().map(Msg::EditCopy),
            "cut" => edit::find_focused_id().map(Msg::EditCut),
            "paste" => iced::clipboard::read().map(Msg::ClipboardPasted),
            "select_all" => edit::find_focused_id().map(Msg::EditSelectAll),
            _ => Task::none(),
        }
    }

    fn focused_value(&self, id: Option<&iced::widget::Id>) -> Option<String> {
        let id = id?;
        mail::focused_value(&self.mail_ui, id)
            .or_else(|| calendar::focused_value(&self.calendar_ui, id))
            .or_else(|| applications::focused_value(&self.apps_ui, id))
    }

    fn set_focused_value(&mut self, id: Option<&iced::widget::Id>, value: String) {
        let Some(id) = id else {
            return;
        };
        if mail::set_focused_value(&mut self.mail_ui, id, &value) {
            return;
        }
        if calendar::set_focused_value(&mut self.calendar_ui, id, &value) {
            return;
        }
        let _ = applications::set_focused_value(&mut self.apps_ui, id, &value);
    }

    fn view(&self) -> Element<'_, Msg> {
        let nav = sidebar(vec![SidebarSection::new(
            "Settings",
            vec![
                SidebarItem::new("Applications", Msg::SelectPanel(Panel::Applications))
                    .active(self.panel == Panel::Applications),
                SidebarItem::new("Appearance", Msg::SelectPanel(Panel::Appearance))
                    .active(self.panel == Panel::Appearance),
                SidebarItem::new("Login", Msg::SelectPanel(Panel::Login))
                    .active(self.panel == Panel::Login),
                SidebarItem::new("Mail", Msg::SelectPanel(Panel::Mail))
                    .active(self.panel == Panel::Mail),
                SidebarItem::new("Calendar", Msg::SelectPanel(Panel::Calendar))
                    .active(self.panel == Panel::Calendar),
                SidebarItem::new("Zones", Msg::SelectPanel(Panel::Zones))
                    .active(self.panel == Panel::Zones),
                SidebarItem::new("Keybindings", Msg::SelectPanel(Panel::Keybindings))
                    .active(self.panel == Panel::Keybindings),
            ],
        )]);

        let title_text = match self.panel {
            Panel::Applications => "Applications",
            Panel::Appearance => "Appearance",
            Panel::Login => "Login",
            Panel::Mail => "Mail",
            Panel::Calendar => "Calendar",
            Panel::Zones => "Zones",
            Panel::Keybindings => "Keybindings",
        };

        // Page pad 24 = SPACE_XL + SPACE_MD (content margin, not control density).
        let page_pad = SPACE_XL + SPACE_MD;
        // Every page scrolls on its own (`page::page`) under its title.
        let main_inner: Element<'_, Msg> = match self.panel {
            Panel::Applications => column![
                kit_text::heading(title_text),
                applications::view(&self.applications, &self.running, &self.apps_ui).map(Msg::Apps),
            ]
            .spacing(page_pad)
            .padding(Padding::new(page_pad))
            .height(Length::Fill)
            .into(),
            Panel::Appearance => column![
                kit_text::heading(title_text),
                appearance::view(&self.appearance).map(Msg::Appearance),
            ]
            .spacing(page_pad)
            .padding(Padding::new(page_pad))
            .height(Length::Fill)
            .into(),
            Panel::Login => column![
                kit_text::heading(title_text),
                login::view(&self.login_ui).map(Msg::Login),
            ]
            .spacing(page_pad)
            .padding(Padding::new(page_pad))
            .height(Length::Fill)
            .into(),
            Panel::Mail => column![
                kit_text::heading(title_text),
                mail::view(&self.mail, &self.mail_ui).map(Msg::Mail),
            ]
            .spacing(page_pad)
            .padding(Padding::new(page_pad))
            .height(Length::Fill)
            .into(),
            Panel::Calendar => column![
                kit_text::heading(title_text),
                calendar::view(&self.calendar, &self.calendar_ui).map(Msg::Calendar),
            ]
            .spacing(page_pad)
            .padding(Padding::new(page_pad))
            .height(Length::Fill)
            .into(),
            Panel::Zones => column![
                kit_text::heading(title_text),
                zones::view(&self.zones, &self.keys).map(Msg::Zones),
            ]
            .spacing(page_pad)
            .padding(Padding::new(page_pad))
            .height(Length::Fill)
            .into(),
            Panel::Keybindings => column![
                kit_text::heading(title_text),
                keybindings::view(&self.keys).map(Msg::Keys),
            ]
            .spacing(page_pad)
            .padding(Padding::new(page_pad))
            .height(Length::Fill)
            .into(),
        };

        let main_pane = container(main_inner)
            .width(Length::Fill)
            .height(Length::Fill);

        // The kit's sidebar is fixed-width; pair it with the main
        // pane in a plain row (no draggable divider — settings has
        // a stable two-pane shape, not the monitor's resizable one).
        let content: Element<'_, Msg> = row![nav, main_pane]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        sola_kit::window_frame(
            self.float.chrome_any(),
            "Settings",
            Msg::TitleDrag,
            Msg::TitleClose,
            Msg::TitleResize,
            content,
        )
    }

    fn subscription(&self) -> Subscription<Msg> {
        let bus = bus_subscription().map(Msg::BusMessage);
        if self.keys.capturing() {
            let keys = keybindings::capture_keys().map(|(p, m)| Msg::Keys(KeysMsg::Pressed(p, m)));
            return Subscription::batch([bus, keys]);
        }
        if self.zones.keying() || self.zones.choosing() {
            let keys = keybindings::capture_keys().map(|(p, m)| Msg::Zones(ZonesMsg::Pressed(p, m)));
            return Subscription::batch([bus, keys]);
        }
        bus
    }
}

/// Accounts-only publishes leave `calendars` empty (keep the last shelf).
/// A shelf publish with empty accounts keeps the last accounts list.
fn merge_calendar_config(dst: &mut CalendarConfig, mut src: CalendarConfig) {
    if src.calendars.is_empty() && !dst.calendars.is_empty() {
        src.calendars = dst.calendars.clone();
    }
    if src.accounts.is_empty() && !src.calendars.is_empty() && !dst.accounts.is_empty() {
        src.accounts = dst.accounts.clone();
    }
    *dst = src;
}
