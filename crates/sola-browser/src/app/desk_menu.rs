//! A right-click in a page opens the desk menu: the shell draws it
//! (`docs/specs/2026-10-31-sola-vault-design.md`, P6) and the kit's
//! [`sola_kit::desk_menu`] carries it. The browser sends what was clicked
//! and the page's own rows; the shell adds Edit and the vault's rows, and
//! sends back the pick for this process and token.
//!
//! Chrome's own fields (the location bar, the vault sheet) get theirs
//! from the kit's text input; this is the page's.

use iced::Task;
use sola_kit::desk_menu::{self, DeskEdit, DeskMenuPick, DeskPick, Secret, VaultError};

use super::{App, Msg, webapps};
use crate::engine::{Cmd, EditCmd, Engine, PageContext, TabId};
use crate::page_menu::{self, MenuWindow, PageMenuKind};

/// The menu up now: its token, and the page and click it was for.
pub(crate) struct OpenMenu {
    token: u64,
    tab: TabId,
    ctx: PageContext,
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
        // A web app's window draws no developer tools and is wrapped
        // already.
        let window = if self.webapps.tabs.contains_key(&tab) {
            MenuWindow::WebApp
        } else {
            MenuWindow::Browser {
                wrappable: webapps::can_wrap(&url),
            }
        };
        let token = desk_menu::next_token();
        // Where the kit's frame saw the right press; CEF reports the menu
        // after a round trip, so fall back to the last pointer position.
        let at = desk_menu::pointer().unwrap_or_else(super::last_cursor_point);
        tracing::info!(
            tab = tab.0,
            token,
            editable = ctx.editable,
            host = %page_menu::menu_host(&url),
            "page right-click → desk menu"
        );
        desk_menu::open(
            token,
            at.x,
            at.y,
            page_menu::desk_context(&ctx, page_menu::menu_site(&url)),
            page_menu::desk_items(&ctx, window),
            page_menu::desk_edit(&ctx),
        );
        self.desk_menu = Some(OpenMenu { token, tab, ctx });
    }

    /// The shell's answer (for this process), if it is for the page's
    /// menu — a kit field's picks are the kit's.
    pub(crate) fn on_desk_pick(&mut self, pick: DeskMenuPick) -> Task<Msg> {
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
            DeskPick::Edit(edit) => self.desk_edit(edit, menu),
            DeskPick::VaultInsert { ticket } => self.desk_vault_insert(menu.tab, ticket),
            DeskPick::VaultFill { item } => self.desk_vault_fill(menu.tab, item),
        }
    }

    /// An Edit row into the page it was opened on. Copy and Cut take the
    /// selection CEF reported with the click, onto the clipboard from the
    /// compositor's side (the pick can land before the window has the
    /// keyboard back).
    fn desk_edit(&mut self, edit: DeskEdit, menu: OpenMenu) -> Task<Msg> {
        if menu.tab != self.cached_active {
            self.switch_active_tab(menu.tab);
        }
        let selection = menu.ctx.selection.filter(|s| !s.is_empty());
        match (edit, selection) {
            (DeskEdit::Copy, Some(text)) => desk_menu::copy(text),
            (DeskEdit::Cut, Some(text)) => {
                let _ = self.cmd_tx.send(Cmd::Target {
                    tab: menu.tab,
                    cmd: Box::new(Cmd::Edit(EditCmd::Cut)),
                });
                desk_menu::copy(text)
            }
            (DeskEdit::Copy, None) => self.edit_page(EditCmd::Copy),
            (DeskEdit::Cut, None) => self.edit_page(EditCmd::Cut),
            (DeskEdit::Paste, _) => self.edit_page(EditCmd::Paste),
            (DeskEdit::SelectAll, _) => self.edit_page(EditCmd::SelectAll),
        }
    }

    /// A vault value for the page's field: redeemed on the vault's socket
    /// (only this process can), then inserted at the caret.
    fn desk_vault_insert(&mut self, tab: TabId, ticket: String) -> Task<Msg> {
        tracing::info!(tab = tab.0, "desk menu: a vault value for the page");
        desk_menu::redeem_task(ticket).map(move |r| Msg::DeskRedeemed(tab, r))
    }

    /// The redeemed value goes in at the page's caret — never through the
    /// clipboard; the [`Secret`] is cleared when it drops here.
    pub(crate) fn desk_redeemed(&mut self, tab: TabId, value: Result<Secret, VaultError>) {
        match value {
            Ok(secret) => {
                let _ = self.cmd_tx.send(Cmd::Target {
                    tab,
                    cmd: Box::new(Cmd::PasteText(secret.as_str().to_string())),
                });
            }
            Err(e) => tracing::warn!(code = ?e.code, "desk menu: the vault value could not be redeemed"),
        }
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
                // Behind, as the page menu always opened it: the page
                // you clicked on stays in front.
                if let Some(url) = ctx.link_url.filter(|u| !u.is_empty()) {
                    let opener = self.cached_active;
                    let profile = self.profile_of(opener);
                    self.open_link(opener, url, false, &profile);
                }
                Task::none()
            }
            PageMenuKind::CopyLink => {
                match crate::util::usable_clipboard_text(ctx.link_url) {
                    // The pick can land before the window has the keyboard back.
                    Some(t) => desk_menu::copy(t),
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
