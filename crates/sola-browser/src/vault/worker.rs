//! Background vault worker — owns `VaultService` on a dedicated tokio thread.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use zeroize::Zeroize;

use super::client::{
    CardSummary, LoginOutcome, MatchSummary, TotpSummary, TwoFactorKind, VaultError, VaultService,
    VaultStatus,
};
use super::item::{IdentityFillMaterial, ItemRecord, ItemSummary};
use super::passkey::PasskeyCandidate;

/// Commands from chrome → vault worker.
pub enum VaultCmd {
    Login {
        email: String,
        password: String,
    },
    /// Complete 2FA / new-device verification.
    LoginTwoFactor {
        email: String,
        password: String,
        token: String,
        kind: TwoFactorKind,
        remember: bool,
    },
    /// Re-send new-device / email 2FA code.
    ResendEmailCode {
        email: String,
        password: String,
        kind: TwoFactorKind,
    },
    Sync,
    Matches {
        url: String,
    },
    Fill {
        id: String,
    },
    /// List logins that have a TOTP secret (page URL ranks matches first).
    ListTotp {
        url: String,
    },
    /// Decrypt + generate the current authenticator code.
    FillTotp {
        id: String,
    },
    /// List every card cipher (no URI filter).
    ListCards,
    /// Decrypt a card for page fill.
    FillCard {
        id: String,
    },
    /// All vault items for the unified panel (page URL ranks URI matches).
    ListItems {
        url: String,
    },
    /// Full decrypted record for the item view.
    GetItem {
        id: String,
    },
    /// Decrypt an identity for page fill.
    FillIdentity {
        id: String,
    },
    /// Persist a new login then return fill material.
    CreateLogin {
        name: String,
        username: String,
        password: String,
        uri: String,
    },
    /// List passkeys for a WebAuthn get() RP so chrome can show a picker.
    PasskeyList {
        req_id: u64,
        rp_id: String,
    },
    /// WebAuthn get() — sign with a vault passkey the user selected.
    PasskeyAssert {
        /// Page request id (for JS resolve).
        req_id: u64,
        origin: String,
        /// Serialized `publicKey` options (challenge etc. base64url).
        public_key_json: String,
        /// Cipher id chosen in the passkey picker (required).
        cipher_id: String,
    },
    /// WebAuthn create() — register a vault passkey.
    PasskeyRegister {
        req_id: u64,
        origin: String,
        public_key_json: String,
        /// Existing login to attach to. `None` creates a new personal login.
        cipher_id: Option<String>,
    },
    Status,
    Quit,
}

/// Events from vault worker → chrome.
#[derive(Debug, Clone)]
pub enum VaultEvent {
    Status(VaultStatus),
    LoginOk {
        email: String,
    },
    /// Email OTP (new device) or authenticator TOTP required.
    LoginNeedsTwoFactor {
        email: String,
        kinds: Vec<TwoFactorKind>,
        preferred: TwoFactorKind,
        email_hint: Option<String>,
        /// True if we successfully asked the server to email a code.
        email_sent: bool,
    },
    LoginFailed {
        message: String,
    },
    EmailCodeSent,
    EmailCodeFailed {
        message: String,
    },
    SyncOk {
        full: bool,
    },
    SyncFailed {
        message: String,
    },
    Matches(Vec<MatchSummary>),
    Cards(Vec<CardSummary>),
    Totp(Vec<TotpSummary>),
    Items(Vec<ItemSummary>),
    ItemReady(ItemRecord),
    IdentityFillReady(IdentityFillMaterial),
    TotpFillReady {
        code: String,
    },
    FillReady {
        username: Option<String>,
        password: Option<String>,
    },
    CardFillReady {
        cardholder_name: Option<String>,
        number: Option<String>,
        exp_month: Option<String>,
        exp_year: Option<String>,
        code: Option<String>,
        brand: Option<String>,
    },
    /// New login is on the server — fill the page (same payload as FillReady).
    Created {
        id: Option<String>,
        username: Option<String>,
        password: Option<String>,
    },
    /// Passkeys available for a pending WebAuthn get().
    PasskeyCandidates {
        req_id: u64,
        candidates: Vec<PasskeyCandidate>,
    },
    /// Passkey assertion for the page polyfill (`req_id` matches intercept).
    PasskeyReady {
        req_id: u64,
        ok: bool,
        /// On ok: assertion JSON string; on err: error message.
        payload: String,
    },
    Error {
        message: String,
    },
}

/// Handle held by `App` for the vault worker.
pub struct VaultHandle {
    cmd_tx: Sender<VaultCmd>,
    event_rx: Arc<Mutex<Receiver<VaultEvent>>>,
}

impl VaultHandle {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<VaultCmd>();
        let (event_tx, event_rx) = mpsc::channel::<VaultEvent>();

        thread::Builder::new()
            .name("sola-vault".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("vault tokio runtime");
                rt.block_on(worker_loop(cmd_rx, event_tx));
            })
            .expect("spawn sola-vault thread");

        Self {
            cmd_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
        }
    }

    pub fn send(&self, cmd: VaultCmd) {
        if let Err(e) = self.cmd_tx.send(cmd) {
            tracing::error!(error = %e, "vault: cmd channel closed");
        }
    }

    pub fn try_recv(&self) -> Option<VaultEvent> {
        match self.event_rx.lock().ok()?.try_recv() {
            Ok(ev) => Some(ev),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                tracing::error!("vault: event channel disconnected");
                None
            }
        }
    }
}

impl Drop for VaultHandle {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(VaultCmd::Quit);
    }
}

fn emit(event_tx: &Sender<VaultEvent>, ev: VaultEvent) {
    let _ = event_tx.send(ev);
    crate::chrome_wake::wake();
}

async fn finish_authenticated(
    svc: &mut VaultService,
    event_tx: &Sender<VaultEvent>,
    email: String,
) {
    emit(
        &event_tx,
        VaultEvent::LoginOk {
            email: email.clone(),
        },
    );
    // Status before sync so chrome knows unlocked when SyncOk arrives
    // (match picker requests on SyncOk).
    emit(&event_tx, VaultEvent::Status(svc.status()));
    match svc.sync().await {
        Ok(full) => {
            emit(&event_tx, VaultEvent::SyncOk { full });
        }
        Err(e) => {
            emit(
                &event_tx,
                VaultEvent::SyncFailed {
                    message: e.to_string(),
                },
            );
        }
    }
    emit(&event_tx, VaultEvent::Status(svc.status()));
}

async fn handle_login_outcome(
    svc: &mut VaultService,
    event_tx: &Sender<VaultEvent>,
    email: String,
    outcome: LoginOutcome,
) {
    match outcome {
        LoginOutcome::Authenticated { .. } => {
            finish_authenticated(svc, event_tx, email).await;
        }
        LoginOutcome::NeedsTwoFactor {
            kinds,
            preferred,
            email_hint,
            email_sent,
        } => {
            tracing::info!(
                ?preferred,
                email_sent,
                n_kinds = kinds.len(),
                "vault: login needs second factor"
            );
            emit(
                &event_tx,
                VaultEvent::LoginNeedsTwoFactor {
                    email,
                    kinds,
                    preferred,
                    email_hint,
                    email_sent,
                },
            );
            emit(&event_tx, VaultEvent::Status(svc.status()));
        }
    }
}

async fn worker_loop(cmd_rx: Receiver<VaultCmd>, event_tx: Sender<VaultEvent>) {
    let mut svc = VaultService::new();
    emit(&event_tx, VaultEvent::Status(svc.status()));

    loop {
        let cmd = match cmd_rx.try_recv() {
            Ok(c) => c,
            Err(TryRecvError::Empty) => {
                tokio::time::sleep(Duration::from_millis(16)).await;
                continue;
            }
            Err(TryRecvError::Disconnected) => break,
        };

        match cmd {
            VaultCmd::Quit => break,
            VaultCmd::Status => {
                emit(&event_tx, VaultEvent::Status(svc.status()));
            }
            VaultCmd::Login {
                email,
                mut password,
            } => match svc.login(email.clone(), password.clone()).await {
                Ok(outcome) => {
                    password.zeroize();
                    handle_login_outcome(&mut svc, &event_tx, email, outcome).await;
                }
                Err(e) => {
                    password.zeroize();
                    let message = match e {
                        VaultError::LoginFailed => "Login failed.".into(),
                        other => other.to_string(),
                    };
                    tracing::warn!(%message, "vault: login failed");
                    emit(&event_tx, VaultEvent::LoginFailed { message });
                    emit(&event_tx, VaultEvent::Status(svc.status()));
                }
            },
            VaultCmd::LoginTwoFactor {
                email,
                mut password,
                token,
                kind,
                remember,
            } => {
                match svc
                    .login_with_two_factor(email.clone(), password.clone(), token, kind, remember)
                    .await
                {
                    Ok(outcome) => {
                        password.zeroize();
                        handle_login_outcome(&mut svc, &event_tx, email, outcome).await;
                    }
                    Err(e) => {
                        password.zeroize();
                        tracing::warn!(error = %e, "vault: 2FA login failed");
                        emit(
                            &event_tx,
                            VaultEvent::LoginFailed {
                                message: e.to_string(),
                            },
                        );
                        emit(&event_tx, VaultEvent::Status(svc.status()));
                    }
                }
            }
            VaultCmd::ResendEmailCode {
                email,
                mut password,
                kind,
            } => match svc.resend_otp(email, password.clone(), kind).await {
                Ok(()) => {
                    password.zeroize();
                    tracing::info!(?kind, "vault: OTP resend ok");
                    emit(&event_tx, VaultEvent::EmailCodeSent);
                }
                Err(e) => {
                    password.zeroize();
                    tracing::warn!(?kind, error = %e, "vault: OTP resend failed");
                    emit(
                        &event_tx,
                        VaultEvent::EmailCodeFailed {
                            message: e.to_string(),
                        },
                    );
                }
            },
            VaultCmd::Sync => match svc.sync().await {
                Ok(full) => {
                    emit(&event_tx, VaultEvent::SyncOk { full });
                    emit(&event_tx, VaultEvent::Status(svc.status()));
                }
                Err(e) => {
                    emit(
                        &event_tx,
                        VaultEvent::SyncFailed {
                            message: e.to_string(),
                        },
                    );
                }
            },
            VaultCmd::Matches { url } => match svc.matches_for_url(&url).await {
                Ok(m) => {
                    emit(&event_tx, VaultEvent::Matches(m));
                }
                Err(e) => {
                    emit(
                        &event_tx,
                        VaultEvent::Error {
                            message: e.to_string(),
                        },
                    );
                }
            },
            VaultCmd::CreateLogin {
                name,
                username,
                mut password,
                uri,
            } => {
                let user = if username.trim().is_empty() {
                    None
                } else {
                    Some(username)
                };
                let pass = if password.trim().is_empty() {
                    None
                } else {
                    Some(password.clone())
                };
                match svc
                    .create_login(name, user, pass, Some(uri).filter(|s| !s.trim().is_empty()))
                    .await
                {
                    Ok((id, mut material)) => {
                        if let Some(ref id) = id {
                            crate::vault::VaultPrefs::touch_cipher(id);
                        }
                        if let Some(ref u) = material.username {
                            crate::vault::VaultPrefs::save_last_username(u);
                        }
                        let username = material.username.take();
                        let password = material.password.take();
                        tracing::info!(id = id.as_deref(), "vault: created login");
                        emit(
                            &event_tx,
                            VaultEvent::Created {
                                id,
                                username,
                                password,
                            },
                        );
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "vault: create login failed");
                        emit(
                            &event_tx,
                            VaultEvent::Error {
                                message: e.to_string(),
                            },
                        );
                    }
                }
                password.zeroize();
            }
            VaultCmd::ListTotp { url } => match svc.list_totp(&url).await {
                Ok(m) => {
                    emit(&event_tx, VaultEvent::Totp(m));
                }
                Err(e) => {
                    emit(
                        &event_tx,
                        VaultEvent::Error {
                            message: e.to_string(),
                        },
                    );
                }
            },
            VaultCmd::FillTotp { id } => match svc.fill_totp(&id).await {
                Ok(code) => {
                    crate::vault::VaultPrefs::touch_cipher(&id);
                    emit(&event_tx, VaultEvent::TotpFillReady { code });
                }
                Err(e) => {
                    emit(
                        &event_tx,
                        VaultEvent::Error {
                            message: e.to_string(),
                        },
                    );
                }
            },
            VaultCmd::ListCards => match svc.list_cards().await {
                Ok(m) => {
                    emit(&event_tx, VaultEvent::Cards(m));
                }
                Err(e) => {
                    emit(
                        &event_tx,
                        VaultEvent::Error {
                            message: e.to_string(),
                        },
                    );
                }
            },
            VaultCmd::FillCard { id } => match svc.fill_card(&id).await {
                Ok(mut material) => {
                    crate::vault::VaultPrefs::touch_cipher(&id);
                    let cardholder_name = material.cardholder_name.take();
                    let number = material.number.take();
                    let exp_month = material.exp_month.take();
                    let exp_year = material.exp_year.take();
                    let code = material.code.take();
                    let brand = material.brand.take();
                    emit(
                        &event_tx,
                        VaultEvent::CardFillReady {
                            cardholder_name,
                            number,
                            exp_month,
                            exp_year,
                            code,
                            brand,
                        },
                    );
                }
                Err(e) => {
                    emit(
                        &event_tx,
                        VaultEvent::Error {
                            message: e.to_string(),
                        },
                    );
                }
            },
            VaultCmd::ListItems { url } => match svc.list_items(&url).await {
                Ok(m) => emit(&event_tx, VaultEvent::Items(m)),
                Err(e) => emit(
                    &event_tx,
                    VaultEvent::Error {
                        message: e.to_string(),
                    },
                ),
            },
            VaultCmd::GetItem { id } => match svc.get_item(&id).await {
                Ok(item) => emit(&event_tx, VaultEvent::ItemReady(item)),
                Err(e) => emit(
                    &event_tx,
                    VaultEvent::Error {
                        message: e.to_string(),
                    },
                ),
            },
            VaultCmd::FillIdentity { id } => match svc.fill_identity(&id).await {
                Ok(material) => {
                    crate::vault::VaultPrefs::touch_cipher(&id);
                    emit(&event_tx, VaultEvent::IdentityFillReady(material));
                }
                Err(e) => emit(
                    &event_tx,
                    VaultEvent::Error {
                        message: e.to_string(),
                    },
                ),
            },
            VaultCmd::Fill { id } => match svc.fill_fields(&id).await {
                Ok(mut material) => {
                    crate::vault::VaultPrefs::touch_cipher(&id);
                    if let Some(ref u) = material.username {
                        crate::vault::VaultPrefs::save_last_username(u);
                    }
                    let username = material.username.take();
                    let password = material.password.take();
                    emit(&event_tx, VaultEvent::FillReady { username, password });
                }
                Err(e) => {
                    emit(
                        &event_tx,
                        VaultEvent::Error {
                            message: e.to_string(),
                        },
                    );
                }
            },
            VaultCmd::PasskeyList { req_id, rp_id } => {
                match super::passkey::list_candidates(&svc, &rp_id).await {
                    Ok(candidates) => {
                        tracing::info!(
                            req_id,
                            %rp_id,
                            n = candidates.len(),
                            "vault: passkey candidates"
                        );
                        emit(
                            &event_tx,
                            VaultEvent::PasskeyCandidates { req_id, candidates },
                        );
                    }
                    Err(e) => {
                        tracing::warn!(req_id, error = %e, "vault: passkey list failed");
                        emit(
                            &event_tx,
                            VaultEvent::PasskeyReady {
                                req_id,
                                ok: false,
                                payload: e.to_string(),
                            },
                        );
                    }
                }
            }
            VaultCmd::PasskeyAssert {
                req_id,
                origin,
                public_key_json,
                cipher_id,
            } => {
                match super::passkey::authenticate(
                    &svc,
                    &origin,
                    &public_key_json,
                    Some(cipher_id.clone()),
                )
                .await
                {
                    Ok(assertion) => {
                        crate::vault::VaultPrefs::touch_cipher(&cipher_id);
                        let payload = serde_json::to_string(&assertion)
                            .unwrap_or_else(|e| format!(r#"{{"error":"{e}"}}"#));
                        tracing::info!(req_id, %origin, %cipher_id, "vault: passkey assertion ok");
                        emit(
                            &event_tx,
                            VaultEvent::PasskeyReady {
                                req_id,
                                ok: true,
                                payload,
                            },
                        );
                    }
                    Err(e) => {
                        tracing::warn!(req_id, error = %e, "vault: passkey assertion failed");
                        emit(
                            &event_tx,
                            VaultEvent::PasskeyReady {
                                req_id,
                                ok: false,
                                payload: e.to_string(),
                            },
                        );
                    }
                }
            }
            VaultCmd::PasskeyRegister {
                req_id,
                origin,
                public_key_json,
                cipher_id,
            } => {
                match super::passkey::register(&svc, &origin, &public_key_json, cipher_id.clone())
                    .await
                {
                    Ok((attestation, ctx)) => match svc.persist_encryption_context(ctx).await {
                        Ok(id) => {
                            if let Some(ref id) = id {
                                crate::vault::VaultPrefs::touch_cipher(id);
                            }
                            let payload = serde_json::to_string(&attestation)
                                .unwrap_or_else(|e| format!(r#"{{"error":"{e}"}}"#));
                            tracing::info!(
                                req_id,
                                %origin,
                                cipher_id = id.as_deref().unwrap_or("-"),
                                attached = cipher_id.is_some(),
                                "vault: passkey register ok"
                            );
                            emit(
                                &event_tx,
                                VaultEvent::PasskeyReady {
                                    req_id,
                                    ok: true,
                                    payload,
                                },
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                req_id,
                                error = %e,
                                "vault: passkey register persist failed"
                            );
                            emit(
                                &event_tx,
                                VaultEvent::PasskeyReady {
                                    req_id,
                                    ok: false,
                                    payload: format!(
                                        "Could not save the passkey to Bitwarden: {e}"
                                    ),
                                },
                            );
                        }
                    },
                    Err(e) => {
                        tracing::warn!(req_id, error = %e, "vault: passkey register failed");
                        emit(
                            &event_tx,
                            VaultEvent::PasskeyReady {
                                req_id,
                                ok: false,
                                payload: e.to_string(),
                            },
                        );
                    }
                }
            }
        }
    }
}
