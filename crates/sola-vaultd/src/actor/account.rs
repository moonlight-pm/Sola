//! The account: sign in (with its second step), unlock, lock, log out,
//! the master-password check.

use std::time::Instant;

use sola_vault_proto::{ErrorCode, LoginOutcome, Reply, Request, Server, TwoFactorKind};
use tracing::info;
use zeroize::Zeroizing;

use super::{Account, Pending, Vault};
use crate::peer::Peer;
use crate::vault::errors::{Result, err, signed_out};
use crate::vault::{Login, VaultService};

impl Vault {
    pub(super) async fn account_request(&mut self, req: Request, peer: &Peer) -> Result<Reply> {
        match req {
            Request::Login {
                email,
                password,
                server,
            } => {
                let password = Zeroizing::new(password);
                self.login(email, password, server).await
            }
            Request::TwoFactor { kind, code } => {
                let code = Zeroizing::new(code);
                self.two_factor(kind, &code).await
            }
            Request::SendEmailCode { kind } => {
                let p = self
                    .pending
                    .as_ref()
                    .ok_or_else(|| err(ErrorCode::Invalid, "No login is waiting for a code."))?;
                let device = self.prefs.device_id(&self.dir);
                VaultService::send_code(&p.server, &device, &p.email, &p.password, kind).await?;
                info!(?kind, "vault: login code sent");
                Ok(Reply::Done)
            }
            Request::Unlock { password } => {
                let password = Zeroizing::new(password);
                self.unlock(&password).await
            }
            Request::Lock => {
                self.lock("asked");
                Ok(Reply::Done)
            }
            Request::Logout => {
                self.sign_out();
                info!("vault: logged out");
                Ok(Reply::Done)
            }
            Request::VerifyMasterPassword { password } => {
                let password = Zeroizing::new(password);
                let session = match &self.account {
                    Account::Locked(s) => s,
                    Account::Unlocked(v) => v.session(),
                    Account::SignedOut => return Err(signed_out()),
                };
                Ok(Reply::Checked(session.check_password(&password)?))
            }
            req => self.vault_request(req, peer).await,
        }
    }

    async fn login(&mut self, email: String, password: Zeroizing<String>, server: Server) -> Result<Reply> {
        if !matches!(self.account, Account::SignedOut) {
            return Err(err(ErrorCode::Invalid, "An account is signed in. Log out first."));
        }
        let email = email.trim().to_string();
        if email.is_empty() || password.is_empty() {
            return Err(err(ErrorCode::Invalid, "Enter the email and the master password."));
        }
        check_server(&server)?;
        self.pending = None;
        let device = self.prefs.device_id(&self.dir);
        let login = VaultService::login(&self.dir, &server, &device, &email, &password, None).await?;
        self.after_login(login, email, password, server).await
    }

    async fn two_factor(&mut self, kind: TwoFactorKind, code: &str) -> Result<Reply> {
        let Some(p) = self.pending.take() else {
            return Err(err(ErrorCode::Invalid, "No login is waiting for a code."));
        };
        let device = self.prefs.device_id(&self.dir);
        let code = code.trim();
        let result =
            VaultService::login(&self.dir, &p.server, &device, &p.email, &p.password, Some((kind, code))).await;
        match result {
            Ok(login) => self.after_login(login, p.email.clone(), p.password.clone(), p.server.clone()).await,
            Err(e) => {
                // A wrong code: the login still waits.
                self.pending = Some(p);
                Err(e)
            }
        }
    }

    async fn after_login(
        &mut self,
        login: Login,
        email: String,
        password: Zeroizing<String>,
        server: Server,
    ) -> Result<Reply> {
        match login {
            Login::Unlocked(svc) => {
                info!(server = %server.label(), "vault: signed in");
                self.prefs.remember_email(&self.dir, &email);
                self.pending = None;
                self.account = Account::Unlocked(svc);
                self.next_screen_poll = Instant::now();
                if let Err(e) = self.sync_now().await {
                    tracing::warn!(error = %e, "vault: first sync failed");
                }
                Ok(Reply::Login(LoginOutcome::Unlocked))
            }
            Login::NeedsTwoFactor {
                kinds,
                preferred,
                email_hint,
                email_sent,
                unsupported,
            } => {
                info!(?preferred, email_sent, "vault: login needs its second step");
                self.pending = Some(Pending {
                    email,
                    password,
                    server,
                    since: Instant::now(),
                });
                Ok(Reply::Login(LoginOutcome::NeedsTwoFactor {
                    kinds,
                    preferred,
                    email_hint,
                    email_sent,
                    unsupported,
                }))
            }
        }
    }

    async fn unlock(&mut self, password: &str) -> Result<Reply> {
        let session = match &self.account {
            Account::Unlocked(_) => return Ok(Reply::Done),
            Account::SignedOut => return Err(signed_out()),
            Account::Locked(s) => s.clone(),
        };
        let device = self.prefs.device_id(&self.dir);
        let svc = VaultService::unlock(&self.dir, session, &device, password).await?;
        self.account = Account::Unlocked(Box::new(svc));
        self.next_screen_poll = Instant::now();
        self.idle.used(Instant::now());
        if let Err(e) = self.sync_now().await {
            tracing::warn!(error = %e, "vault: sync after unlock failed");
        }
        Ok(Reply::Done)
    }
}

/// A self-hosted server's URL must be an http(s) URL with a host.
fn check_server(server: &Server) -> Result<()> {
    if let Server::SelfHosted { url } = server {
        let ok = url::Url::parse(url.trim())
            .is_ok_and(|u| matches!(u.scheme(), "https" | "http") && u.host_str().is_some());
        if !ok {
            return Err(err(ErrorCode::Invalid, "The server must be an https:// address."));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_self_hosted_server_is_an_address() {
        assert!(check_server(&Server::Us).is_ok());
        assert!(check_server(&Server::SelfHosted { url: "https://vault.example.com".into() }).is_ok());
        assert!(check_server(&Server::SelfHosted { url: "vault.example.com".into() }).is_err());
        assert!(check_server(&Server::SelfHosted { url: "file:///etc".into() }).is_err());
    }
}
