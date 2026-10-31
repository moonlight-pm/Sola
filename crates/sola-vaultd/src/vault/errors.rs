//! The socket's errors ([`VaultError`]) from what Bitwarden and the SDK
//! say. Messages are for a person; none carries a secret.

use bitwarden_core::ApiError;
use sola_vault_proto::{ErrorCode, VaultError};

pub type Result<T> = std::result::Result<T, VaultError>;

pub fn err(code: ErrorCode, message: impl Into<String>) -> VaultError {
    VaultError::new(code, message)
}

pub fn internal(message: impl Into<String>) -> VaultError {
    err(ErrorCode::Internal, message)
}

pub fn not_found() -> VaultError {
    err(ErrorCode::NotFound, "No such item.")
}

pub fn locked() -> VaultError {
    err(ErrorCode::Locked, "The vault is locked.")
}

pub fn signed_out() -> VaultError {
    err(ErrorCode::SignedOut, "No account is signed in.")
}

/// An API call `op` failed: unreachable, gone, refused or stale.
pub fn api(op: &str, e: ApiError) -> VaultError {
    match e {
        ApiError::Reqwest(_) | ApiError::ReqwestMiddleware(_) | ApiError::Io(_) => {
            tracing::warn!(op, error = %e, "vault: Bitwarden unreachable");
            err(ErrorCode::Network, format!("Bitwarden could not be reached ({op})."))
        }
        ApiError::Response(rc) if rc.status.as_u16() == 404 => not_found(),
        ApiError::Response(rc) => refused(op, &format!("{}: {}", rc.status.as_u16(), rc.message)),
        ApiError::Serde(e) => internal(format!("{op}: {e}")),
    }
}

/// Bitwarden refused a write: a newer copy on the server, a client it
/// thinks too old, or something else (kept as said).
pub fn refused(op: &str, said: &str) -> VaultError {
    let lower = said.to_lowercase();
    if lower.contains("update to the latest version") {
        err(
            ErrorCode::Invalid,
            "Bitwarden rejected this save (client too old for the item format).",
        )
    } else if lower.contains("out of date")
        || lower.contains("outdated")
        || lower.contains("revision")
        || (lower.contains("version") && lower.contains("old"))
    {
        err(
            ErrorCode::Conflict,
            "Bitwarden has a newer copy of this item (often after a passkey was used \
             elsewhere). Try Save again.",
        )
    } else {
        tracing::warn!(op, said, "vault: Bitwarden refused");
        err(ErrorCode::Internal, format!("{op}: {said}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stale_revision_is_a_conflict() {
        let e = refused("update item", "Cipher revisionDate is outdated.");
        assert_eq!(e.code, ErrorCode::Conflict);
        assert!(e.message.contains("newer copy"), "{e}");
        // Bitwarden's own wording (CipherService.ValidateCipherLastKnownRevisionDate).
        let e = refused(
            "update item",
            r#"400: {"message":"The cipher you are updating is out of date. Please save your work, sync your vault, and try again."}"#,
        );
        assert_eq!(e.code, ErrorCode::Conflict);
        let e = refused("update item", "the version is old");
        assert_eq!(e.code, ErrorCode::Conflict);
    }

    #[test]
    fn other_refusals_keep_their_words() {
        let e = refused("update item", "network down");
        assert_eq!(e.code, ErrorCode::Internal);
        assert!(e.message.contains("update item: network down"));
        let e = refused(
            "update item",
            r#"400: {"message":"Cannot edit item. Update to the latest version of Bitwarden and try again."}"#,
        );
        assert!(e.message.contains("client too old"), "{e}");
    }
}
