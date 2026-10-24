//! Pages and their profiles (Next slice 5). Each desktop workspace picks a
//! profile; every page window on it runs that profile, in that profile's
//! helper. A page opens under the showing workspace's profile (or its
//! opener's); a page whose window lands on a workspace with another
//! profile — sent there, or its workspace's profile changed — reopens at
//! the same URL under that profile, in the same window. Sessions are saved
//! per profile.

use std::collections::HashMap;
use std::time::Duration;

use iced::Task;
use sola_bus::Message;
use sola_bus::topics::Topic;

use super::{App, BLANK_URL, Msg};

/// How long the shell's workspace word must stand before pages reopen.
const REBIND_SETTLE: Duration = Duration::from_millis(600);
use crate::engine::{Cmd, Engine, TabId, TabInfo};
use crate::session;

impl<E: Engine> App<E> {
    /// Open page `id` under `profile`: its helper runs it.
    pub(super) fn open_in(&self, profile: &str, id: TabId, url: String, title: String) {
        let _ = self.cmd_tx.send(Cmd::InProfile {
            profile: profile.to_string(),
            cmd: Box::new(Cmd::OpenTab { id, url, title }),
        });
    }

    /// The profile a page runs under (its helper's, else the one chrome
    /// opened it in).
    pub(super) fn profile_of(&self, id: TabId) -> String {
        self.cached_tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.profile.clone())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(crate::profiles::showing_profile)
    }

    /// Fold `Workspaces` / `WindowWorkspace`. Pages whose workspace wants
    /// another profile reopen under it once the shell's word has settled
    /// ([`REBIND_SETTLE`]): a restarting shell moves windows for a moment
    /// while it pairs them, and a page must not reopen for that.
    pub(super) fn on_workspace_message(&mut self, msg: &Message) -> Task<Msg> {
        match Topic::parse(msg) {
            Some(Topic::Workspaces(w)) => {
                let ids: Vec<String> = w.workspaces.iter().map(|ws| ws.id.clone()).collect();
                crate::profiles::adopt_workspaces(&ids, &w.showing);
                // The menu checks the showing workspace's profile.
                crate::integration::republish_menus(self.app_id);
            }
            Some(Topic::WindowWorkspace(ww)) => {
                if msg.sticky {
                    self.window_ws.insert(ww.window_id, ww.workspace);
                } else {
                    self.window_ws.remove(&ww.window_id);
                }
            }
            // A window just paired with its sola-river id may already
            // have a workspace.
            Some(Topic::TabWindow(_)) => {}
            _ => return Task::none(),
        }
        self.rebind_gen = self.rebind_gen.wrapping_add(1);
        let generation = self.rebind_gen;
        Task::perform(tokio::time::sleep(REBIND_SETTLE), move |_| {
            Msg::RebindPages(generation)
        })
    }

    /// The settle timer fired: rebind unless the shell spoke again since.
    pub(super) fn on_rebind_timer(&mut self, generation: u64) {
        if generation == self.rebind_gen {
            self.rebind_pages();
        }
    }

    /// Every page whose window is on a workspace with another profile
    /// reopens under that workspace's profile.
    pub(super) fn rebind_pages(&mut self) {
        let moves: Vec<(TabId, String)> = self
            .cached_tabs
            .iter()
            .filter(|t| !t.profile.is_empty())
            // A web app runs on its own profile, whatever the workspace's.
            .filter(|t| !self.webapps.tabs.contains_key(&t.id))
            .filter_map(|t| {
                let window = self.windows.window_of(t.id)?;
                let river = self.windows.river_id(window)?;
                let ws = self.window_ws.get(&river)?;
                let want = crate::profiles::profile_for_workspace(ws);
                (want != t.profile).then_some((t.id, want))
            })
            .collect();
        for (id, profile) in moves {
            self.reopen_in(id, &profile);
        }
    }

    /// Reopen page `old` at its URL under `profile`, in the same window:
    /// its key (the shell's tab) and place stay; its back history and form
    /// state do not.
    pub(super) fn reopen_in(&mut self, old: TabId, profile: &str) {
        let Some(at) = self.cached_tabs.iter().position(|t| t.id == old) else {
            return;
        };
        let info = self.cached_tabs[at].clone();
        let url = if info.url.is_empty() {
            BLANK_URL.to_string()
        } else {
            info.url.clone()
        };
        let new = self.engine.alloc_tab_id();
        tracing::info!(
            old = old.0,
            new = new.0,
            from = %info.profile,
            to = %profile,
            %url,
            "page reopens under its workspace's profile"
        );
        if let Some(page_id) = self.page_ids.remove(&old) {
            self.page_ids.insert(new, page_id);
        }
        self.cached_tabs[at] = TabInfo {
            is_loading: url != BLANK_URL,
            profile: profile.to_string(),
            ..TabInfo::chrome(new, url.clone(), info.title.clone())
        };
        self.windows.retarget(old, new);
        // Gone for good, even if its helper lists it once more before it
        // closes (unlike `closed_tabs`, never pruned: ids are not reused).
        self.retired_tabs.insert(old);
        self.closed_tabs.insert(old);
        let _ = self.cmd_tx.send(Cmd::CloseTab(old));
        self.open_in(profile, new, url, info.title);
        if self.cached_active == old {
            self.cached_active = new;
            self.shown_active = new;
            self.active_handle
                .store(new.0, std::sync::atomic::Ordering::Relaxed);
            self.slot().present_tab(new);
            let _ = self.cmd_tx.send(Cmd::SetActiveTab(new));
        }
        self.persist_session();
    }

    /// Write each profile's session (its pages) when it changed. A profile
    /// whose last page closed this run is saved empty.
    pub fn persist_session(&mut self) {
        // Merge so a mid-navigation engine `about:blank` does not persist
        // over the URL we just committed.
        let live: Vec<TabInfo> = self
            .tabs_handle
            .lock()
            .unwrap()
            .iter()
            .filter(|t| !self.retired_tabs.contains(&t.id))
            .cloned()
            .collect();
        let tabs = if live.is_empty() {
            self.cached_tabs.clone()
        } else {
            super::merge_tab_snapshot(&self.cached_tabs, &live, &self.closed_tabs)
        };
        let mut by_profile: Vec<(String, Vec<TabInfo>)> = Vec::new();
        for t in &tabs {
            self.page_id(t.id);
            let profile = if t.profile.is_empty() {
                crate::profiles::showing_profile()
            } else {
                t.profile.clone()
            };
            match by_profile.iter_mut().find(|(p, _)| *p == profile) {
                Some((_, list)) => list.push(t.clone()),
                None => by_profile.push((profile, vec![t.clone()])),
            }
        }
        // Profiles saved before whose pages are all gone now.
        let emptied: Vec<String> = self
            .session_fp
            .keys()
            .filter(|p| !by_profile.iter().any(|(q, _)| q == *p))
            .cloned()
            .collect();
        for profile in emptied {
            by_profile.push((profile, Vec::new()));
        }
        for (profile, list) in by_profile {
            let session = if list.is_empty() {
                session::BrowserSession::default()
            } else {
                let active = if list.iter().any(|t| t.id == self.cached_active) {
                    self.cached_active
                } else {
                    list[0].id
                };
                session::session_from_tabs(
                    &list,
                    active,
                    &self.recently_closed,
                    &self.page_ids,
                    &self.webapps.tabs,
                )
            };
            let fp = session::fingerprint(&session);
            if self.session_fp.get(&profile) == Some(&fp) {
                continue;
            }
            session.save_to(&crate::profiles::session_path_for(&profile));
            self.session_fp.insert(profile, fp);
        }
    }
}

/// Which profiles' saved pages open at start: those a workspace uses, and
/// the registry's active one. Other profiles' pages stay saved until a
/// workspace picks them.
pub fn boot_profiles() -> Vec<String> {
    let mut out = vec![crate::profiles::registry_active_id()];
    for p in crate::profiles::workspace_profiles() {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out.retain(|p| !p.is_empty());
    out
}

/// The pages to open at start, each with its profile, and which is active
/// (the active profile's saved active page; a URL given on the command line
/// opens last and is active).
pub fn boot_pages(
    argv_url: Option<String>,
    default_url: &str,
) -> (Vec<(String, session::SessionTab)>, usize, Vec<session::ClosedTab>) {
    let profiles = boot_profiles();
    let active_profile = profiles.first().cloned().unwrap_or_default();
    let mut pages = Vec::new();
    let mut active = 0;
    let mut closed = Vec::new();
    for profile in &profiles {
        let saved = session::BrowserSession::load_from(&crate::profiles::session_path_for(profile));
        let is_active = *profile == active_profile;
        if is_active {
            closed = saved.closed.clone();
        }
        let argv = if is_active { argv_url.clone() } else { None };
        // Only the active profile gets a default page when it has none.
        if saved.tabs.is_empty() && argv.is_none() && !is_active {
            continue;
        }
        let start = pages.len();
        let (tabs, index) = saved.bootstrap(argv, default_url);
        if is_active {
            active = start + index;
        }
        pages.extend(tabs.into_iter().map(|t| (profile.clone(), t)));
    }
    (pages, active, closed)
}

/// Page profile tallies, for logs.
pub fn count_by_profile(pages: &[(String, session::SessionTab)]) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for (p, _) in pages {
        *out.entry(p.clone()).or_insert(0) += 1;
    }
    out
}
