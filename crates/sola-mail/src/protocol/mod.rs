//! IMAP/SMTP/IDLE protocol layer (lifted from apocrypha mail).

pub mod account;
pub mod attachments;
pub mod boxes;
pub mod date;
pub mod from;
pub mod html_text;
pub mod idle;
pub mod imap;
#[cfg(test)]
pub mod links;
pub mod rules;
pub mod sender;
pub mod types;
pub mod wicket;

pub use account::Account;
pub use from::pick_from_for_reply;
pub use idle::{IdleChange, IdleHandle, start_idle};
pub use imap::ImapClient;
pub use rules::rule_matches;
pub use types::{
    Folder, MailAttachment, MailId, MessageBody, MessageSummary, folder_count_badge, folder_label,
};
