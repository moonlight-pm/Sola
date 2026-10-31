//! What the vault's clients see: items, records, folders, settings.
//!
//! Summaries never carry a secret (no password, card number, code key or
//! note). A record does: only Vault, the browser and the shell may ask for
//! one, and it is zeroized when dropped.

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::totp::TotpSpec;

/// Which Bitwarden the account lives on (V5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Server {
    /// bitwarden.com (US).
    #[default]
    Us,
    /// bitwarden.eu.
    Eu,
    /// A self-hosted server's base URL (`https://vault.example.com`).
    SelfHosted { url: String },
}

impl Server {
    /// The web vault, for links and display.
    pub fn web_vault(&self) -> String {
        match self {
            Self::Us => "https://vault.bitwarden.com".into(),
            Self::Eu => "https://vault.bitwarden.eu".into(),
            Self::SelfHosted { url } => url.trim_end_matches('/').to_string(),
        }
    }

    pub fn api_url(&self) -> String {
        match self {
            Self::Us => "https://api.bitwarden.com".into(),
            Self::Eu => "https://api.bitwarden.eu".into(),
            Self::SelfHosted { url } => format!("{}/api", url.trim_end_matches('/')),
        }
    }

    pub fn identity_url(&self) -> String {
        match self {
            Self::Us => "https://identity.bitwarden.com".into(),
            Self::Eu => "https://identity.bitwarden.eu".into(),
            Self::SelfHosted { url } => format!("{}/identity", url.trim_end_matches('/')),
        }
    }

    /// A short name: `bitwarden.com`, `bitwarden.eu`, the host.
    pub fn label(&self) -> String {
        match self {
            Self::Us => "bitwarden.com".into(),
            Self::Eu => "bitwarden.eu".into(),
            Self::SelfHosted { url } => url
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/')
                .to_string(),
        }
    }
}

/// The vault as the daemon sees it now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Status {
    /// An account is signed in (locked or not).
    pub signed_in: bool,
    pub unlocked: bool,
    pub email: String,
    pub server: Server,
    /// Unix seconds of the sync whose items the vault holds: the last
    /// that worked, or, after an unlock without network, the one kept on
    /// disk. `None` while unlocked: no items until it syncs.
    pub synced_at: Option<u64>,
    pub syncing: bool,
    pub sync_error: Option<String>,
    /// Items (trash not counted); 0 while locked.
    pub items: u32,
    /// Items that could not be decrypted (an organization key that did
    /// not load).
    pub unreadable: u32,
    /// A login waits for its second step ([`LoginOutcome::NeedsTwoFactor`]).
    #[serde(default)]
    pub two_factor_pending: bool,
}

/// A second step Bitwarden asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TwoFactorKind {
    /// New-device protection: Bitwarden emailed a code on its own.
    NewDevice,
    /// The account's email two-step login.
    Email,
    /// An authenticator app's code.
    Authenticator,
}

/// What a login or a second step came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoginOutcome {
    Unlocked,
    NeedsTwoFactor {
        /// What this account offers, supported kinds only.
        kinds: Vec<TwoFactorKind>,
        preferred: TwoFactorKind,
        /// The masked address a code went (or would go) to.
        email_hint: Option<String>,
        /// A code was emailed already.
        email_sent: bool,
        /// Providers the account has that Sola cannot do (security key,
        /// Duo, YubiKey OTP), for a message.
        unsupported: Vec<String>,
    },
}

/// A Bitwarden item's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemKind {
    Login,
    Card,
    Identity,
    SecureNote,
    SshKey,
    BankAccount,
    DriversLicense,
    Passport,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Login => "Login",
            Self::Card => "Card",
            Self::Identity => "Identity",
            Self::SecureNote => "Note",
            Self::SshKey => "SSH key",
            Self::BankAccount => "Bank account",
            Self::DriversLicense => "License",
            Self::Passport => "Passport",
        }
    }

    /// A page form can take it.
    pub fn can_fill(self) -> bool {
        matches!(self, Self::Login | Self::Card | Self::Identity)
    }
}

/// One row of the vault — never a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSummary {
    pub id: String,
    pub kind: ItemKind,
    pub name: String,
    /// Username, card brand and last four, a person's name.
    pub subtitle: String,
    /// Unix seconds; 0 when unknown.
    pub last_used: i64,
    /// Unix seconds of its last change.
    #[serde(default)]
    pub revised: i64,
    pub has_totp: bool,
    pub has_passkey: bool,
    /// A website of this login matches the URL the list was asked for.
    pub uri_match: bool,
    /// Asks for the master password before it opens or fills.
    pub reprompt: bool,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub folder_id: Option<String>,
    #[serde(default)]
    pub organization_id: Option<String>,
    #[serde(default)]
    pub collection_ids: Vec<String>,
    /// In the trash.
    #[serde(default)]
    pub deleted: bool,
    /// Lowercased text to search: name, usernames, websites, notes'
    /// first words — never a password.
    #[serde(default)]
    pub search: String,
}

impl ItemSummary {
    /// Every word of `query` is in the item.
    pub fn matches_query(&self, query: &str) -> bool {
        let q = query.trim();
        if q.is_empty() {
            return true;
        }
        let hay = if self.search.is_empty() {
            format!("{} {}", self.name, self.subtitle).to_lowercase()
        } else {
            self.search.clone()
        };
        q.split_whitespace()
            .all(|w| hay.contains(&w.to_lowercase()))
    }
}

/// One labelled value on a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordField {
    /// Stable key: `username`, `password`, `uri0`, `card_number`,
    /// `custom:<index>`, ...
    pub key: String,
    pub label: String,
    pub value: String,
    /// A secret: shown as dots until revealed.
    pub hidden: bool,
    /// Monospaced (numbers, keys).
    pub mono: bool,
}

/// How a website of a login matches a page (Bitwarden's `UriMatchType`;
/// `None` on [`UriEntry`] means the account's default, Domain).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UriMatch {
    Domain,
    Host,
    StartsWith,
    Exact,
    RegularExpression,
    Never,
}

impl UriMatch {
    pub fn label(self) -> &'static str {
        match self {
            Self::Domain => "Domain",
            Self::Host => "Host",
            Self::StartsWith => "Starts with",
            Self::Exact => "Exact",
            Self::RegularExpression => "Regular expression",
            Self::Never => "Never",
        }
    }

    pub fn all() -> [UriMatch; 6] {
        [
            Self::Domain,
            Self::Host,
            Self::StartsWith,
            Self::Exact,
            Self::RegularExpression,
            Self::Never,
        ]
    }
}

/// A website of a login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UriEntry {
    pub uri: String,
    pub match_type: Option<UriMatch>,
}

/// A passkey on a login (no key material).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasskeyInfo {
    pub rp_id: String,
    pub user_name: Option<String>,
    pub user_display_name: Option<String>,
    /// RFC 3339.
    pub created: Option<String>,
}

/// A password the login had before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub password: String,
    /// RFC 3339: when it stopped being the password.
    pub last_used: String,
}

impl Drop for HistoryEntry {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

/// A file attached to an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub file_name: String,
    /// Human size (`12.3 KB`).
    pub size: String,
}

/// A whole item, decrypted. Zeroized when dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRecord {
    pub id: String,
    pub kind: ItemKind,
    pub name: String,
    pub notes: Option<String>,
    /// The kind's own fields, in order, then custom fields.
    pub fields: Vec<RecordField>,
    /// The raw authenticator key (`otpauth://` or a secret) when present,
    /// whether or not [`TotpSpec`] can read it.
    pub totp_secret: Option<String>,
    pub totp_period: u32,
    pub has_passkey: bool,
    /// The server lets this account write it.
    pub can_edit: bool,
    /// False when an organization hides the password from this account.
    pub view_password: bool,
    #[serde(default)]
    pub reprompt: bool,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub folder_id: Option<String>,
    #[serde(default)]
    pub organization_id: Option<String>,
    #[serde(default)]
    pub collection_ids: Vec<String>,
    #[serde(default)]
    pub uris: Vec<UriEntry>,
    #[serde(default)]
    pub passkeys: Vec<PasskeyInfo>,
    #[serde(default)]
    pub password_history: Vec<HistoryEntry>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// RFC 3339.
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub revised: Option<String>,
    /// In the trash since (RFC 3339).
    #[serde(default)]
    pub deleted: Option<String>,
}

impl ItemRecord {
    pub fn can_fill(&self) -> bool {
        self.kind.can_fill()
    }

    /// The code at `unix_secs` and its period, when the key is readable.
    pub fn totp_code_at(&self, unix_secs: u64) -> Option<(String, u32)> {
        let spec = TotpSpec::parse(self.totp_secret.as_deref()?)?;
        Some((spec.code_at(unix_secs), spec.period))
    }

    /// A stored authenticator key no code can be made from (another
    /// algorithm, Steam, a typo): shown as it is, never hidden.
    pub fn unreadable_totp(&self) -> Option<&str> {
        self.totp_secret
            .as_deref()
            .filter(|raw| TotpSpec::parse(raw).is_none())
    }

    /// A field's value by key.
    pub fn field(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.value.as_str())
    }
}

impl Drop for ItemRecord {
    fn drop(&mut self) {
        if let Some(s) = self.totp_secret.as_mut() {
            s.zeroize();
        }
        if let Some(n) = self.notes.as_mut() {
            n.zeroize();
        }
        for f in &mut self.fields {
            if f.hidden {
                f.value.zeroize();
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Organization {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub id: String,
    pub organization_id: String,
    pub name: String,
    /// This account cannot add or change items in it.
    pub read_only: bool,
}

/// Which value of an item a delivery carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliverField {
    Username,
    Password,
    /// The authenticator code now.
    Code,
    /// Any field of the record by its [`RecordField::key`].
    Field { key: String },
}

/// What the generator makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GeneratorKind {
    #[default]
    Password,
    Passphrase,
    Username,
}

/// The generator's settings (Bitwarden's defaults).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratorOptions {
    pub kind: GeneratorKind,
    /// Password length (5–128).
    pub length: u8,
    pub lowercase: bool,
    pub uppercase: bool,
    pub numbers: bool,
    pub special: bool,
    pub min_numbers: u8,
    pub min_special: u8,
    pub avoid_ambiguous: bool,
    /// Passphrase words (3–20).
    pub words: u8,
    pub separator: String,
    pub capitalize: bool,
    pub include_number: bool,
}

impl Default for GeneratorOptions {
    fn default() -> Self {
        Self {
            kind: GeneratorKind::Password,
            length: 16,
            lowercase: true,
            uppercase: true,
            numbers: true,
            special: true,
            min_numbers: 1,
            min_special: 1,
            avoid_ambiguous: false,
            words: 4,
            separator: "-".into(),
            capitalize: false,
            include_number: false,
        }
    }
}

/// What a page form gets. The browser zeroizes it after the fill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillMaterial {
    Login {
        username: Option<String>,
        password: Option<String>,
        /// The code now, when the login has a readable key.
        code: Option<String>,
        /// Its websites: frames they match may be filled too.
        uris: Vec<UriEntry>,
    },
    Card {
        cardholder_name: Option<String>,
        number: Option<String>,
        exp_month: Option<String>,
        exp_year: Option<String>,
        code: Option<String>,
        brand: Option<String>,
    },
    Identity(IdentityFill),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IdentityFill {
    pub title: Option<String>,
    pub first_name: Option<String>,
    pub middle_name: Option<String>,
    pub last_name: Option<String>,
    pub address1: Option<String>,
    pub address2: Option<String>,
    pub address3: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub company: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub ssn: Option<String>,
    pub username: Option<String>,
    pub passport_number: Option<String>,
    pub license_number: Option<String>,
}

/// A passkey a site's sign-in could use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasskeyCandidate {
    pub cipher_id: String,
    pub name: String,
    pub username: Option<String>,
    pub rp_id: String,
    pub user_display_name: Option<String>,
    pub reprompt: bool,
}

/// How an export is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    /// Bitwarden JSON, plain.
    Json,
    /// Bitwarden JSON, encrypted with a password of its own.
    EncryptedJson { password: String },
    Csv,
}

/// The daemon's settings, edited in Vault → Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Lock after this many idle minutes; `None` = only by the rules
    /// below and at a restart.
    pub idle_lock_minutes: Option<u32>,
    /// Lock when the screen locks.
    pub lock_on_screen_lock: bool,
    /// Clear a copied value from the clipboard after this many seconds
    /// (0 = never).
    pub clipboard_clear_secs: u32,
    /// Filling a login that has a code also copies the code (V6).
    pub copy_code_on_fill: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            idle_lock_minutes: Some(60),
            lock_on_screen_lock: true,
            clipboard_clear_secs: 30,
            copy_code_on_fill: true,
        }
    }
}
