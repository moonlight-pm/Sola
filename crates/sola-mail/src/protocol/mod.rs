//! IMAP/SMTP/IDLE protocol layer (lifted from apocrypha mail).

pub mod account;
pub mod html_text;
pub mod idle;
pub mod imap;
pub mod rules;
pub mod sender;
pub mod types;
pub mod wicket;

pub use account::Account;
pub use idle::{IdleHandle, start_idle};
pub use imap::ImapClient;
pub use rules::rule_matches;
pub use types::{Folder, MessageBody, MessageSummary};
