//! The SDK client: built per server and device, with the session-scoped
//! stores and the sync handlers a vault needs.

use std::sync::Arc;

use bitwarden_core::auth::ClientManagedTokenHandler;
use bitwarden_core::client::ClientBuilder;
use bitwarden_core::client::persisted_state::OrganizationSharedKey;
use bitwarden_core::key_management::{
    BLOB_SECURITY_VERSION, KeySlotIds, LocalUserDataKeyState, UserKeyState,
};
use bitwarden_core::{Client, ClientSettings, DeviceType, FromClient};
use bitwarden_crypto::KeyStore;
use bitwarden_crypto_sync_handler::CryptoSyncHandler;
use bitwarden_pm::PasswordManagerClient;
use bitwarden_sync::{SyncClient, SyncHandler};
use bitwarden_vault::{Cipher, Folder, FolderSyncHandler};
use sola_vault_proto::Server;

use super::identity::TokenCell;
use super::memory_repo::MemoryRepository;
use super::org_crypto::OrgCryptoSyncHandler;
use super::sync_cipher::CipherSyncHandler;
use super::sync_orgs::{SyncOrgs, SyncOrgsHandler};

/// Sent as `Bitwarden-Client-Version`. The cloud parses it as `YYYY.M.P`;
/// the crate's own 0.1.0 makes PUT /ciphers answer "Update to the latest
/// version of Bitwarden" on blob and passkey items. This SDK pin is the
/// 2026.8 line.
pub const BITWARDEN_CLIENT_VERSION: &str = "2026.8.0";

/// Keystore version after V1 account init. Decrypt enforces a login-URI
/// checksum only when that cipher has its own key.
pub const READ_SECURITY_STATE_VERSION: u64 = 1;

/// One client with what it reads and writes.
pub struct Built {
    pub pm: PasswordManagerClient,
    /// Long-lived: the handlers live on it. `pm.sync()` makes a new, empty
    /// one each call — handlers registered on that never run.
    pub sync: SyncClient,
    pub tokens: Arc<TokenCell>,
    pub ciphers: Arc<MemoryRepository<Cipher>>,
    pub folders: Arc<MemoryRepository<Folder>>,
    pub orgs: Arc<SyncOrgs>,
    /// The handlers that fill the stores from a sync, in order: replayed
    /// on a cached sync when an unlock cannot reach Bitwarden.
    pub replay: Vec<Arc<dyn SyncHandler>>,
}

pub fn settings(server: &Server, device_id: &str) -> ClientSettings {
    ClientSettings {
        identity_url: server.identity_url(),
        api_url: server.api_url(),
        user_agent: format!("SolaVault/{}", env!("CARGO_PKG_VERSION")),
        device_type: DeviceType::LinuxDesktop,
        device_identifier: Some(device_id.to_string()),
        bitwarden_client_version: Some(BITWARDEN_CLIENT_VERSION.into()),
        bitwarden_package_type: Some("desktop".into()),
        ..ClientSettings::default()
    }
}

pub fn build(server: &Server, device_id: &str) -> Built {
    let tokens = Arc::new(TokenCell::default());
    let client = ClientBuilder::new()
        .with_settings(settings(server, device_id))
        .with_token_handler(ClientManagedTokenHandler::new(tokens.clone()))
        .build();

    // Session-scoped stores: without them sync handlers have nowhere to
    // write and `ciphers().get_all()` is always empty.
    let ciphers = Arc::new(MemoryRepository::<Cipher>::default());
    let folders = Arc::new(MemoryRepository::<Folder>::default());
    let state = client.platform().state();
    state.register_client_managed(ciphers.clone());
    state.register_client_managed(folders.clone());
    state.register_client_managed(Arc::new(MemoryRepository::<UserKeyState>::default()));
    state.register_client_managed(Arc::new(MemoryRepository::<LocalUserDataKeyState>::default()));
    state.register_client_managed(Arc::new(MemoryRepository::<OrganizationSharedKey>::default()));

    let pm = PasswordManagerClient(client);
    let sync = pm.sync();
    let orgs = Arc::new(SyncOrgs::default());
    // Key management first (not replayed: the session holds the keys).
    sync.register_sync_handler(Arc::new(CryptoSyncHandler::new(pm.0.clone())));
    let replay: Vec<Arc<dyn SyncHandler>> = vec![
        Arc::new(FolderSyncHandler::from_client(&pm.0)),
        Arc::new(CipherSyncHandler::from_client(&pm.0)),
        // Org keys after ciphers are stored; decrypt happens later in get_all().
        Arc::new(OrgCryptoSyncHandler::new(pm.0.clone())),
        Arc::new(SyncOrgsHandler(orgs.clone())),
    ];
    for handler in &replay {
        sync.register_sync_handler(handler.clone());
    }
    Built {
        pm,
        sync,
        tokens,
        ciphers,
        folders,
        orgs,
        replay,
    }
}

/// While alive, personal cipher writes use blob encryption (security
/// state 2); drop restores [`READ_SECURITY_STATE_VERSION`].
///
/// State 2 also makes decrypt discard login URIs that lack a checksum,
/// which official clients only do when the cipher has its own key. Hold
/// this across `ciphers().encrypt` (and the FIDO calls that encrypt inside
/// the SDK), never across `get` / `get_all` / sync.
pub struct BlobCipherWrites {
    store: KeyStore<KeySlotIds>,
}

impl BlobCipherWrites {
    pub fn enter(client: &Client) -> Self {
        let store = client.internal.get_key_store().clone();
        store.set_security_state_version(BLOB_SECURITY_VERSION);
        Self { store }
    }
}

impl Drop for BlobCipherWrites {
    fn drop(&mut self) {
        self.store
            .set_security_state_version(READ_SECURITY_STATE_VERSION);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_cipher_writes_raise_security_state_only_while_held() {
        let client = Client::new(None);
        let store = client.internal.get_key_store().clone();
        assert_eq!(
            store.context().get_security_state_version(),
            READ_SECURITY_STATE_VERSION
        );
        {
            let _blob = BlobCipherWrites::enter(&client);
            assert_eq!(
                store.context().get_security_state_version(),
                BLOB_SECURITY_VERSION
            );
        }
        assert_eq!(
            store.context().get_security_state_version(),
            READ_SECURITY_STATE_VERSION
        );
    }

    #[test]
    fn bitwarden_client_version_is_yyyy_m() {
        let y: u32 = BITWARDEN_CLIENT_VERSION
            .split('.')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(y >= 2026, "cloud rejects crate version 0.1.0: {BITWARDEN_CLIENT_VERSION}");
    }

    #[test]
    fn settings_follow_the_server() {
        let s = settings(&Server::Eu, "dev");
        assert_eq!(s.api_url, "https://api.bitwarden.eu");
        assert_eq!(s.identity_url, "https://identity.bitwarden.eu");
        let s = settings(
            &Server::SelfHosted {
                url: "https://vault.example.com/".into(),
            },
            "dev",
        );
        assert_eq!(s.api_url, "https://vault.example.com/api");
        assert_eq!(s.identity_url, "https://vault.example.com/identity");
    }
}
