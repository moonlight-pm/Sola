//! IMAP/SMTP/IDLE protocol layer (lifted from apocrypha mail).

pub mod account;
pub mod html_text;
pub mod idle;
pub mod imap;
pub mod letter;
#[cfg(test)]
pub mod links;
pub mod rules;
pub mod sender;
pub mod types;
pub mod wicket;

pub use account::Account;
pub use idle::{IdleChange, IdleHandle, start_idle};
pub use imap::ImapClient;
pub use letter::{ProseBlock, ProseRun, flatten, parse_plain, visible_text};
pub use rules::rule_matches;
pub use types::{Folder, MessageBody, MessageSummary, folder_count_badge, folder_label};
