//! This device's place in a Signal account: who it is, how it signs in, and
//! the account's keys the phone handed over when it linked.

use libsignal_core::{Aci, DeviceId, Pni};
use libsignal_protocol::IdentityKeyPair;

/// An account has two identities: its own (ACI) and its phone number's (PNI).
/// Each has an identity key, a registration id and pre-keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identity {
    Aci,
    Pni,
}

impl Identity {
    pub fn as_str(self) -> &'static str {
        match self {
            Identity::Aci => "aci",
            Identity::Pni => "pni",
        }
    }
}

/// One identity's key and registration id on this device.
#[derive(Clone)]
pub struct IdentityKeys {
    pub key_pair: IdentityKeyPair,
    pub registration_id: u32,
}

#[derive(Clone)]
pub struct Account {
    pub aci: Aci,
    pub device_id: DeviceId,
    /// What this device signs in with; made here, never shown.
    pub password: String,
    pub aci_keys: IdentityKeys,
    /// None for an account with no phone number.
    pub pni: Option<(Pni, IdentityKeys)>,
    pub number: Option<String>,
    pub profile_key: Vec<u8>,
    /// The root of the account's storage and backup keys.
    pub account_entropy_pool: Option<String>,
    pub media_root_backup_key: Option<Vec<u8>>,
    /// The key of the history archive the phone offers once, at link.
    pub ephemeral_backup_key: Option<Vec<u8>>,
    pub read_receipts: bool,
}
