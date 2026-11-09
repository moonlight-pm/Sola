//! Filling a page, copying a value, and opening Vault.
//!
//! The daemon gives fill material only for the page's own URL; the
//! browser runs the fill script in the frames that may have it
//! ([`FrameScope`]) and drops what it held. A login with a code also puts
//! the code on the clipboard (V6), taken off again after the vault's
//! clipboard time if it is still there.

use std::time::Duration;

use iced::Task;
use sola_vault_proto::{FillMaterial, ItemKind, ItemSummary, Request};
use zeroize::Zeroize;

use super::update::VaultMsg;
use super::{AfterFill, Aim, Ask, Clip, RepromptFor, Screen};
use crate::app::{App, BLANK_URL, Msg};
use crate::engine::{Cmd, Engine, FrameScope, TabId};
use crate::vault::{
    fill_card_script, fill_credentials_script_ex, fill_identity_script, fill_totp_script, origin_of,
    site_uri,
};

impl<E: Engine> App<E> {
    /// Where a fill for `tab` goes, while it stays on the origin it has now.
    pub(super) fn vault_aim(&self, tab: TabId, then: AfterFill) -> Option<Aim> {
        let t = self.cached_tabs.iter().find(|t| t.id == tab)?;
        if t.url.is_empty() || t.url == BLANK_URL {
            return None;
        }
        let origin = origin_of(&t.url)?;
        Some(Aim {
            tab,
            origin,
            url: t.url.clone(),
            then,
        })
    }

    /// Fill the active page from the sheet (a row, Enter, the record's Fill).
    pub(super) fn vault_fill_from_panel(&mut self, item: String) -> Task<Msg> {
        let v = &self.vault;
        if v.busy || !v.unlocked() {
            return Task::none();
        }
        if self.vault_needs_reprompt(&item) {
            return self.vault_ask_reprompt(RepromptFor::Fill(item));
        }
        let kind = v
            .record
            .item
            .as_ref()
            .filter(|r| r.id == item)
            .map(|r| r.kind)
            .or_else(|| v.list.items.iter().find(|i| i.id == item).map(|i| i.kind));
        if !kind.is_some_and(ItemKind::can_fill) {
            return self.vault_open_item(item);
        }
        let Some(aim) = self.vault_aim(self.cached_active, AfterFill::Close) else {
            self.vault.error = Some("This page can't be filled.".into());
            return Task::none();
        };
        let v = &mut self.vault;
        v.busy = true;
        v.error = None;
        let url = aim.url.clone();
        v.ask(Ask::Fill(aim), Request::FillMaterial { id: item, url });
        Task::none()
    }

    /// The desk menu's Fill on a page: the item into `tab`, the sheet
    /// untouched (a re-prompt, or a refusal, opens it). An item the
    /// sheet's list does not have is looked up first: a re-prompt item is
    /// never filled on the list's word alone.
    pub(crate) fn vault_fill_into(&mut self, tab: TabId, item: String) -> Task<Msg> {
        let v = &self.vault;
        let known = v.reprompt_ok.contains(&item) || v.list.items.iter().any(|i| i.id == item);
        if !known {
            self.vault
                .ask(Ask::FillCheck { tab, item }, Request::List { url: None });
            return Task::none();
        }
        self.vault_fill_known(tab, item)
    }

    /// The service's list for a [`Ask::FillCheck`]: fill, ask for the
    /// master password, or say the item is gone.
    pub(super) fn vault_fill_checked(&mut self, tab: TabId, item: String, items: &[ItemSummary]) -> Task<Msg> {
        match asks_master_password(items, &item) {
            None => self.vault_fill_refused(tab, "That item isn't in the vault.".into()),
            Some(true) if !self.vault.reprompt_ok.contains(&item) => {
                self.switch_active_tab(tab);
                // The sheet's list, for the fill after the re-prompt.
                self.vault_list();
                self.vault_ask_reprompt(RepromptFor::Fill(item))
            }
            Some(_) => self.vault_fill_send(tab, item),
        }
    }

    fn vault_fill_known(&mut self, tab: TabId, item: String) -> Task<Msg> {
        if self.vault_needs_reprompt(&item) {
            self.switch_active_tab(tab);
            return self.vault_ask_reprompt(RepromptFor::Fill(item));
        }
        self.vault_fill_send(tab, item)
    }

    fn vault_fill_send(&mut self, tab: TabId, item: String) -> Task<Msg> {
        let Some(aim) = self.vault_aim(tab, AfterFill::Quiet) else {
            return self.vault_fill_refused(tab, "This page can't be filled.".into());
        };
        let url = aim.url.clone();
        self.vault
            .ask(Ask::Fill(aim), Request::FillMaterial { id: item, url });
        Task::none()
    }

    /// A desk-menu fill that did not happen: the sheet opens over `tab`
    /// and says why (the browser has no other place to say it).
    pub(super) fn vault_fill_refused(&mut self, tab: TabId, message: String) -> Task<Msg> {
        tracing::info!(tab = tab.0, "vault: the desk menu's fill was refused");
        if tab != self.cached_active {
            self.switch_active_tab(tab);
        }
        let opened = if self.vault.is_open() {
            Task::none()
        } else {
            self.set_vault_open(true);
            self.vault_opened()
        };
        self.vault.error = Some(message);
        opened
    }

    /// Fill material is here: run its script at the aim, then let go of it.
    pub(super) fn vault_filled(&mut self, aim: Aim, mut material: FillMaterial) -> Task<Msg> {
        self.vault.busy = false;
        let mut copy = Task::none();
        let (script, scope, code) = match &mut material {
            FillMaterial::Login {
                username,
                password,
                code,
                uris,
            } => {
                let script = fill_credentials_script_ex(
                    username.as_deref(),
                    password.as_deref(),
                    aim.then == AfterFill::Created,
                );
                let uris = uris
                    .iter()
                    .map(|u| site_uri(&u.uri, u.match_type))
                    .collect();
                let scope = FrameScope::Site {
                    origin: aim.origin.clone(),
                    uris,
                };
                (script, scope, code.take())
            }
            FillMaterial::Card {
                cardholder_name,
                number,
                exp_month,
                exp_year,
                code,
                brand,
            } => {
                let script = fill_card_script(
                    cardholder_name.as_deref(),
                    number.as_deref(),
                    exp_month.as_deref(),
                    exp_year.as_deref(),
                    code.as_deref(),
                    brand.as_deref(),
                );
                // Payment fields live in the processor's frames.
                let scope = FrameScope::Page {
                    origin: aim.origin.clone(),
                };
                (script, scope, None)
            }
            FillMaterial::Identity(i) => {
                let script = fill_identity_script(
                    i.title.as_deref(),
                    i.first_name.as_deref(),
                    i.middle_name.as_deref(),
                    i.last_name.as_deref(),
                    i.address1.as_deref(),
                    i.address2.as_deref(),
                    i.address3.as_deref(),
                    i.city.as_deref(),
                    i.state.as_deref(),
                    i.postal_code.as_deref(),
                    i.country.as_deref(),
                    i.company.as_deref(),
                    i.email.as_deref(),
                    i.phone.as_deref(),
                    i.ssn.as_deref(),
                    i.username.as_deref(),
                    i.passport_number.as_deref(),
                    i.license_number.as_deref(),
                );
                let scope = FrameScope::Site {
                    origin: aim.origin.clone(),
                    uris: Vec::new(),
                };
                (script, scope, None)
            }
        };
        zeroize_material(&mut material);
        self.send_fill(aim.tab, scope.clone(), script);
        if let Some(mut code) = code {
            // The code field, if this page has one too: in the frames the
            // password may go to (a sign-in frame from the login's site).
            self.send_fill(aim.tab, scope, fill_totp_script(&code));
            if self.vault.settings.copy_code_on_fill {
                copy = self.vault_copy_secret(std::mem::take(&mut code));
            }
            code.zeroize();
        }
        tracing::info!(host = %crate::vault::log_host(&aim.url), "vault: fill sent to the page");
        match aim.then {
            AfterFill::Close => {
                self.vault.screen = Screen::Browse;
                self.vault.close_record();
                self.set_vault_open(false);
            }
            AfterFill::Created => self.vault_await_fill(),
            AfterFill::Quiet => {}
        }
        copy
    }

    fn send_fill(&self, tab: TabId, scope: FrameScope, script: String) {
        let _ = self.cmd_tx.send(Cmd::Target {
            tab,
            cmd: Box::new(Cmd::EvaluateJsIn { scope, script }),
        });
    }

    /// A field's copy button: on the clipboard, the row says so.
    pub(super) fn vault_copy(&mut self, key: String, value: String) -> Task<Msg> {
        self.vault.record.copied = Some((key, std::time::Instant::now()));
        self.vault_copy_secret(value)
    }

    /// Put `value` on the clipboard and take it off after the vault's
    /// clipboard time, if it is still there then.
    pub(super) fn vault_copy_secret(&mut self, value: String) -> Task<Msg> {
        let secs = self.vault.settings.clipboard_clear_secs;
        let write = iced::clipboard::write(value.clone());
        if secs == 0 {
            let mut value = value;
            value.zeroize();
            return write;
        }
        let generation = self.vault.next_clip_generation();
        self.vault.clip = Some(Clip { generation, value });
        let expire = Task::perform(
            tokio::time::sleep(Duration::from_secs(u64::from(secs))),
            move |_| Msg::Vault(VaultMsg::ClipExpired(generation)),
        );
        Task::batch([write, expire])
    }

    pub(super) fn vault_clip_expired(&mut self, generation: u64) -> Task<Msg> {
        if self.vault.clip.as_ref().is_none_or(|c| c.generation != generation) {
            return Task::none();
        }
        iced::clipboard::read().map(move |now| Msg::Vault(VaultMsg::ClipNow(generation, now)))
    }

    /// The clipboard now: clear it only if it still holds our value.
    pub(super) fn vault_clip_now(&mut self, generation: u64, now: Option<String>) -> Task<Msg> {
        let mut now = now.unwrap_or_default();
        let ours = self
            .vault
            .clip
            .as_ref()
            .is_some_and(|c| c.generation == generation && c.value == now);
        now.zeroize();
        if self.vault.clip.as_ref().is_some_and(|c| c.generation == generation) {
            self.vault.clip = None;
        }
        if ours {
            tracing::debug!("vault: copied value taken off the clipboard");
            return iced::clipboard::write(String::new());
        }
        Task::none()
    }

    /// The page reported whether a new login's fill found fields.
    pub(super) fn vault_fill_reported(&mut self, found: bool) {
        if self.vault.awaiting_fill.is_some() {
            self.vault_created_filled(found);
        }
    }
}

fn zeroize_material(m: &mut FillMaterial) {
    let wipe = |s: &mut Option<String>| {
        if let Some(s) = s.as_mut() {
            s.zeroize();
        }
    };
    match m {
        FillMaterial::Login {
            username,
            password,
            code,
            ..
        } => {
            wipe(username);
            wipe(password);
            wipe(code);
        }
        FillMaterial::Card {
            cardholder_name,
            number,
            code,
            ..
        } => {
            wipe(cardholder_name);
            wipe(number);
            wipe(code);
        }
        FillMaterial::Identity(i) => {
            wipe(&mut i.ssn);
            wipe(&mut i.passport_number);
            wipe(&mut i.license_number);
        }
    }
}

/// How long Vault has to answer `vault.open` before the browser starts it.
const OPEN_CALL_WAIT: Duration = Duration::from_secs(3);

/// Show `item` (or the vault) in Vault, the app: the daemon's call plane
/// `vault.open` brings Vault up on it; with no daemon to ask, the session
/// starts `sola-vault --item <id>`.
pub(crate) fn open_in_vault(item: Option<String>) {
    let spawned = std::thread::Builder::new()
        .name("sola-vault-open".into())
        .spawn(move || {
            let params = match &item {
                Some(id) => serde_json::json!({ "id": id }),
                None => serde_json::json!({}),
            };
            match sola_call::invoke("vault", "open", params, OPEN_CALL_WAIT) {
                Ok(_) => tracing::info!("vault: opened in Vault"),
                Err(e) => {
                    tracing::info!(error = %e, "vault: vault.open not answered — launching Vault");
                    launch_vault(item.as_deref());
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "vault: could not start the open thread");
    }
}

fn launch_vault(item: Option<&str>) {
    use sola_bus::topics::{LaunchAppPayload, Topic};
    let bin = sola_core::paths::bin_path("sola-vault").display().to_string();
    let command = match item {
        Some(id) => format!("{bin} --item {id}"),
        None => bin,
    };
    let Some(bus) = sola_kit::app::try_bus() else {
        return;
    };
    if let Ok(mut bus) = bus.lock() {
        let _ = bus.emit(Topic::LaunchApp(LaunchAppPayload {
            app_id: "sola-vault".into(),
            command,
        }));
    }
}

/// Whether item `id` asks for the master password before it fills;
/// `None` when the list does not have it.
fn asks_master_password(items: &[ItemSummary], id: &str) -> Option<bool> {
    items.iter().find(|i| i.id == id && !i.deleted).map(|i| i.reprompt)
}

#[cfg(test)]
mod tests {
    use super::asks_master_password;
    use sola_vault_proto::{ItemKind, ItemSummary};

    fn item(id: &str, reprompt: bool, deleted: bool) -> ItemSummary {
        ItemSummary {
            id: id.into(),
            kind: ItemKind::Login,
            name: id.into(),
            subtitle: String::new(),
            last_used: 0,
            revised: 0,
            has_totp: false,
            has_passkey: false,
            uri_match: false,
            reprompt,
            favorite: false,
            folder_id: None,
            organization_id: None,
            collection_ids: Vec::new(),
            deleted,
            search: String::new(),
        }
    }

    #[test]
    fn the_services_list_decides_the_re_prompt() {
        let items = [item("a", false, false), item("b", true, false), item("c", false, true)];
        assert_eq!(asks_master_password(&items, "a"), Some(false));
        assert_eq!(asks_master_password(&items, "b"), Some(true));
        assert_eq!(asks_master_password(&items, "c"), None, "in the trash");
        assert_eq!(asks_master_password(&items, "d"), None);
    }
}
