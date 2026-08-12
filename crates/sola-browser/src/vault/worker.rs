//! Background vault worker — owns `VaultService` on a dedicated tokio thread.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use zeroize::Zeroize;

use super::client::{
    LoginOutcome, MatchSummary, TwoFactorKind, VaultError, VaultService, VaultStatus,
};
use super::passkey::PasskeyCandidate;

/// Commands from chrome → vault worker.
pub enum VaultCmd {
    Login { email: String, password: String },
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
    Matches { url: String },
    Fill { id: String },
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
    Status,
    Quit,
}

/// Events from vault worker → chrome.
#[derive(Debug, Clone)]
pub enum VaultEvent {
    Status(VaultStatus),
    LoginOk { email: String },
    /// Email OTP (new device) or authenticator TOTP required.
    LoginNeedsTwoFactor {
        email: String,
        kinds: Vec<TwoFactorKind>,
        preferred: TwoFactorKind,
        email_hint: Option<String>,
        /// True if we successfully asked the server to email a code.
        email_sent: bool,
    },
    LoginFailed { message: String },
    EmailCodeSent,
    EmailCodeFailed { message: String },
    SyncOk { full: bool },
    SyncFailed { message: String },
    Matches(Vec<MatchSummary>),
    FillReady {
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
    Error { message: String },
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

async fn finish_authenticated(
    svc: &mut VaultService,
    event_tx: &Sender<VaultEvent>,
    email: String,
) {
    let _ = event_tx.send(VaultEvent::LoginOk {
        email: email.clone(),
    });
    // Status before sync so chrome knows unlocked when SyncOk arrives
    // (match picker requests on SyncOk).
    let _ = event_tx.send(VaultEvent::Status(svc.status()));
    match svc.sync().await {
        Ok(full) => {
            let _ = event_tx.send(VaultEvent::SyncOk { full });
        }
        Err(e) => {
            let _ = event_tx.send(VaultEvent::SyncFailed {
                message: e.to_string(),
            });
        }
    }
    let _ = event_tx.send(VaultEvent::Status(svc.status()));
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
            let _ = event_tx.send(VaultEvent::LoginNeedsTwoFactor {
                email,
                kinds,
                preferred,
                email_hint,
                email_sent,
            });
            let _ = event_tx.send(VaultEvent::Status(svc.status()));
        }
    }
}

async fn worker_loop(cmd_rx: Receiver<VaultCmd>, event_tx: Sender<VaultEvent>) {
    let mut svc = VaultService::new();
    let _ = event_tx.send(VaultEvent::Status(svc.status()));

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
                let _ = event_tx.send(VaultEvent::Status(svc.status()));
            }
            VaultCmd::Login { email, mut password } => {
                match svc.login(email.clone(), password.clone()).await {
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
                        let _ = event_tx.send(VaultEvent::LoginFailed { message });
                        let _ = event_tx.send(VaultEvent::Status(svc.status()));
                    }
                }
            }
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
                        let _ = event_tx.send(VaultEvent::LoginFailed {
                            message: e.to_string(),
                        });
                        let _ = event_tx.send(VaultEvent::Status(svc.status()));
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
                    let _ = event_tx.send(VaultEvent::EmailCodeSent);
                }
                Err(e) => {
                    password.zeroize();
                    tracing::warn!(?kind, error = %e, "vault: OTP resend failed");
                    let _ = event_tx.send(VaultEvent::EmailCodeFailed {
                        message: e.to_string(),
                    });
                }
            },
            VaultCmd::Sync => match svc.sync().await {
                Ok(full) => {
                    let _ = event_tx.send(VaultEvent::SyncOk { full });
                    let _ = event_tx.send(VaultEvent::Status(svc.status()));
                }
                Err(e) => {
                    let _ = event_tx.send(VaultEvent::SyncFailed {
                        message: e.to_string(),
                    });
                }
            },
            VaultCmd::Matches { url } => match svc.matches_for_url(&url).await {
                Ok(m) => {
                    let _ = event_tx.send(VaultEvent::Matches(m));
                }
                Err(e) => {
                    let _ = event_tx.send(VaultEvent::Error {
                        message: e.to_string(),
                    });
                }
            },
            VaultCmd::Fill { id } => match svc.fill_fields(&id).await {
                Ok(mut material) => {
                    let username = material.username.take();
                    let password = material.password.take();
                    let _ = event_tx.send(VaultEvent::FillReady {
                        username,
                        password,
                    });
                }
                Err(e) => {
                    let _ = event_tx.send(VaultEvent::Error {
                        message: e.to_string(),
                    });
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
                        let _ = event_tx.send(VaultEvent::PasskeyCandidates {
                            req_id,
                            candidates,
                        });
                    }
                    Err(e) => {
                        tracing::warn!(req_id, error = %e, "vault: passkey list failed");
                        let _ = event_tx.send(VaultEvent::PasskeyReady {
                            req_id,
                            ok: false,
                            payload: e.to_string(),
                        });
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
                        let payload = serde_json::to_string(&assertion).unwrap_or_else(|e| {
                            format!(r#"{{"error":"{e}"}}"#)
                        });
                        tracing::info!(req_id, %origin, %cipher_id, "vault: passkey assertion ok");
                        let _ = event_tx.send(VaultEvent::PasskeyReady {
                            req_id,
                            ok: true,
                            payload,
                        });
                    }
                    Err(e) => {
                        tracing::warn!(req_id, error = %e, "vault: passkey assertion failed");
                        let _ = event_tx.send(VaultEvent::PasskeyReady {
                            req_id,
                            ok: false,
                            payload: e.to_string(),
                        });
                    }
                }
            }
        }
    }
}
