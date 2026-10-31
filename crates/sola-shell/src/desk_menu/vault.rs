//! The vault as the desk menu asks it: on the vault's own socket
//! (`sola_vault_proto`), off the iced thread. The shell sees names and
//! usernames, never a value: a picked value is delivered by the daemon to
//! the app's process, and the app gets only a ticket to redeem.

use std::time::Duration;

use sola_bus::topics::{LaunchAppPayload, Topic};
use sola_vault_proto::{
    Client, DeliverField, ErrorCode, ItemKind, ItemSummary, Reply, Request, VaultError,
};

/// How the shell names itself to the daemon.
const CLIENT: &str = "sola-shell";

/// Vault's app id: Sign in, Unlock and Open in Vault bring it forward.
pub const VAULT_APP: &str = "sola-vault";

/// Search results shown at once.
pub const FOUND_MAX: usize = 8;

/// The vault, as Vault ▸ shows it.
#[derive(Debug, Clone, PartialEq)]
pub enum VaultView {
    Asking,
    /// The daemon is not running.
    Unavailable,
    SignedOut,
    Locked,
    /// Values can be delivered; the site's logins, best first (none
    /// without a site).
    Unlocked { logins: Vec<ItemSummary> },
    /// The daemon said why not, for a person.
    Failed(String),
}

/// Vault ▸ turned into a search over every item.
#[derive(Debug, Clone, Default)]
pub struct Search {
    pub query: String,
    /// `None` while the list is on its way.
    pub items: Option<Result<Vec<ItemSummary>, String>>,
}

impl Search {
    /// The logins the query finds, most recently used first.
    pub fn found(&self) -> Vec<&ItemSummary> {
        let Some(Ok(items)) = &self.items else {
            return Vec::new();
        };
        let mut found: Vec<&ItemSummary> = items
            .iter()
            .filter(|i| i.kind == ItemKind::Login && !i.deleted)
            .filter(|i| i.matches_query(&self.query))
            .collect();
        found.sort_by(|a, b| {
            b.last_used
                .cmp(&a.last_used)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        found.truncate(FOUND_MAX);
        found
    }
}

/// A delivery's ticket: never printed.
#[derive(Clone)]
pub struct Ticket(pub String);

impl std::fmt::Debug for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ticket(..)")
    }
}

/// Run `job` on a thread of its own; `None` if it went away.
pub async fn off_thread<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(job());
    });
    rx.await.ok()
}

/// The vault's state, and the logins for `site` when it is unlocked.
/// Blocking.
pub fn ask(site: Option<String>) -> VaultView {
    let mut client = match Client::connect(CLIENT) {
        Ok(c) => c,
        Err(e) => return refused(e),
    };
    let status = match client.call(Request::Status) {
        Ok(Reply::Status(s)) => s,
        Ok(_) => return VaultView::Failed("The vault answered something else.".into()),
        Err(e) => return refused(e),
    };
    if !status.signed_in {
        return VaultView::SignedOut;
    }
    if !status.unlocked {
        return VaultView::Locked;
    }
    let logins = match site {
        Some(url) => match client.call(Request::Matches { url }) {
            Ok(Reply::Items { items, .. }) => items.into_iter().filter(|i| !i.deleted).collect(),
            Ok(_) => Vec::new(),
            Err(e) => {
                tracing::warn!(code = ?e.code, error = %e, "desk menu: the site's logins");
                Vec::new()
            }
        },
        None => Vec::new(),
    };
    VaultView::Unlocked { logins }
}

/// Every item (summaries: names, never values). Blocking.
pub fn list() -> Result<Vec<ItemSummary>, String> {
    match sola_vault_proto::call(CLIENT, Request::List { url: None }) {
        Ok(Reply::Items { items, .. }) => Ok(items),
        Ok(_) => Err("The vault answered something else.".into()),
        Err(e) => Err(e.message),
    }
}

/// Ask the daemon to deliver `item`'s `field` to process `pid`: a ticket
/// for that process to redeem. Blocking.
pub fn deliver(item: String, field: DeliverField, pid: u32) -> Result<Ticket, String> {
    match sola_vault_proto::call(CLIENT, Request::Deliver { id: item, field, pid }) {
        Ok(Reply::Ticket { ticket }) => Ok(Ticket(ticket)),
        Ok(_) => Err("The vault answered something else.".into()),
        Err(e) => Err(e.message),
    }
}

/// How long `vault.open` has to answer before the shell starts Vault
/// itself.
const OPEN_CALL_WAIT: Duration = Duration::from_secs(3);

/// Show item `id` in Vault (a re-prompt item: its values wait for the
/// master password there). The service's `vault.open` checks the id and
/// has the session start or raise Vault on it; with no service to ask,
/// the session starts `sola-vault --item <id>` itself. Blocking.
pub fn open_item(id: String) {
    match sola_call::invoke("vault", "open", serde_json::json!({ "id": id }), OPEN_CALL_WAIT) {
        Ok(_) => tracing::info!("desk menu: an item opened in Vault"),
        Err(e) => {
            tracing::info!(error = %e, "desk menu: vault.open not answered; starting Vault");
            let Some(bus) = sola_kit::app::try_bus() else {
                return;
            };
            if let Ok(mut bus) = bus.lock() {
                let _ = bus.emit(Topic::LaunchApp(LaunchAppPayload {
                    app_id: VAULT_APP.into(),
                    command: open_command(&id),
                }));
            }
        }
    }
}

/// Vault at item `id`; an id that is not an item's (a UUID) is left off,
/// so nothing else reaches a command line.
fn open_command(id: &str) -> String {
    let bin = sola_core::paths::bin_path(VAULT_APP).display().to_string();
    let uuid = id.len() == 36
        && id
            .char_indices()
            .all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() });
    if uuid {
        format!("{bin} --item {id}")
    } else {
        bin
    }
}

fn refused(e: VaultError) -> VaultView {
    match e.code {
        ErrorCode::Unavailable => VaultView::Unavailable,
        ErrorCode::SignedOut => VaultView::SignedOut,
        ErrorCode::Locked => VaultView::Locked,
        _ => VaultView::Failed(e.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk_menu::rows::tests::login;

    #[test]
    fn search_finds_logins_most_recent_first() {
        let mut a = login("alpha", false);
        a.last_used = 10;
        let mut b = login("beta", false);
        b.last_used = 20;
        let mut note = login("alpha-note", false);
        note.kind = ItemKind::SecureNote;
        let mut gone = login("alpha-old", false);
        gone.deleted = true;
        let mut s = Search {
            query: String::new(),
            items: Some(Ok(vec![a, b, note, gone])),
        };
        let ids = |s: &Search| s.found().iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&s), ["beta", "alpha"]);
        s.query = "alp".into();
        assert_eq!(ids(&s), ["alpha"]);
        s.items = None;
        assert!(s.found().is_empty());
    }

    #[test]
    fn only_an_item_id_reaches_the_command_line() {
        assert!(open_command("11111111-aaaa-1111-1111-111111111111").ends_with("sola-vault --item 11111111-aaaa-1111-1111-111111111111"));
        assert!(open_command("x; rm -rf ~").ends_with("sola-vault"));
    }

    #[test]
    fn a_ticket_never_prints() {
        assert_eq!(format!("{:?}", Ticket("t-123".into())), "Ticket(..)");
    }

    #[test]
    fn refusals_read_as_states() {
        let e = |code| VaultError::new(code, "why");
        assert_eq!(refused(e(ErrorCode::Unavailable)), VaultView::Unavailable);
        assert_eq!(refused(e(ErrorCode::Locked)), VaultView::Locked);
        assert_eq!(refused(e(ErrorCode::Denied)), VaultView::Failed("why".into()));
    }
}
