//! Web apps (`docs/specs/2026-10-23-web-apps-design.md`): a page the
//! browser hosts as its own app — the app's Wayland app id, name and menus,
//! no location bar — on the browser profile it was wrapped from, so it is
//! already signed in. Browser → Wrap This Page… makes the page one in place;
//! the app's own menu removes it. Each app is an entry in the catalog
//! (`Topic::Application`, kind `web`), which this process both reads and
//! writes.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::widget::{Shader, Space, column, container, mouse_area, row, stack, text, toggler};
use iced::{Alignment, Element, Length, Task, window};
use sola_bus::Message;
use sola_bus::topics::{
    AppKind, AppMenuPayload, AppNotification, Application, MenuDefinition, MenuItem, Topic,
};
use sola_core::KeyCode;
use sola_kit::components::WindowFrame;
use sola_kit::components::button as kit_button;
use sola_kit::components::card;
use sola_kit::components::text_input::text_input;

use super::{App, BLANK_URL, Msg};
use crate::engine::{Cmd, Engine, FrameSlot, TabId};
use crate::integration::{self, EDIT_MENU_ITEMS};

/// The app's own menu.
pub const ACTION_NEW_WINDOW: &str = "web-new-window";
pub const ACTION_OPEN_IN_BROWSER: &str = "web-open-in-browser";
pub const ACTION_CLOSE_WINDOW: &str = "web-close-window";
pub const ACTION_REMOVE: &str = "web-remove";
pub const ACTION_QUIT: &str = "web-quit";

/// How long a restored app page waits for its catalog entry before its
/// window opens anyway, as a browser page (the app went while the browser
/// was off).
const CATALOG_WAIT: Duration = Duration::from_secs(2);

/// What the browser knows of web apps.
#[derive(Default)]
pub struct WebApps {
    /// The catalog's web apps, by app id.
    pub apps: HashMap<String, Application>,
    /// Pages that are a web app's: page → app id.
    pub tabs: HashMap<TabId, String>,
    /// Apps whose menus the shell has (for their shortcuts).
    published: HashSet<String>,
    /// The start URL the engine has for each app page (`Cmd::AppSite`).
    sites_sent: HashMap<TabId, String>,
    /// Restored app pages wait for their app's catalog entry until then.
    hold_until: Option<Instant>,
    /// Each page's latest favicon (PNG): a wrapped app's icon.
    pub favicons: HashMap<TabId, Vec<u8>>,
    /// The Wrap This Page sheet.
    pub wrap: Option<WrapDraft>,
    /// Remove Web App asks once: this app.
    pub removing: Option<String>,
    /// A web app to open once its catalog entry is in and the session is
    /// restored (`--app <id>` started this chrome, or a handoff came early).
    pub launch: Option<String>,
}

/// The Wrap This Page sheet: the page and the name typed.
#[derive(Debug, Clone)]
pub struct WrapDraft {
    pub tab: TabId,
    pub name: String,
    /// The app is to open several windows.
    pub multi_window: bool,
}

impl WebApps {
    /// The app page `tab` is, if it is one.
    pub fn app_of(&self, tab: TabId) -> Option<&Application> {
        self.apps.get(self.tabs.get(&tab)?)
    }

    /// Page `tab`'s window waits: it is an app's whose catalog entry has
    /// not come in yet (a restore beats the catalog).
    pub fn holds(&self, tab: TabId) -> bool {
        self.hold_until.is_some_and(|until| Instant::now() < until)
            && self.tabs.get(&tab).is_some_and(|app| !self.apps.contains_key(app))
    }

    pub fn hosts(&self, app_id: &str) -> bool {
        self.apps.contains_key(app_id)
    }

    /// App `app_id`'s pages.
    /// The app whose group `url` belongs in: an app that opens several
    /// windows, on that site. If two are, the first by name.
    fn group_for(&self, url: &str) -> Option<&Application> {
        self.apps
            .values()
            .filter(|a| a.multi_window)
            .filter(|a| a.url.as_deref().is_some_and(|start| same_site(start, url)))
            .min_by(|a, b| (&a.label, &a.app_id).cmp(&(&b.label, &b.app_id)))
    }

    /// The app a link from page `opener` opens in, as a window of its own:
    /// the opener's, on its site; else an app with a group for that site.
    /// `None`: a browser page.
    fn app_for_link(&self, opener: TabId, url: &str) -> Option<&Application> {
        self.app_of(opener)
            .filter(|a| a.url.as_deref().is_some_and(|start| same_site(start, url)))
            .or_else(|| self.group_for(url))
    }

    pub fn tabs_of(&self, app_id: &str) -> Vec<TabId> {
        self.tabs
            .iter()
            .filter(|(_, a)| *a == app_id)
            .map(|(t, _)| *t)
            .collect()
    }
}

/// Web app `app`'s menus: its own, then Edit.
pub fn app_menu(app: &Application) -> AppMenuPayload {
    let action = |id: &str, label: String, shortcut| MenuItem::Action {
        id: id.into(),
        label,
        shortcut,
        disabled: false,
        checked: false,
    };
    let mut own = Vec::new();
    if app.multi_window {
        own.push(action(ACTION_NEW_WINDOW, "New Window".into(), Some(KeyCode::N.meta())));
        own.push(MenuItem::Divider);
    }
    own.extend([
        action(ACTION_OPEN_IN_BROWSER, "Open in Browser".into(), None),
        action(integration::ACTION_RELOAD, "Reload".into(), Some(KeyCode::R.meta())),
        MenuItem::Divider,
        action(ACTION_CLOSE_WINDOW, "Close Window".into(), Some(KeyCode::W.meta())),
        action(ACTION_REMOVE, "Remove Web App…".into(), None),
        MenuItem::Divider,
        action(ACTION_QUIT, format!("Quit {}", app.label), Some(KeyCode::Q.meta())),
    ]);
    // Undo through Select All: a web app has no find bar.
    let edit = EDIT_MENU_ITEMS
        .iter()
        .take(6)
        .map(|(id, label, chord)| action(id, (*label).into(), Some(*chord)))
        .collect();
    AppMenuPayload {
        app_id: app.app_id.clone(),
        menus: vec![
            MenuDefinition {
                label: app.label.clone(),
                items: own,
            },
            MenuDefinition {
                label: "Edit".into(),
                items: edit,
            },
        ],
    }
}

/// The host of an http(s) URL, lowercased.
fn host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..end].rsplit('@').next()?;
    let host = host.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// `url` is on the same site as `start` (another subdomain counts).
pub fn same_site(start: &str, url: &str) -> bool {
    crate::popup::same_site(start, url)
}

/// A popup is the app's while it has no address yet (a huddle's window opens
/// blank) or one on the app's site; else it is a browser page, its address
/// in sight.
fn popup_is_the_apps(start: &str, url: &str) -> bool {
    crate::popup::is_blank_popup_url(url) || same_site(start, url)
}

/// The profile an app runs on: its own, else (gone) the one showing.
fn app_profile(app: &Application) -> String {
    app.profile
        .clone()
        .filter(|p| crate::profiles::list().iter().any(|q| q.id == *p))
        .unwrap_or_else(crate::profiles::showing_profile)
}

/// A page's title as an app's name: the site's part of it (titles end
/// with the site — "general - Acme - Slack"), else the title, else the
/// host.
pub fn suggested_name(title: &str, url: &str) -> String {
    let title = title.trim();
    // The shortest tail after a separator.
    let name = [" - ", " | ", " — ", " · "]
        .iter()
        .filter_map(|sep| title.rsplit_once(sep).map(|(_, tail)| tail.trim()))
        .filter(|tail| !tail.is_empty())
        .min_by_key(|tail| tail.len())
        .unwrap_or(title);
    if !name.is_empty() {
        return name.to_string();
    }
    host(url).map_or_else(|| "Web App".into(), |h| crate::popup::apex(&h).to_string())
}

/// Where wrapped apps' icons are kept.
fn icon_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    home.join(".local/share/sola/icons")
}

impl<E: Engine> App<E> {
    /// Messages about web apps: the catalog's entries, and menu actions,
    /// close requests and notification clicks addressed to an app this
    /// process hosts. `None` when the message is not about one.
    pub(crate) fn on_web_app_bus(&mut self, message: &Message) -> Option<Task<Msg>> {
        match Topic::parse(message)? {
            Topic::Application(app) if app.kind == AppKind::Web => {
                if message.sticky {
                    tracing::info!(app = %app.app_id, label = %app.label, "web app");
                    self.webapps.published.remove(&app.app_id);
                    let id = app.app_id.clone();
                    // Renamed, or several windows on or off (Settings →
                    // Applications): its open windows' menus follow now.
                    let menus_moved = self.webapps.apps.get(&id).is_some_and(|old| {
                        old.label != app.label || old.multi_window != app.multi_window
                    });
                    if menus_moved && !self.webapps.tabs_of(&id).is_empty() {
                        sola_kit::menu::publish_for(app_menu(&app));
                        self.webapps.published.insert(id.clone());
                    }
                    self.webapps.apps.insert(id.clone(), app);
                    if self.webapps.launch.as_deref() == Some(id.as_str()) {
                        self.open_app(&id);
                    }
                    Some(self.reopen_as_app(&id))
                } else {
                    // Removed (here or elsewhere): its windows close.
                    self.webapps.apps.remove(&app.app_id);
                    Some(self.close_app_windows(&app.app_id))
                }
            }
            Topic::MenuAction(m) if self.webapps.hosts(&m.app_id) => {
                Some(self.on_app_menu(&m.app_id, &m.action_id))
            }
            Topic::CloseApp(app_id) if self.webapps.hosts(&app_id) => {
                Some(self.close_app_windows(&app_id))
            }
            Topic::NotificationActivate(a) if self.webapps.hosts(&a.app_id) => {
                let tab = a.tab_id.map(TabId)?;
                if self.cached_tabs.iter().any(|t| t.id == tab) {
                    self.switch_active_tab(tab);
                    self.windows.show(tab);
                }
                Some(Task::none())
            }
            _ => None,
        }
    }

    /// Open web app `app_id` (`--app`, a handoff, the launcher): bring up
    /// its window, or open its page at its start URL on its profile. Waits
    /// for its catalog entry and the session's pages (its page may be among
    /// them).
    pub(crate) fn open_app(&mut self, app_id: &str) {
        let restored = self.pending_session.is_none();
        let known = self.webapps.apps.get(app_id).cloned();
        let Some(app) = known.filter(|_| restored) else {
            self.webapps.launch = Some(app_id.to_string());
            return;
        };
        self.webapps.launch = None;
        if let Some(tab) = self.webapps.tabs_of(app_id).into_iter().min_by_key(|t| t.0) {
            tracing::info!(app = %app_id, tab = tab.0, "web app up");
            self.switch_active_tab(tab);
            self.windows.show(tab);
            return;
        }
        self.open_app_window(&app.app_id);
    }

    /// A launch handed to this running browser: another window of an app
    /// that opens several, else [`Self::open_app`].
    pub(crate) fn launch_app(&mut self, app_id: &str) {
        let several = self.pending_session.is_none()
            && self.webapps.apps.get(app_id).is_some_and(|a| a.multi_window)
            && !self.webapps.tabs_of(app_id).is_empty();
        if several {
            self.open_app_window(app_id);
        } else {
            self.open_app(app_id);
        }
    }

    /// A new window of app `app_id`, at its start page on its profile.
    fn open_app_window(&mut self, app_id: &str) {
        let Some(app) = self.webapps.apps.get(app_id).cloned() else {
            return;
        };
        let url = app.url.clone().unwrap_or_default();
        let profile = app_profile(&app);
        tracing::info!(app = %app_id, %url, %profile, "web app opens a window");
        let tab = self.open_tab_beside(url, true, &profile);
        self.webapps.tabs.insert(tab, app_id.to_string());
        self.persist_session();
    }

    /// A link from page `opener` opening a window of its own (⌘-click,
    /// `target=_blank`, a web app's link off its site): in the app its site
    /// belongs to ([`WebApps::app_for_link`]), on that app's profile, else
    /// a browser page on the opener's.
    pub(crate) fn open_link(&mut self, opener: TabId, url: String, activate: bool, profile: &str) {
        let Some(app) = self.webapps.app_for_link(opener, &url).cloned() else {
            tracing::info!(%url, activate, "link → browser page");
            self.open_tab_beside(url, activate, profile);
            return;
        };
        tracing::info!(%url, activate, app = %app.app_id, "link → its app");
        // A link within the app is on the page's own profile.
        let own = self.webapps.tabs.get(&opener) == Some(&app.app_id);
        let profile = if own { profile.to_string() } else { app_profile(&app) };
        let tab = self.open_tab_beside(url, activate, &profile);
        self.webapps.tabs.insert(tab, app.app_id);
        self.persist_session();
    }

    /// The page an app's menu acts on: the active page when it is the
    /// app's, else its first.
    fn app_page(&self, app_id: &str) -> Option<TabId> {
        if self.webapps.tabs.get(&self.cached_active).map(String::as_str) == Some(app_id) {
            return Some(self.cached_active);
        }
        self.webapps.tabs_of(app_id).into_iter().min_by_key(|t| t.0)
    }

    fn on_app_menu(&mut self, app_id: &str, action: &str) -> Task<Msg> {
        let tab = self.app_page(app_id);
        match action {
            ACTION_NEW_WINDOW => {
                self.open_app_window(app_id);
                Task::none()
            }
            ACTION_OPEN_IN_BROWSER => {
                if let Some(t) = tab.and_then(|t| self.cached_tabs.iter().find(|i| i.id == t)) {
                    let (url, profile) = (t.url.clone(), self.profile_of(t.id));
                    self.open_tab_beside(url, true, &profile);
                }
                Task::none()
            }
            ACTION_CLOSE_WINDOW => match tab {
                Some(t) => self.update(Msg::CloseTab(t)),
                None => Task::none(),
            },
            ACTION_REMOVE => {
                self.webapps.removing = Some(app_id.to_string());
                Task::none()
            }
            ACTION_QUIT => self.close_app_windows(app_id),
            // Reload and Edit act on the active page: the app's, since its
            // window has the keys.
            other => integration::run_intent(self, integration::intent_for_menu_action(other)),
        }
    }

    /// Close every window of app `app_id` (its pages).
    pub(crate) fn close_app_windows(&mut self, app_id: &str) -> Task<Msg> {
        let tabs = self.webapps.tabs_of(app_id);
        Task::batch(tabs.into_iter().map(|t| self.update(Msg::CloseTab(t))))
    }

    /// After a restore: app pages' windows wait for their apps, so they
    /// open as the app and the shell puts them back in their place. Sync
    /// again when the wait is over.
    pub(crate) fn hold_restored_apps(&mut self) -> Task<Msg> {
        let web = &self.webapps;
        if web.tabs.values().all(|app| web.apps.contains_key(app)) {
            return Task::none();
        }
        self.webapps.hold_until = Some(Instant::now() + CATALOG_WAIT);
        Task::perform(tokio::time::sleep(CATALOG_WAIT), |_| Msg::Noop)
    }

    /// App `app_id`'s pages whose windows opened before the browser knew
    /// the app (its entry came after the wait): they reopen as the app's
    /// on the next sync.
    fn reopen_as_app(&mut self, app_id: &str) -> Task<Msg> {
        let stale: Vec<TabId> = self
            .webapps
            .tabs_of(app_id)
            .into_iter()
            .filter(|t| self.windows.app_id_of(*t).is_some_and(|a| a != app_id))
            .collect();
        Task::batch(stale.into_iter().map(|t| {
            tracing::info!(tab = t.0, app = %app_id, "page window reopens as its app");
            self.windows.close(t).discard()
        }))
    }

    /// Before the window of page `tab` opens: the app id it opens under
    /// (the browser's for a page), the app's menus sent to the shell first.
    pub(crate) fn window_app_id(&mut self, tab: TabId) -> String {
        let Some(app) = self.webapps.app_of(tab).cloned() else {
            return self.app_id.to_string();
        };
        if self.webapps.published.insert(app.app_id.clone()) {
            sola_kit::menu::publish_for(app_menu(&app));
        }
        app.app_id
    }

    /// Tell the engine each app page's start URL, so a link off the app's
    /// site opens as a browser page (once per page, when the app is known).
    pub(crate) fn sync_app_sites(&mut self) {
        let web = &mut self.webapps;
        web.sites_sent.retain(|tab, _| web.tabs.contains_key(tab));
        for (tab, app_id) in &web.tabs {
            let Some(start) = web.apps.get(app_id).and_then(|a| a.url.clone()) else {
                continue;
            };
            if web.sites_sent.get(tab) != Some(&start) {
                web.sites_sent.insert(*tab, start.clone());
                let _ = self.cmd_tx.send(Cmd::AppSite { id: *tab, start });
            }
        }
    }

    /// New pages the engine opened (popups, a huddle's `about:blank`) from
    /// page `opener`: an app's are the app's too.
    pub(crate) fn adopt_popups(&mut self, opener: TabId, new: &[TabId]) {
        let Some(app) = self.webapps.app_of(opener).cloned() else {
            return;
        };
        let start = app.url.clone().unwrap_or_default();
        for tab in new {
            let url = self.cached_tabs.iter().find(|t| t.id == *tab).map_or("", |t| t.url.as_str());
            if popup_is_the_apps(&start, url) {
                tracing::info!(tab = tab.0, app = %app.app_id, "web app popup");
                self.webapps.tabs.insert(*tab, app.app_id.clone());
            } else {
                tracing::info!(tab = tab.0, %url, "popup off the app's site: a browser page");
            }
        }
    }

    /// A notification from page `show.tab_id`: under its app when it is
    /// one's.
    pub(crate) fn notification_for(&self, show: &crate::notify::IpcShow) -> AppNotification {
        match self.webapps.app_of(TabId(show.tab_id)) {
            Some(app) => {
                let mut n = crate::notify::to_bus_for(&app.app_id, show);
                n.source = app.label.clone();
                n
            }
            None => crate::notify::to_bus(show),
        }
    }

    // ── Wrap This Page ────────────────────────────────────────────

    /// Browser → Wrap This Page…: the sheet over the active page, its name
    /// suggested from the page.
    pub(crate) fn open_wrap_sheet(&mut self) -> Task<Msg> {
        let tab = self.cached_active;
        let Some(info) = self.cached_tabs.iter().find(|t| t.id == tab) else {
            return Task::none();
        };
        if self.webapps.tabs.contains_key(&tab) || !can_wrap(&info.url) {
            return Task::none();
        }
        self.webapps.wrap = Some(WrapDraft {
            tab,
            name: suggested_name(&info.title, &info.url),
            multi_window: false,
        });
        iced::widget::operation::focus(wrap_name_id())
    }

    /// Wrap: the page becomes the app in place (same page, no reload) —
    /// the catalog gets the app, its window reopens as the app's.
    pub(crate) fn wrap_page(&mut self) -> Task<Msg> {
        let Some(draft) = self.webapps.wrap.take() else {
            return Task::none();
        };
        let name = draft.name.trim();
        let Some(info) = self.cached_tabs.iter().find(|t| t.id == draft.tab) else {
            return Task::none();
        };
        let app_id = format!("web-{}", crate::profiles::new_uuid());
        let mut app = Application {
            app_id: app_id.clone(),
            label: if name.is_empty() {
                suggested_name(&info.title, &info.url)
            } else {
                name.to_string()
            },
            command: String::new(),
            icon: self.save_icon(&app_id, draft.tab),
            kind: AppKind::Web,
            url: Some(info.url.clone()),
            profile: Some(self.profile_of(draft.tab)),
            multi_window: draft.multi_window,
        };
        app.finalize();
        tracing::info!(app = %app.app_id, label = %app.label, url = ?app.url, "wrapped a page");
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            if let Err(e) = bus.emit(Topic::Application(app.clone())) {
                tracing::warn!(error = %e, "add web app failed");
            }
        }
        self.webapps.apps.insert(app_id.clone(), app);
        self.webapps.tabs.insert(draft.tab, app_id);
        self.persist_session();
        // Its window reopens as the app's on the next sync.
        self.windows.close(draft.tab).discard()
    }

    /// The page's favicon as the app's icon (a file); a globe without one.
    pub(super) fn save_icon(&self, app_id: &str, tab: TabId) -> String {
        let Some(png) = self.webapps.favicons.get(&tab) else {
            return "lucide/globe".into();
        };
        let dir = icon_dir();
        let path = dir.join(format!("{app_id}.png"));
        match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, png)) {
            Ok(()) => path.display().to_string(),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "save web app icon failed");
                "lucide/globe".into()
            }
        }
    }

    /// Remove Web App: out of the catalog (its windows close when the
    /// retraction comes back); its sign-in stays with the profile.
    pub(crate) fn remove_app(&mut self) -> Task<Msg> {
        let Some(app_id) = self.webapps.removing.take() else {
            return Task::none();
        };
        let Some(app) = self.webapps.apps.remove(&app_id) else {
            return Task::none();
        };
        tracing::info!(app = %app_id, "remove web app");
        if let Ok(mut bus) = sola_kit::app::bus().lock() {
            if let Err(e) = bus.retract(Topic::Application(app)) {
                tracing::warn!(error = %e, "remove web app failed");
            }
        }
        if app_id.starts_with("web-") {
            let _ = std::fs::remove_file(icon_dir().join(format!("{app_id}.png")));
        }
        self.close_app_windows(&app_id)
    }

    // ── Views ─────────────────────────────────────────────────────

    /// A web app's window: its page, the page's dialogs while it is the
    /// active one, under a titlebar with the app's name and menus.
    pub(crate) fn view_app(
        &self,
        window: window::Id,
        tab: TabId,
        slot: std::sync::Arc<FrameSlot<E>>,
        app: &Application,
    ) -> Element<'_, Msg> {
        let active = tab == self.cached_active;
        let modal = if !active {
            None
        } else if self.webapps.removing.as_deref() == Some(app.app_id.as_str()) {
            Some(self.view_remove_sheet(app))
        } else {
            self.page_modal()
        };
        let webview = Shader::new(E::make_program(slot.clone()))
            .width(Length::Fill)
            .height(Length::Fill);
        let webview = crate::cef::page_ime::page_ime(webview, slot, active && modal.is_none());
        let canvas = self.theme.extended_palette().background.base.color;
        let canvas = iced::Color { a: 1.0, ..canvas };
        let page: Element<'_, Msg> = container(webview)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_t: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(canvas)),
                ..container::Style::default()
            })
            .into();
        let content = match modal {
            Some(modal) => stack![page, modal].into(),
            None => page,
        };
        WindowFrame::new(self.window_chrome(window), content)
            .title(app.label.clone())
            .on_drag(Msg::WindowDrag(window))
            .on_close(Msg::WindowClose(window))
            .on_resize(move |dir| Msg::WindowResize(window, dir))
            .menus(app_menu(app))
            .into()
    }

    /// The Wrap This Page sheet: the app's name, then Wrap.
    pub(crate) fn view_wrap_sheet(&self) -> Element<'_, Msg> {
        let Some(draft) = &self.webapps.wrap else {
            return Space::new().into();
        };
        let name = text_input("Name", &draft.name)
            .id(wrap_name_id())
            .size(13)
            .style(sola_kit::components::text_input::style)
            .width(Length::Fill)
            .on_input(Msg::WrapName)
            .on_submit(Msg::WrapSubmit);
        let body = column![
            text("Wrap This Page").size(15).font(sola_kit::fonts::ui_medium()),
            muted_line(
                "It opens as its own app — in the launcher, its own window and menus — \
                 already signed in."
            ),
            name,
            sola_kit::components::form::form_row(
                "Open several windows",
                toggler(draft.multi_window)
                    .on_toggle(Msg::WrapMultiWindow)
                    .style(sola_kit::components::form::toggle_style),
            ),
            row![
                kit_button::labeled("Wrap", kit_button::primary).on_press(Msg::WrapSubmit),
                kit_button::labeled("Cancel", kit_button::ghost).on_press(Msg::WrapCancel),
            ]
            .spacing(sola_kit::components::style::SPACE_SM)
            .align_y(Alignment::Center),
        ];
        sheet(body, Msg::WrapCancel)
    }

    fn view_remove_sheet(&self, app: &Application) -> Element<'_, Msg> {
        let body = column![
            text(format!("Remove {}?", app.label))
                .size(15)
                .font(sola_kit::fonts::ui_medium()),
            muted_line(
                "It leaves the launcher and its windows close. Its sign-in stays with the \
                 browser profile."
            ),
            row![
                kit_button::labeled("Remove", kit_button::primary).on_press(Msg::RemoveAppConfirm),
                kit_button::labeled("Cancel", kit_button::ghost).on_press(Msg::RemoveAppCancel),
            ]
            .spacing(sola_kit::components::style::SPACE_SM)
            .align_y(Alignment::Center),
        ];
        sheet(body, Msg::RemoveAppCancel)
    }
}

/// The Wrap sheet's name field.
fn wrap_name_id() -> iced::widget::Id {
    iced::widget::Id::new("web-app-wrap-name")
}

fn muted_line<'a>(s: &'a str) -> Element<'a, Msg> {
    text(s)
        .size(12)
        .style(|theme: &iced::Theme| {
            let t = theme.extended_palette().background.base.text;
            iced::widget::text::Style {
                color: Some(iced::Color { a: 0.72, ..t }),
            }
        })
        .into()
}

/// A small card centered over a dimmed page; a click outside cancels.
fn sheet<'a>(body: iced::widget::Column<'a, Msg>, cancel: Msg) -> Element<'a, Msg> {
    use sola_kit::components::style::{SPACE_MD, SPACE_SM};
    let panel = card::modal(
        container(body.spacing(SPACE_SM).width(Length::Fixed(300.0))).padding(SPACE_MD + SPACE_SM),
    )
    .width(Length::Fixed(340.0));
    let backdrop = mouse_area(
        container(Space::new().width(Length::Fill).height(Length::Fill)).style(|_t| {
            container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgba(
                    0.0, 0.0, 0.0, 0.22,
                ))),
                ..container::Style::default()
            }
        }),
    )
    .on_press(cancel);
    let centered = container(panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
    stack![backdrop, centered].into()
}

/// Only a web page wraps (not a blank or local one).
pub fn can_wrap(url: &str) -> bool {
    url != BLANK_URL && (url.starts_with("https://") || url.starts_with("http://"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdomains_are_the_same_site() {
        assert!(same_site("https://app.slack.com/client", "https://files.slack.com/x"));
        assert!(same_site("https://app.slack.com", "https://slack.com/"));
        assert!(!same_site("https://app.slack.com", "https://example.com"));
        assert!(!same_site("https://app.slack.com", "about:blank"));
        assert!(same_site("http://localhost:3000/a", "http://localhost:3000/b"));
    }

    #[test]
    fn a_name_is_the_titles_site_part() {
        assert_eq!(suggested_name("general - Acme - Slack", "https://app.slack.com"), "Slack");
        assert_eq!(suggested_name("Inbox (3) | Fastmail", "https://app.fastmail.com"), "Fastmail");
        assert_eq!(suggested_name("Linear", "https://linear.app"), "Linear");
        assert_eq!(suggested_name("", "https://app.slack.com/x"), "slack.com");
    }

    fn web_app(id: &str, label: &str, url: &str, multi_window: bool) -> Application {
        Application {
            app_id: id.into(),
            label: label.into(),
            url: Some(url.into()),
            kind: AppKind::Web,
            multi_window,
            ..Default::default()
        }
    }

    fn catalog() -> WebApps {
        let mut web = WebApps::default();
        for app in [
            web_app("shortcut", "Shortcut", "https://app.shortcut.com/acme", true),
            web_app("slack", "Slack", "https://acme.slack.com", false),
        ] {
            web.apps.insert(app.app_id.clone(), app);
        }
        web.tabs.insert(TabId(1), "shortcut".into());
        web.tabs.insert(TabId(2), "slack".into());
        web
    }

    fn home(web: &WebApps, opener: u64, url: &str) -> Option<String> {
        web.app_for_link(TabId(opener), url).map(|a| a.app_id.clone())
    }

    #[test]
    fn a_link_stays_in_its_app_on_its_apex_and_finds_a_group_elsewhere() {
        let web = catalog();
        // On the app's apex, any subdomain.
        assert_eq!(home(&web, 1, "https://api.shortcut.com/x").as_deref(), Some("shortcut"));
        // A single-window app keeps its own too.
        assert_eq!(home(&web, 2, "https://files.slack.com/x").as_deref(), Some("slack"));
        // Another app's group.
        assert_eq!(home(&web, 2, "https://app.shortcut.com/story/1").as_deref(), Some("shortcut"));
        // From a browser page too.
        assert_eq!(home(&web, 9, "https://app.shortcut.com/story/1").as_deref(), Some("shortcut"));
    }

    #[test]
    fn any_other_link_is_a_browser_page() {
        let web = catalog();
        assert_eq!(home(&web, 1, "https://github.com/sola"), None);
        // A single-window app has no group to join.
        assert_eq!(home(&web, 1, "https://acme.slack.com/x"), None);
        assert_eq!(home(&web, 9, "https://acme.slack.com/x"), None);
    }

    #[test]
    fn two_apps_on_a_site_settle_on_the_first_by_name() {
        let mut web = catalog();
        let other = web_app("board", "Board", "https://app.shortcut.com/other", true);
        web.apps.insert(other.app_id.clone(), other);
        assert_eq!(home(&web, 9, "https://app.shortcut.com/x").as_deref(), Some("board"));
    }

    #[test]
    fn a_popup_is_the_app_s_while_blank_or_on_its_site() {
        let start = "https://app.shortcut.com/acme";
        assert!(popup_is_the_apps(start, ""));
        assert!(popup_is_the_apps(start, "about:blank"));
        assert!(popup_is_the_apps(start, "https://huddle.shortcut.com/x"));
        assert!(!popup_is_the_apps(start, "https://accounts.google.com/signin"));
    }

    #[test]
    fn only_a_multi_window_app_s_menu_opens_another_window() {
        let ids = |app: &Application| -> Vec<String> {
            app_menu(app).menus[0]
                .items
                .iter()
                .filter_map(|i| match i {
                    MenuItem::Action { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect()
        };
        let mut app = Application {
            app_id: "web-1".into(),
            label: "Shortcut".into(),
            ..Default::default()
        };
        assert!(!ids(&app).iter().any(|i| i == ACTION_NEW_WINDOW));
        app.multi_window = true;
        assert_eq!(ids(&app)[0], ACTION_NEW_WINDOW);
    }

    #[test]
    fn the_menu_has_the_apps_name_and_no_find() {
        let app = Application {
            app_id: "web-1".into(),
            label: "Slack".into(),
            kind: AppKind::Web,
            ..Default::default()
        };
        let menu = app_menu(&app);
        assert_eq!(menu.app_id, "web-1");
        assert_eq!(menu.menus[0].label, "Slack");
        let ids: Vec<&str> = menu.menus[1]
            .items
            .iter()
            .filter_map(|i| match i {
                MenuItem::Action { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["undo", "redo", "cut", "copy", "paste", "select_all"]);
        let kept = integration::menu_keys(&sola_kit::keys::apply(&menu));
        assert_eq!(kept, integration::menu_keys(&menu));
    }
}
