//! IMAP/SMTP/IDLE protocol layer.

pub mod account;
pub mod attachments;
pub mod boxes;
pub mod date;
mod dest_uid;
pub mod from;
pub mod html;
pub mod idle;
pub mod imap;
pub mod parse;
pub mod picture;
pub mod recipients;
pub mod rules;
pub mod sender;
pub mod types;

pub use account::Account;
pub use idle::{IdleChange, IdleHandle, start_idle};
pub use imap::ImapClient;
pub use types::{
    Folder, FolderRef, LetterImage, MailAttachment, MailId, MessageBody, MessageSummary,
};
