//! A right-click in a page opens the desk menu: the shell draws it
//! (`docs/specs/2026-10-31-sola-vault-design.md`, P6). The browser sends
//! what was clicked and the page's own rows; the shell adds Edit and the
//! vault's rows, and sends back the pick for this process and token.
//!
//! Chrome's own fields (the location bar, the vault sheet) get theirs
//! from the kit's text input; this is the page's.

use iced::Task;
use sola_bus::topics::{DeskEdit, DeskMenuOpen, DeskMenuPick, DeskPick, Topic};

use super::{App, Msg, webapps};
use crate::engine::{Cmd, EditCmd, Engine, PageContext, TabId};
use crate::page_menu::{self, PageMenuKind};

/// The menu up now: its token, and the page and click it was for.
pub(crate) struct OpenMenu {
    token: u64,
    tab: TabId,
    ctx: PageContext,
}

/// Tokens start from the clock, so a restarted browser does not take a
/// pick meant for its last run.
pub(crate) fn first_token() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
}

impl<E: Engine> App<E> {
    /// CEF reported a right-click in a page: ask the shell for the menu.
    pub(crate) fn open_desk_menu(&mut self, ctx: PageContext) {
        let tab = if ctx.tab_id != 0 {
            TabId(ctx.tab_id)
        } else {
            self.cached_active
        };
        let url = self
            .cached_tabs
            .iter()
            .find(|t| t.id == tab)
            .map(|t| t.url.clone())
            .unwrap_or_default();
        let wrappable = !self.webapps.tabs.contains_key(&tab) && webapps::can_wrap(&url);
        self.desk_token = self.desk_token.wrapping_add(1);
        let token = self.desk_token;
        let at = super::last_cursor_point();
        let open = DeskMenuOpen {
            app_id: self
                .webapps
                .tabs
                .get(&tab)
                .cloned()
                .unwrap_or_else(|| self.app_id.to_string()),
            pid: std::process::id(),
            token,
            x: at.x.round() as i32,
            y: at.y.round() as i32,
            context: page_menu::desk_context(&ctx, ctx.secret, page_menu::menu_site(&url)),
            items: page_menu::desk_items(&ctx, wrappable),
            edit: page_menu::desk_edit(&ctx),
        };
        tracing::info!(
            tab = tab.0,
            token,
            editable = ctx.editable,
            secret = ctx.secret,
            host = %page_menu::menu_host(&url),
            "page right-click → desk menu"
        );
        let sent = sola_kit::app::try_bus()
            .and_then(|bus| bus.lock().ok())
            .is_some_and(|mut bus| bus.emit(Topic::DeskMenuOpen(open)).is_ok());
        if !sent {
            tracing::warn!("desk menu: the bus is not connected — no menu");
        }
        self.desk_menu = Some(OpenMenu { token, tab, ctx });
    }

    /// The shell's answer, if it is for our menu.
    pub(crate) fn on_desk_pick(&mut self, pick: DeskMenuPick) -> Task<Msg> {
        if pick.pid != std::process::id() {
            return Task::none();
        }
        let Some(menu) = self.desk_menu.take_if(|m| m.token == pick.token) else {
            tracing::debug!(token = pick.token, "desk menu: a pick for a menu no longer up");
            return Task::none();
        };
        match pick.pick {
            DeskPick::Dismissed => Task::none(),
            DeskPick::Action(id) => match PageMenuKind::from_id(&id) {
                Some(kind) => self.run_page_row(kind, menu),
                None => {
                    tracing::warn!(%id, "desk menu: a row the page did not send");
                    Task::none()
                }
            },
            DeskPick::Edit(edit) => {
                if menu.tab != self.cached_active {
                    self.switch_active_tab(menu.tab);
                }
                self.edit_page(match edit {
                    DeskEdit::Cut => EditCmd::Cut,
                    DeskEdit::Copy => EditCmd::Copy,
                    DeskEdit::Paste => EditCmd::Paste,
                    DeskEdit::SelectAll => EditCmd::SelectAll,
                })
            }
            DeskPick::VaultInsert { ticket } => self.desk_vault_insert(menu.tab, ticket),
            DeskPick::VaultFill { item } => self.desk_vault_fill(menu.tab, item),
        }
    }

    #[cfg(feature = "vault")]
    fn desk_vault_insert(&mut self, tab: TabId, ticket: String) -> Task<Msg> {
        tracing::info!(tab = tab.0, "desk menu: a vault value for the page");
        self.vault.ask(
            super::vault::Ask::Redeem { tab },
            sola_vault_proto::Request::Redeem { ticket },
        );
        Task::none()
    }

    #[cfg(not(feature = "vault"))]
    fn desk_vault_insert(&mut self, _tab: TabId, _ticket: String) -> Task<Msg> {
        Task::none()
    }

    #[cfg(feature = "vault")]
    fn desk_vault_fill(&mut self, tab: TabId, item: String) -> Task<Msg> {
        tracing::info!(tab = tab.0, "desk menu: fill the page from the vault");
        self.vault_fill_into(tab, item)
    }

    #[cfg(not(feature = "vault"))]
    fn desk_vault_fill(&mut self, _tab: TabId, _item: String) -> Task<Msg> {
        Task::none()
    }

    /// One of the page's rows, for the click it was opened on.
    fn run_page_row(&mut self, kind: PageMenuKind, menu: OpenMenu) -> Task<Msg> {
        let OpenMenu { tab, ctx, .. } = menu;
        if tab != self.cached_active
            && matches!(
                kind,
                PageMenuKind::Back
                    | PageMenuKind::Forward
                    | PageMenuKind::Reload
                    | PageMenuKind::InspectElement
                    | PageMenuKind::DevTools
                    | PageMenuKind::Wrap
            )
        {
            self.switch_active_tab(tab);
        }
        match kind {
            PageMenuKind::OpenLink => {
                if let Some(url) = ctx.link_url.filter(|u| !u.is_empty()) {
                    self.open_tab(url, true);
                }
                Task::none()
            }
            PageMenuKind::CopyLink => {
                match crate::util::usable_clipboard_text(ctx.link_url) {
                    Some(t) => iced::clipboard::write(t),
                    None => Task::none(),
                }
            }
            PageMenuKind::Back => self.update(Msg::NavBack),
            PageMenuKind::Forward => self.update(Msg::NavForward),
            PageMenuKind::Reload => self.update(Msg::NavReloadOrStop),
            PageMenuKind::DevTools => {
                let _ = self.cmd_tx.send(Cmd::ShowDevTools {
                    panel: "console".into(),
                    inspect_x: None,
                    inspect_y: None,
                });
                Task::none()
            }
            PageMenuKind::InspectElement => {
                let _ = self.cmd_tx.send(Cmd::ShowDevTools {
                    panel: "elements".into(),
                    inspect_x: Some(ctx.x),
                    inspect_y: Some(ctx.y),
                });
                Task::none()
            }
            PageMenuKind::UseAsBackground => {
                self.wallpaper_start(ctx.src_url.unwrap_or_default(), ctx.link_url)
            }
            PageMenuKind::Wrap => self.update(Msg::WrapOpen),
            PageMenuKind::Separator => Task::none(),
        }
    }

    /// An Edit action into the page's focused field (the shell's menu, or
    /// the Edit menu while the page has the keys).
    pub(crate) fn edit_page(&mut self, cmd: EditCmd) -> Task<Msg> {
        tracing::debug!(?cmd, "edit → engine (web content)");
        // Paste: a data-control read (image or text). Iced's text receive
        // can drop the offer; CEF's `paste()` is empty off screen and can
        // *set* that empty selection as the new source.
        if cmd == EditCmd::Paste {
            return crate::page_paste::read_task().map(Msg::PageOffer);
        }
        if cmd == EditCmd::Copy || cmd == EditCmd::Cut {
            // frame.copy() only fills Chromium's clipboard: extract the
            // selection with a script and write it to Wayland ourselves.
            let _ = self
                .cmd_tx
                .send(Cmd::EvaluateJs(crate::paste_js::copy_selection_script()));
            if cmd == EditCmd::Cut {
                let _ = self.cmd_tx.send(Cmd::Edit(EditCmd::Cut));
            }
            return Task::none();
        }
        let _ = self.cmd_tx.send(Cmd::Edit(cmd));
        Task::none()
    }
}
