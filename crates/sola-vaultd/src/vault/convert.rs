//! The SDK's enums as the socket names them, and back.

use bitwarden_vault::{CipherType, UriMatchType};
use sola_vault_proto::{ItemKind, UriMatch};

pub fn kind(t: CipherType) -> ItemKind {
    match t {
        CipherType::Login => ItemKind::Login,
        CipherType::Card => ItemKind::Card,
        CipherType::Identity => ItemKind::Identity,
        CipherType::SecureNote => ItemKind::SecureNote,
        CipherType::SshKey => ItemKind::SshKey,
        CipherType::BankAccount => ItemKind::BankAccount,
        CipherType::DriversLicense => ItemKind::DriversLicense,
        CipherType::Passport => ItemKind::Passport,
    }
}

pub fn cipher_type(k: ItemKind) -> CipherType {
    match k {
        ItemKind::Login => CipherType::Login,
        ItemKind::Card => CipherType::Card,
        ItemKind::Identity => CipherType::Identity,
        ItemKind::SecureNote => CipherType::SecureNote,
        ItemKind::SshKey => CipherType::SshKey,
        ItemKind::BankAccount => CipherType::BankAccount,
        ItemKind::DriversLicense => CipherType::DriversLicense,
        ItemKind::Passport => CipherType::Passport,
    }
}

pub fn uri_match(m: UriMatchType) -> UriMatch {
    match m {
        UriMatchType::Domain => UriMatch::Domain,
        UriMatchType::Host => UriMatch::Host,
        UriMatchType::StartsWith => UriMatch::StartsWith,
        UriMatchType::Exact => UriMatch::Exact,
        UriMatchType::RegularExpression => UriMatch::RegularExpression,
        UriMatchType::Never => UriMatch::Never,
    }
}

pub fn uri_match_type(m: UriMatch) -> UriMatchType {
    match m {
        UriMatch::Domain => UriMatchType::Domain,
        UriMatch::Host => UriMatchType::Host,
        UriMatch::StartsWith => UriMatchType::StartsWith,
        UriMatch::Exact => UriMatchType::Exact,
        UriMatch::RegularExpression => UriMatchType::RegularExpression,
        UriMatch::Never => UriMatchType::Never,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_matches_round_trip() {
        for k in [
            ItemKind::Login,
            ItemKind::Card,
            ItemKind::Identity,
            ItemKind::SecureNote,
            ItemKind::SshKey,
            ItemKind::BankAccount,
            ItemKind::DriversLicense,
            ItemKind::Passport,
        ] {
            assert_eq!(kind(cipher_type(k)), k);
        }
        for m in UriMatch::all() {
            assert_eq!(uri_match(uri_match_type(m)), m);
        }
    }
}
