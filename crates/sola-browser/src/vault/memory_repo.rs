//! In-process repository for Bitwarden SDK items (session lifetime only).
//!
//! Mirrors `bitwarden_test::MemoryRepository` without depending on the test
//! crate. Required so cipher/folder sync has somewhere to write and
//! `vault().ciphers().get_all()` has somewhere to read.

use std::collections::HashMap;
use std::sync::Mutex;

use bitwarden_state::repository::{Repository, RepositoryError, RepositoryItem};

pub struct MemoryRepository<V: RepositoryItem> {
    store: Mutex<HashMap<String, V>>,
}

impl<V: RepositoryItem + Clone> Default for MemoryRepository<V> {
    fn default() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl<V: RepositoryItem + Clone> Repository<V> for MemoryRepository<V> {
    async fn get(&self, key: V::Key) -> Result<Option<V>, RepositoryError> {
        let store = self
            .store
            .lock()
            .map_err(|e| RepositoryError::Internal(e.to_string()))?;
        Ok(store.get(&key.to_string()).cloned())
    }

    async fn list(&self) -> Result<Vec<V>, RepositoryError> {
        let store = self
            .store
            .lock()
            .map_err(|e| RepositoryError::Internal(e.to_string()))?;
        Ok(store.values().cloned().collect())
    }

    async fn set(&self, key: V::Key, value: V) -> Result<(), RepositoryError> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| RepositoryError::Internal(e.to_string()))?;
        store.insert(key.to_string(), value);
        Ok(())
    }

    async fn set_bulk(&self, values: Vec<(V::Key, V)>) -> Result<(), RepositoryError> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| RepositoryError::Internal(e.to_string()))?;
        for (key, value) in values {
            store.insert(key.to_string(), value);
        }
        Ok(())
    }

    async fn remove(&self, key: V::Key) -> Result<(), RepositoryError> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| RepositoryError::Internal(e.to_string()))?;
        store.remove(&key.to_string());
        Ok(())
    }

    async fn remove_bulk(&self, keys: Vec<V::Key>) -> Result<(), RepositoryError> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| RepositoryError::Internal(e.to_string()))?;
        for key in keys {
            store.remove(&key.to_string());
        }
        Ok(())
    }

    async fn remove_all(&self) -> Result<(), RepositoryError> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| RepositoryError::Internal(e.to_string()))?;
        store.clear();
        Ok(())
    }
}
