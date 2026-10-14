//! Secret storage adapters (D007, D032).
//!
//! Secret values must never be logged or included in error messages; errors only
//! describe what went wrong, not what was stored.

use std::collections::HashMap;
use std::sync::Mutex;

/// Service name under which every LexTrail credential is stored (the app identifier).
pub const SERVICE: &str = "io.github.aspeternity.lextrail";

/// Identifies one secret. Constructed only through the named constructors so the
/// naming scheme stays in one place.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretKey(String);

impl SecretKey {
    /// API key of the AI provider with the given settings id.
    pub fn ai_api_key(provider_id: &str) -> Self {
        Self(format!("ai-api-key/{provider_id}"))
    }

    /// Token for the sync server.
    pub fn sync_token() -> Self {
        Self("sync-token".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("credential store is unavailable: {0}")]
    Unavailable(String),
    #[error("stored credential for {0} is unreadable")]
    Unreadable(String),
    #[error("credential store error: {0}")]
    Backend(String),
}

/// Where secrets are kept. Implementations must treat "not found" as `Ok(None)` on
/// read and as success on delete.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError>;
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError>;
    fn delete(&self, key: &SecretKey) -> Result<(), SecretError>;
}

/// In-memory store for tests and for running without a credential manager.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    values: Mutex<HashMap<SecretKey, String>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn values(&self) -> std::sync::MutexGuard<'_, HashMap<SecretKey, String>> {
        // A panic while holding the lock cannot leave the map half-updated.
        self.values.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError> {
        Ok(self.values().get(key).cloned())
    }

    fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError> {
        self.values().insert(key.clone(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.values().remove(key);
        Ok(())
    }
}

#[cfg(windows)]
pub use windows::WindowsCredentialStore;

#[cfg(windows)]
mod windows {
    use std::sync::Arc;

    use keyring_core::api::CredentialStoreApi;
    use keyring_core::{Entry, Error};
    use windows_native_keyring_store::Store;

    use super::{SERVICE, SecretError, SecretKey, SecretStore};

    /// Windows Credential Manager (generic credentials).
    #[derive(Debug)]
    pub struct WindowsCredentialStore {
        store: Arc<Store>,
    }

    impl WindowsCredentialStore {
        pub fn new() -> Result<Self, SecretError> {
            let store = Store::new().map_err(|e| SecretError::Unavailable(describe(&e)))?;
            Ok(Self { store })
        }

        fn entry(&self, key: &SecretKey) -> Result<Entry, SecretError> {
            self.store
                .build(SERVICE, key.as_str(), None)
                .map_err(|e| map_error(key, &e))
        }
    }

    impl SecretStore for WindowsCredentialStore {
        fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError> {
            match self.entry(key)?.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(Error::NoEntry) => Ok(None),
                Err(e) => Err(map_error(key, &e)),
            }
        }

        fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError> {
            self.entry(key)?
                .set_password(value)
                .map_err(|e| map_error(key, &e))
        }

        fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
            match self.entry(key)?.delete_credential() {
                Ok(()) | Err(Error::NoEntry) => Ok(()),
                Err(e) => Err(map_error(key, &e)),
            }
        }
    }

    fn map_error(key: &SecretKey, error: &Error) -> SecretError {
        match error {
            // These variants carry the stored bytes; never format them.
            Error::BadEncoding(_) | Error::BadDataFormat(..) => {
                SecretError::Unreadable(key.as_str().to_owned())
            }
            Error::NoStorageAccess(_) => SecretError::Unavailable(describe(error)),
            _ => SecretError::Backend(describe(error)),
        }
    }

    fn describe(error: &Error) -> String {
        match error {
            Error::BadEncoding(_) | Error::BadDataFormat(..) => "unreadable credential".into(),
            other => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Behaviour every `SecretStore` must share.
    fn exercise(store: &dyn SecretStore, key: &SecretKey) {
        assert_eq!(store.get(key).unwrap(), None);
        store.set(key, "sk-test-1").unwrap();
        assert_eq!(store.get(key).unwrap().as_deref(), Some("sk-test-1"));
        store.set(key, "sk-test-2 with ünïcode").unwrap();
        assert_eq!(
            store.get(key).unwrap().as_deref(),
            Some("sk-test-2 with ünïcode")
        );
        store.delete(key).unwrap();
        assert_eq!(store.get(key).unwrap(), None);
        // Deleting something that is not there is not an error.
        store.delete(key).unwrap();
    }

    #[test]
    fn key_names_are_stable() {
        // Changing these would orphan credentials users already saved.
        assert_eq!(
            SecretKey::ai_api_key("openai").as_str(),
            "ai-api-key/openai"
        );
        assert_eq!(SecretKey::sync_token().as_str(), "sync-token");
    }

    #[test]
    fn memory_store_behaves_like_a_secret_store() {
        exercise(&MemorySecretStore::new(), &SecretKey::ai_api_key("demo"));
    }

    #[test]
    fn memory_store_keeps_keys_separate() {
        let store = MemorySecretStore::new();
        store.set(&SecretKey::ai_api_key("a"), "1").unwrap();
        store.set(&SecretKey::sync_token(), "2").unwrap();
        assert_eq!(
            store.get(&SecretKey::ai_api_key("a")).unwrap().as_deref(),
            Some("1")
        );
        assert_eq!(store.get(&SecretKey::ai_api_key("b")).unwrap(), None);
    }

    /// Talks to the real Windows Credential Manager with a throwaway key.
    #[cfg(windows)]
    #[test]
    fn windows_credential_store_round_trip() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let key = SecretKey::ai_api_key(&format!("test-{}-{nanos}", std::process::id()));
        let store = WindowsCredentialStore::new().unwrap();
        exercise(&store, &key);
    }
}
