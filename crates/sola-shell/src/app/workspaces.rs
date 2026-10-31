//! Desktop workspaces (Next slice 5): showing one, sending a window to
//! one, the workspace list's actions, each workspace's last focused
//! window, and what the shell tells apps (`Topic::Workspaces`,
//! `Topic::WindowWorkspace`). The model is [`crate::stacks`].

use std::collections::HashMap;

use sola_bus::topics::{Topic, WindowWorkspace, Workspaces};

use super::{Shell, with_bus};

/// What the shell remembers about workspaces beyond the model.
#[derive(Debug, Default)]
pub struct WorkspaceState {
    /// The window that had focus when each workspace was left (by id).
    focus: HashMap<String, u32>,
    /// Last published `Topic::Workspaces`.
    sent: Option<Workspaces>,
    /// Last published workspace of each window.
    sent_windows: HashMap<u32, String>,
    /// The workspace being renamed and the name typed so far.
    pub renaming: Option<(usize, String)>,
}

/// What the workspace menu can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceAction {
    Show(usize),
    New,
    Rename(usize),
    MoveLeft(usize),
    MoveRight(usize),
    Delete(usize),
}

impl Shell {
    /// Show workspace `index` (Super+1…9, the workspace menu): its last
    /// focused window takes focus.
    pub(crate) fn show_workspace(&mut self, index: usize) {
        self.remember_workspace_focus();
        if !self.tabs.show_workspace(index) {
            return;
        }
        tracing::info!(index, id = %self.tabs.showing_id(), "show workspace");
        self.focus_showing_workspace();
    }

    /// Super+Tab / Super+Shift+Tab.
    pub(crate) fn step_workspace(&mut self, back: bool) {
        let next = self.tabs.step_workspace(back);
        self.show_workspace(next);
    }

    /// Send the focused window to workspace `index` (Super+Ctrl+1…9). The
    /// showing workspace keeps focus on what is left.
    pub(crate) fn send_focused_to_workspace(&mut self, index: usize) {
        let Some(wid) = self.focused_window_id else {
            return;
        };
        let tab = self.tabs.parent_of(wid).unwrap_or(wid);
        if !self.tabs.send(tab, index) {
            return;
        }
        tracing::info!(window_id = tab, index, "send window to workspace");
        self.focus_showing_workspace();
        self.emit_all_frames();
    }

    /// The workspace menu.
    pub(crate) fn workspace_action(&mut self, action: WorkspaceAction) {
        match action {
            WorkspaceAction::Show(index) => {
                self.show_workspace(index);
                return;
            }
            WorkspaceAction::New => {
                self.remember_workspace_focus();
                let n = self.tabs.workspace_count() + 1;
                self.tabs.add_workspace(&format!("Workspace {n}"));
                self.focus_showing_workspace();
            }
            WorkspaceAction::Rename(index) => {
                let name = self
                    .tabs
                    .workspaces()
                    .get(index)
                    .map(|w| w.name.clone())
                    .unwrap_or_default();
                self.workspace.renaming = Some((index, name));
            }
            WorkspaceAction::MoveLeft(index) => {
                self.tabs.move_workspace(index, true);
            }
            WorkspaceAction::MoveRight(index) => {
                self.tabs.move_workspace(index, false);
            }
            WorkspaceAction::Delete(index) => {
                if self.tabs.delete_workspace(index) {
                    self.focus_showing_workspace();
                    self.emit_all_frames();
                }
            }
        }
        self.persist_tabs();
        self.sync_workspaces();
    }

    /// A workspace menu row. Rename swaps the menu for the rename
    /// card and gives it the keyboard; the rest close the menu.
    pub(crate) fn on_workspace_menu(&mut self, action: WorkspaceAction) -> iced::Task<super::Msg> {
        let rename = matches!(action, WorkspaceAction::Rename(_));
        let _ = self.dismiss_open_menu();
        self.workspace_action(action);
        if rename {
            self.menu_open = true;
            self.set_open_panel(Some(super::Panel::RenameWorkspace));
        }
        self.emit_overlay_frames();
        self.emit_composition();
        self.emit_registered_chords();
        if !rename {
            return iced::Task::none();
        }
        if let Some(wid) = self.lookup_window_id(Self::APP_ID, "menu") {
            with_bus(|bus| {
                let _ = bus.emit(Topic::Focus(sola_bus::topics::FocusTarget { window_id: wid }));
            });
        }
        iced::widget::operation::focus::<super::Msg>(iced::widget::Id::new(
            crate::menu::workspace::RENAME_INPUT_ID,
        ))
    }

    /// Finish a rename with the typed name (empty keeps the old one).
    pub(crate) fn commit_workspace_rename(&mut self) {
        if let Some((index, name)) = self.workspace.renaming.take() {
            self.tabs.rename_workspace(index, &name);
            self.persist_tabs();
            self.sync_workspaces();
        }
    }

    /// The focused window, while on the showing workspace, is the one to
    /// come back to.
    pub(crate) fn remember_workspace_focus(&mut self) {
        if let Some(wid) = self.focused_window_id.filter(|w| {
            self.tabs.desk_of(*w) == Some(self.tabs.showing())
        }) {
            let id = self.tabs.showing_id().to_string();
            self.workspace.focus.insert(id, wid);
        }
    }

    /// Focus the showing workspace's last focused window, else its most
    /// recently used one, else nothing; then compose it.
    fn focus_showing_workspace(&mut self) {
        let showing = self.tabs.showing();
        let remembered = self
            .workspace
            .focus
            .get(self.tabs.showing_id())
            .copied()
            .filter(|w| self.tabs.desk_of(*w) == Some(showing))
            .filter(|w| {
                self.app_id_for_window(*w)
                    .is_some_and(|app| !self.is_app_hidden(&app))
            });
        match remembered.or_else(|| self.mru_window_on_showing(None)) {
            Some(wid) => self.raise_window_from_click(wid),
            None => {
                self.focused_app_id = None;
                self.focused_window_id = None;
                self.emit_registered_chords();
                self.emit_composition();
                self.persist_tabs();
            }
        }
        self.sync_workspaces();
    }

    /// The most recently used window on the showing workspace whose app is
    /// not hidden (and is not `except`).
    pub(crate) fn mru_window_on_showing(&self, except: Option<&str>) -> Option<u32> {
        let showing = self.tabs.showing();
        let on_showing = |w: &u32| self.tabs.desk_of(*w) == Some(showing);
        self.mru_apps
            .iter()
            .filter(|app| app.as_str() != Self::APP_ID && Some(app.as_str()) != except)
            .filter(|app| !self.is_app_hidden(app))
            .find_map(|app| {
                self.mru_window_by_app
                    .get(app)
                    .copied()
                    .filter(on_showing)
                    .or_else(|| {
                        self.known_windows
                            .iter()
                            .filter(|w| &w.app_id == app)
                            .map(|w| w.window_id)
                            .find(on_showing)
                    })
            })
    }

    /// Publish the workspaces (when they or the showing one changed) and
    /// each window's workspace (when it changed; retracted for windows
    /// gone).
    pub(crate) fn sync_workspaces(&mut self) {
        // Until the saved layout is back, every window sits on the first
        // workspace for a moment; saying so would make apps act on it (the
        // browser reopens pages under another profile).
        if !self.tab_layout_loaded {
            return;
        }
        let list = Workspaces {
            workspaces: self.tabs.workspaces(),
            showing: self.tabs.showing_id().to_string(),
        };
        let list_changed = self.workspace.sent.as_ref() != Some(&list);
        let mut next: HashMap<u32, String> = HashMap::new();
        for w in &self.known_windows {
            if w.app_id == Self::APP_ID {
                continue;
            }
            if let Some(ws) = self.tabs.workspace_of(w.window_id) {
                next.insert(w.window_id, ws.to_string());
            }
        }
        let changed: Vec<WindowWorkspace> = next
            .iter()
            .filter(|(id, ws)| self.workspace.sent_windows.get(id) != Some(ws))
            .map(|(id, ws)| WindowWorkspace {
                window_id: *id,
                workspace: ws.clone(),
            })
            .collect();
        let gone: Vec<WindowWorkspace> = self
            .workspace
            .sent_windows
            .iter()
            .filter(|(id, _)| !next.contains_key(id))
            .map(|(id, ws)| WindowWorkspace {
                window_id: *id,
                workspace: ws.clone(),
            })
            .collect();
        if !list_changed && changed.is_empty() && gone.is_empty() {
            return;
        }
        self.workspace.sent_windows = next;
        if list_changed {
            self.workspace.sent = Some(list.clone());
        }
        with_bus(|bus| {
            if list_changed {
                let _ = bus.emit(Topic::Workspaces(list));
            }
            for w in changed {
                let _ = bus.emit(Topic::WindowWorkspace(w));
            }
            for w in gone {
                let _ = bus.retract(Topic::WindowWorkspace(w));
            }
        });
    }
}
