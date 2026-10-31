//! The Bitwarden core: sign-in, unlock, sync, items, matching, codes,
//! passkey signing, the generator and export — by Bitwarden's SDK, on the
//! socket's types (`sola_vault_proto`). Moved here from the browser.

mod card;
mod client;
mod convert;
mod edit;
pub mod errors;
pub mod export;
pub mod folders;
pub mod generate;
mod identity;
mod item;
pub mod match_uri;
mod memory_repo;
mod new_item;
mod org_crypto;
pub mod passkey;
pub mod read;
mod scope;
pub mod service;
pub mod session;
mod sync_cipher;
mod sync_orgs;
pub mod write;

pub use service::{Login, VaultService};
pub use session::Session;
