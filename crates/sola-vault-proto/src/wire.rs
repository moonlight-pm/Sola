//! Requests, replies and events on the vault's socket.

use serde::{Deserialize, Serialize};

use crate::draft::ItemDraft;
use crate::types::*;

/// A line on the wire: the request's id, echoed in its response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub id: u64,
    pub body: T,
}

/// What a client asks. Who may ask what is the daemon's peer check:
/// Vault, the browser and the shell everything; any other Sola app only
/// [`Request::Hello`] and [`Request::Redeem`] of a ticket made for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    /// First on every connection.
    Hello { version: u32, client: String },
    Status,
    Settings,
    SetSettings { settings: Settings },

    // Account
    Login { email: String, password: String, server: Server },
    /// The second step of the pending login.
    TwoFactor { kind: TwoFactorKind, code: String },
    /// Email a code for the pending login (or send it again).
    SendEmailCode { kind: TwoFactorKind },
    /// Unlock a signed-in, locked vault.
    Unlock { password: String },
    Lock,
    Logout,
    Sync { force: bool },
    /// Re-prompt: the master password, checked locally.
    VerifyMasterPassword { password: String },

    // Read
    /// Every item (trash included, marked). With `url`, its logins are
    /// marked [`ItemSummary::uri_match`].
    List { url: Option<String> },
    /// The logins for a page, best first.
    Matches { url: String },
    Get { id: String },
    /// An edit of the item, with its base.
    Draft { id: String },
    /// The item's code now.
    Code { id: String },
    Folders,
    Organizations,
    Collections,

    // Write
    /// A new item ([`ItemDraft::blank`], `id` empty).
    Create { draft: ItemDraft },
    Update { draft: ItemDraft },
    Trash { id: String },
    Restore { id: String },
    /// Forever.
    Delete { id: String },
    SetFavorite { id: String, favorite: bool },
    /// `None`: no folder.
    MoveToFolder { id: String, folder_id: Option<String> },
    /// Into an organization's collections (it can't come back out).
    ShareToOrganization { id: String, organization_id: String, collection_ids: Vec<String> },
    CreateFolder { name: String },
    RenameFolder { id: String, name: String },
    DeleteFolder { id: String },

    Generate { options: GeneratorOptions },
    Export { format: ExportFormat },

    // The browser
    /// What a page form gets for an item; `url` is the page's (the daemon
    /// checks a login matches it).
    FillMaterial { id: String, url: String },
    PasskeyCandidates { rp_id: String, public_key_json: String },
    /// WebAuthn get(): `origin` is the frame's, from CEF.
    PasskeyAssert { origin: String, public_key_json: String, cipher_id: String },
    /// WebAuthn create(): onto `cipher_id`, or a new login.
    PasskeyCreate { origin: String, public_key_json: String, cipher_id: Option<String> },

    // The desk menu
    /// What item `id` can put into a field, by label: never a value.
    Fields { id: String },
    /// A value for process `pid`'s focused field: a one-time ticket.
    Deliver { id: String, field: DeliverField, pid: u32 },
    /// The value, once, to the process it was made for.
    Redeem { ticket: String },

    /// The connection carries [`Event`]s from now on.
    Subscribe,
}

/// A request's answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Reply {
    Hello { version: u32 },
    Done,
    Status(Status),
    Settings(Settings),
    Login(LoginOutcome),
    Checked(bool),
    Items { items: Vec<ItemSummary>, unreadable: u32 },
    Item(ItemRecord),
    Draft(ItemDraft),
    Code { code: String, period: u32, remaining: u32 },
    Folders(Vec<Folder>),
    Organizations(Vec<Organization>),
    Collections(Vec<Collection>),
    /// The new or saved item's id.
    Saved { id: String },
    Generated(String),
    Exported { data: String },
    Fill(FillMaterial),
    Passkeys(Vec<PasskeyCandidate>),
    /// The WebAuthn response JSON for the page.
    Credential { json: String },
    Ticket { ticket: String },
    Value(String),
    /// [`Request::Fields`]: the item's values by label, in its record's
    /// order.
    Fields(Vec<FieldInfo>),
}

/// Why a request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    /// The daemon is not running or the socket is gone.
    Unavailable,
    /// This client may not ask that.
    Denied,
    SignedOut,
    Locked,
    BadPassword,
    /// The login needs its second step first.
    TwoFactor,
    NotFound,
    /// The server has a newer version: sync and try again.
    Conflict,
    /// Bitwarden could not be reached.
    Network,
    Invalid,
    Unsupported,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultError {
    pub code: ErrorCode,
    /// For a person: what went wrong.
    pub message: String,
}

impl VaultError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for VaultError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    Ok(Reply),
    Err(VaultError),
}

/// Pushed to subscribers. Never a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Status(Status),
    /// Items were added, changed or removed (a sync, a save).
    ItemsChanged { ids: Vec<String> },
    Folders,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_read_as_method_and_params() {
        let line = serde_json::to_string(&Envelope {
            id: 7,
            body: Request::Get { id: "x".into() },
        })
        .unwrap();
        assert_eq!(line, r#"{"id":7,"body":{"method":"get","params":{"id":"x"}}}"#);
        let unit = serde_json::to_string(&Request::Status).unwrap();
        assert_eq!(unit, r#"{"method":"status"}"#);
        let back: Envelope<Request> = serde_json::from_str(&line).unwrap();
        assert!(matches!(back.body, Request::Get { id } if id == "x"));
    }

    #[test]
    fn fields_carry_labels_never_values() {
        let req = serde_json::to_string(&Request::Fields { id: "x".into() }).unwrap();
        assert_eq!(req, r#"{"method":"fields","params":{"id":"x"}}"#);
        let reply = Reply::Fields(vec![FieldInfo {
            key: FIELD_TOTP.into(),
            label: "One-time code".into(),
            hidden: false,
        }]);
        let line = serde_json::to_string(&reply).unwrap();
        assert_eq!(line, r#"{"Fields":[{"key":"totp","label":"One-time code","hidden":false}]}"#);
    }
}
