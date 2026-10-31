//! A page's WebAuthn `get()` / `create()`: the sheet opens on it, the
//! daemon signs or saves, and the answer goes back to the frame that
//! asked — with the frame's origin as CEF reported it, never the page's.

use iced::Task;
use sola_vault_proto::{PasskeyCandidate, Request};

use super::{Ask, PendingPasskey, Screen};
use crate::app::{App, Msg};
use crate::engine::{Cmd, Engine, FrameScope, TabId};
use crate::vault::{PasskeyPageRequest, origin_of};

impl<E: Engine> App<E> {
    /// What pages sent through their helpers since the last wake.
    pub(crate) fn drain_vault_page_events(&mut self) {
        use crate::cef::ipc::VaultPageEvent;
        let events: Vec<_> = self
            .engine
            .passkeys_handle()
            .lock()
            .unwrap()
            .drain(..)
            .collect();
        for ev in events {
            match ev {
                VaultPageEvent::WebAuthn(ev) if ev.action == "cancel" => {
                    self.page_dropped_passkey(TabId(ev.tab), &ev.nonce);
                }
                VaultPageEvent::WebAuthn(ev) => {
                    let id = self.vault.next_passkey_id();
                    self.passkey_requested(PasskeyPageRequest::new(id, ev));
                }
                VaultPageEvent::Filled { found, .. } => self.vault_fill_reported(found),
            }
        }
        self.end_passkey_if_page_left();
    }

    /// A page asked: open the sheet on it (unlock first if need be).
    fn passkey_requested(&mut self, req: PasskeyPageRequest) {
        // The same site asking again while the sheet is up: answer both
        // with the one pick, so the page does not fail mid-pick.
        if let Some(cur) = self.vault.passkey.as_mut() {
            if cur.req.origin == req.origin
                && cur.req.action == req.action
                && cur.req.rp_id == req.rp_id
            {
                tracing::info!(keep = cur.req.id, extra = req.id, "vault: the same passkey request again");
                cur.extra.push(req);
                return;
            }
        }
        if let Some(old) = self.vault.passkey.take() {
            self.settle_passkey(&old, false, "Superseded by another passkey request.");
        }
        tracing::info!(
            req = req.id,
            tab = req.tab.0,
            host = %crate::vault::log_host(&req.origin),
            action = %req.action,
            "vault: a page asked for a passkey"
        );
        let page_origin = self
            .cached_tabs
            .iter()
            .find(|t| t.id == req.tab)
            .and_then(|t| origin_of(&t.url));
        let create = req.is_create();
        let v = &mut self.vault;
        v.passkey = Some(PendingPasskey {
            req,
            extra: Vec::new(),
            page_origin,
            candidates: Vec::new(),
            loading: true,
            error: None,
        });
        v.error = None;
        v.reprompt = None;
        v.screen = if create {
            Screen::PasskeyCreate
        } else {
            Screen::PasskeyPick
        };
        v.list.chosen = 0;
        self.set_vault_open(true);
        if self.vault.unlocked() {
            let _ = self.passkey_continue();
        }
    }

    /// The vault is open (now): ask for what the request needs.
    pub(super) fn passkey_continue(&mut self) -> Task<Msg> {
        let v = &mut self.vault;
        let Some(p) = v.passkey.as_mut() else {
            return Task::none();
        };
        p.loading = true;
        p.error = None;
        let req = p.req.id;
        if p.is_create() {
            v.screen = Screen::PasskeyCreate;
            v.site_logins.clear();
            v.site_logins_loading = true;
            let url = p.req.origin.clone();
            v.ask(Ask::PasskeyLogins { req }, Request::Matches { url });
        } else {
            v.screen = Screen::PasskeyPick;
            let rp_id = p.req.rp_id.clone();
            let public_key_json = p.req.public_key_json.clone();
            v.ask(
                Ask::PasskeyList { req },
                Request::PasskeyCandidates {
                    rp_id,
                    public_key_json,
                },
            );
        }
        Task::none()
    }

    pub(super) fn passkey_candidates(&mut self, req: u64, candidates: Vec<PasskeyCandidate>) {
        let Some(p) = self.vault.passkey.as_mut().filter(|p| p.req.id == req) else {
            return;
        };
        tracing::info!(req, n = candidates.len(), "vault: passkey candidates");
        p.loading = false;
        p.candidates = candidates;
        self.vault.list.chosen = 0;
    }

    pub(super) fn passkey_candidates_failed(&mut self, req: u64, message: String) {
        if let Some(p) = self.vault.passkey.as_mut().filter(|p| p.req.id == req) {
            p.loading = false;
            p.error = Some(message);
        }
    }

    /// Sign in with the passkey on `item`.
    pub(super) fn passkey_sign(&mut self, item: String) {
        let v = &mut self.vault;
        if v.busy {
            return;
        }
        let Some(p) = v.passkey.as_mut() else {
            return;
        };
        p.error = None;
        let req = p.req.id;
        let origin = p.req.origin.clone();
        let public_key_json = p.req.public_key_json.clone();
        v.busy = true;
        v.ask(
            Ask::PasskeySign { req },
            Request::PasskeyAssert {
                origin,
                public_key_json,
                cipher_id: item,
            },
        );
    }

    /// Save the site's new passkey onto `onto`, or as a new login.
    pub(super) fn passkey_create(&mut self, onto: Option<String>) {
        let v = &mut self.vault;
        if v.busy {
            return;
        }
        let Some(p) = v.passkey.as_mut().filter(|p| p.is_create()) else {
            return;
        };
        p.error = None;
        let req = p.req.id;
        let origin = p.req.origin.clone();
        let public_key_json = p.req.public_key_json.clone();
        v.busy = true;
        v.ask(
            Ask::PasskeySign { req },
            Request::PasskeyCreate {
                origin,
                public_key_json,
                cipher_id: onto,
            },
        );
    }

    /// The daemon signed (or saved) — or could not.
    pub(super) fn passkey_answered(&mut self, req: u64, answer: Result<String, String>) {
        self.vault.busy = false;
        let Some(p) = self.vault.passkey.as_mut().filter(|p| p.req.id == req) else {
            tracing::info!(req, "vault: passkey answer for a request that ended — dropped");
            return;
        };
        match answer {
            Ok(json) => {
                let p = self.vault.passkey.take().expect("checked above");
                self.settle_passkey(&p, true, &json);
                tracing::info!(req, "vault: passkey answer sent to the page");
                self.end_passkey_screen();
            }
            // A save that failed stays up, to try again or cancel.
            Err(message) if p.is_create() => {
                tracing::warn!(req, "vault: passkey save failed");
                p.error = Some(message);
            }
            Err(message) => {
                let p = self.vault.passkey.take().expect("checked above");
                tracing::warn!(req, "vault: passkey sign-in failed");
                self.settle_passkey(&p, false, &message);
                self.end_passkey_screen();
            }
        }
    }

    /// Answer every page request of `p`, each in its own frame.
    fn settle_passkey(&self, p: &PendingPasskey, ok: bool, payload: &str) {
        for req in p.requests() {
            let _ = self.cmd_tx.send(Cmd::Target {
                tab: req.tab,
                cmd: Box::new(Cmd::EvaluateJsIn {
                    scope: FrameScope::Frame(req.frame.clone()),
                    script: req.resolve_script(ok, payload),
                }),
            });
        }
    }

    /// Cancel: the page gets its error, so it can offer another way in.
    pub(crate) fn cancel_passkey(&mut self, reason: &str) {
        if let Some(p) = self.vault.passkey.take() {
            self.settle_passkey(&p, false, reason);
        }
        self.end_passkey_screen();
    }

    fn end_passkey_screen(&mut self) {
        let v = &mut self.vault;
        v.busy = false;
        v.site_logins.clear();
        if matches!(v.screen, Screen::PasskeyPick | Screen::PasskeyCreate) {
            v.screen = Screen::Browse;
            self.set_vault_open(false);
        }
    }

    /// The page stopped waiting for its request `nonce` (its timeout).
    fn page_dropped_passkey(&mut self, tab: TabId, nonce: &str) {
        let Some(p) = self.vault.passkey.as_mut() else {
            return;
        };
        p.extra.retain(|r| r.tab != tab || r.nonce != nonce);
        if p.req.tab == tab && p.req.nonce == nonce {
            tracing::info!(req = p.req.id, "vault: the page stopped waiting for its passkey");
            self.cancel_passkey("The site stopped waiting.");
        }
    }

    /// The asking tab closed or left the site: nothing to answer any more.
    fn end_passkey_if_page_left(&mut self) {
        let Some(p) = self.vault.passkey.as_ref() else {
            return;
        };
        if !self.engine_tabs_seen {
            return;
        }
        let Some(asked_on) = p.page_origin.as_deref() else {
            return;
        };
        let left = match self.cached_tabs.iter().find(|t| t.id == p.req.tab) {
            None => true,
            Some(t) => origin_of(&t.url).as_deref() != Some(asked_on),
        };
        if left {
            tracing::info!(req = p.req.id, "vault: the passkey page went away");
            self.vault.passkey = None;
            self.end_passkey_screen();
        }
    }
}
