//! The navigator in the app: opening one, its keys and rows, and handing
//! its place to the session it opened.

use std::path::PathBuf;

use iced::keyboard::{self, key::Named};
use iced::{Task, window};

use super::{Busy, Folders, Navigator, Pick, hosts, view};
use crate::{APP_ID, App, Msg, NavMsg};

impl App {
    /// Open a navigator window in front (`naming`: at New project).
    pub(crate) fn open_navigator(&mut self, naming: bool) -> Task<Msg> {
        let (window, opened) = self.windows.open_navigator(APP_ID);
        self.navigators.insert(window, Navigator::new(Folders::local(), naming));
        opened.then(|_| focus_field())
    }

    /// The navigator keys go to, if the focused window is one.
    pub(crate) fn focused_navigator(&self) -> Option<&Navigator> {
        self.windows.focused.and_then(|w| self.navigators.get(&w))
    }

    /// What is open here, for the navigator's marks.
    pub(crate) fn open_here(&self) -> view::Open {
        let mut open = view::Open::default();
        for tab in self.tabs.iter().filter(|t| self.windows.has(&t.id)) {
            open.sessions.insert(tab.id.clone());
            if let Some(p) = self.projects.iter().find(|p| p.id == tab.project_id) {
                open.roots.insert((p.host.clone(), p.root.clone()));
            }
        }
        open
    }

    /// Esc steps back (closing the window at the top); the arrows move
    /// down the rows. Every other key is the field's.
    pub(crate) fn nav_key(&mut self, window: window::Id, event: &iced::Event) -> Task<Msg> {
        let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) = event else {
            return Task::none();
        };
        let Some(nav) = self.navigators.get_mut(&window) else {
            return Task::none();
        };
        match key {
            keyboard::Key::Named(Named::Escape) => {
                if nav.back() {
                    focus_field()
                } else {
                    self.close_window(window)
                }
            }
            keyboard::Key::Named(Named::ArrowUp) => {
                nav.step(-1);
                Task::none()
            }
            keyboard::Key::Named(Named::ArrowDown) => {
                nav.step(1);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    pub(crate) fn on_nav(&mut self, window: window::Id, m: NavMsg) -> Task<Msg> {
        let Some(nav) = self.navigators.get_mut(&window) else {
            return Task::none();
        };
        match m {
            NavMsg::Query(q) => {
                nav.set_query(q);
                Task::none()
            }
            NavMsg::Pick(i) => {
                let pick = nav.pick(i);
                self.nav_pick(window, pick)
            }
            NavMsg::Listed(host, listing) => {
                nav.listed(&host, listing);
                focus_field()
            }
            NavMsg::Made(Ok((path, host))) => self.nav_open(window, path, host, None),
            NavMsg::Made(Err(e)) => {
                nav.busy = None;
                nav.error = Some(e);
                Task::none()
            }
        }
    }

    fn nav_pick(&mut self, window: window::Id, pick: Pick) -> Task<Msg> {
        match pick {
            // The step changed: the field is the next step's.
            Pick::Nothing => focus_field(),
            Pick::Open { path, host, session } => self.nav_open(window, path, host, session),
            Pick::List(host) => {
                if let Some(nav) = self.navigators.get_mut(&window) {
                    nav.busy = Some(Busy::Listing(host.clone()));
                }
                let h = host.clone();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || hosts::list(&h))
                            .await
                            .unwrap_or_else(|e| Err(e.to_string()))
                    },
                    move |listing| Msg::Nav(window, NavMsg::Listed(host.clone(), listing)),
                )
            }
            Pick::Create { name, root, host } => {
                if let Some(nav) = self.navigators.get_mut(&window) {
                    nav.busy = Some(Busy::Creating(name.clone()));
                }
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || match &host {
                            Some(h) => hosts::create(h, &root, &name).map(|p| (p, host.clone())),
                            None => super::create_local(&root, &name).map(|p| (p, None)),
                        })
                        .await
                        .unwrap_or_else(|e| Err(e.to_string()))
                    },
                    move |made| Msg::Nav(window, NavMsg::Made(made)),
                )
            }
        }
    }

    /// Open a project (or one session of it) as `project.open` does; the
    /// navigator goes once a window of it is up ([`Self::nav_handed_off`]).
    fn nav_open(
        &mut self,
        window: window::Id,
        path: PathBuf,
        host: Option<String>,
        session: Option<String>,
    ) -> Task<Msg> {
        self.nav_handoff = Some(window);
        if let (Some(nav), Some(name)) = (self.navigators.get_mut(&window), path.file_name()) {
            nav.busy = Some(Busy::Opening(name.to_string_lossy().into_owned()));
        }
        // Here its window is up when this returns; on a host, once its
        // sessions there are listed. Either way through `on_host_sessions`,
        // which hands off.
        match self.cli_open_project(&path.to_string_lossy(), host.as_deref(), session) {
            Ok((_, task)) => task,
            Err(e) => {
                self.nav_handoff = None;
                if let Some(nav) = self.navigators.get_mut(&window) {
                    nav.busy = None;
                    nav.error = Some(e);
                }
                Task::none()
            }
        }
    }

    /// After a navigator's pick opened or showed its session's window:
    /// close the navigator, so that window takes its place.
    pub(crate) fn nav_handed_off(&mut self) -> Task<Msg> {
        match self.nav_handoff.take() {
            Some(window) => self.close_window(window),
            None => Task::none(),
        }
    }
}

fn focus_field() -> Task<Msg> {
    iced::widget::operation::focus::<Msg>(iced::widget::Id::new(view::FIELD_ID))
}
