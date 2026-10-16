//! Secret storage abstraction and Keyring implementation.

use anyhow::{Context, Result};

/// Trait for secret storage. Implementations must be thread-safe.
pub trait SecretStore: Send + Sync {
    /// Get the secret for a profile. Returns Ok(None) if no entry exists.
    fn get(&self, profile_id: &str) -> Result<Option<String>>;

    /// Set the secret for a profile.
    fn set(&self, profile_id: &str, password: &str) -> Result<()>;

    /// Delete the secret for a profile. Returns Ok(()) if no entry existed.
    fn delete(&self, profile_id: &str) -> Result<()>;
}

/// Keyring-based secret store using the OS credential manager.
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new() -> Self {
        Self {
            service: "pgTool".to_string(),
        }
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, profile_id: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(&self.service, profile_id).context("open keyring entry")?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow::Error::new(e).context("keyring get_password failed")),
        }
    }

    fn set(&self, profile_id: &str, password: &str) -> Result<()> {
        let entry = keyring::Entry::new(&self.service, profile_id).context("open keyring entry")?;
        entry
            .set_password(password)
            .context("keyring set_password failed")?;
        Ok(())
    }

    fn delete(&self, profile_id: &str) -> Result<()> {
        let entry = keyring::Entry::new(&self.service, profile_id).context("open keyring entry")?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow::Error::new(e).context("keyring delete_credential failed")),
        }
    }
}

/// No-op secret store for testing or headless environments.
#[cfg(test)]
pub struct MockSecretStore {
    store: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

#[cfg(test)]
impl MockSecretStore {
    pub fn new() -> Self {
        Self {
            store: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

#[cfg(test)]
impl SecretStore for MockSecretStore {
    fn get(&self, profile_id: &str) -> Result<Option<String>> {
        Ok(self.store.lock().unwrap().get(profile_id).cloned())
    }

    fn set(&self, profile_id: &str, password: &str) -> Result<()> {
        self.store
            .lock()
            .unwrap()
            .insert(profile_id.to_string(), password.to_string());
        Ok(())
    }

    fn delete(&self, profile_id: &str) -> Result<()> {
        self.store.lock().unwrap().remove(profile_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that KeyringStore can write and read a secret within the same process.
    /// This verifies the keyring backend is functional (not the in-memory fallback).
    #[test]
    fn keyring_roundtrip() {
        let store = KeyringStore::new();
        let test_id = "pgtool_test_roundtrip";
        let test_pw = "test_password_roundtrip_123!@#";

        // Clean up any existing entry
        let _ = store.delete(test_id);

        // Write
        store.set(test_id, test_pw).expect("set_password failed");

        // Read back
        let retrieved = store.get(test_id).expect("get_password failed");
        assert_eq!(retrieved, Some(test_pw.to_string()), "Secret mismatch");

        // Delete
        store.delete(test_id).expect("delete_credential failed");

        // Verify deleted
        let after_delete = store.get(test_id).expect("get_password failed");
        assert_eq!(after_delete, None, "Secret should be deleted");
    }

    /// Test that MockSecretStore works as expected.
    #[test]
    fn mock_secret_store_roundtrip() {
        let store = MockSecretStore::new();
        let test_id = "mock_test";
        let test_pw = "mock_password";

        assert_eq!(store.get(test_id).unwrap(), None);

        store.set(test_id, test_pw).unwrap();
        assert_eq!(store.get(test_id).unwrap(), Some(test_pw.to_string()));

        store.delete(test_id).unwrap();
        assert_eq!(store.get(test_id).unwrap(), None);
    }
}

// ---------------------------------------------------------------------------
// VaultStore: ONE keychain item for all passwords + in-memory cache
// ---------------------------------------------------------------------------
//
// macOS asks for permission once per keychain *item* per app identity. Storing
// one item per profile therefore prompts for every connection. VaultStore keeps
// every password in a single item (JSON map) that is read at most once per
// launch, so there is at most one prompt per launch (none once "Always Allow"
// sticks, which needs a stably signed binary).

use std::collections::HashMap;
use std::sync::Mutex;

const VAULT_SERVICE: &str = "pgTool";
const VAULT_ACCOUNT: &str = "vault";

/// Where the serialized vault blob lives.
pub trait BlobStore: Send + Sync {
    fn read(&self) -> Result<Option<String>>;
    fn write(&self, blob: &str) -> Result<()>;
}

/// The single OS keychain item holding the vault.
pub struct KeychainBlob;

impl KeychainBlob {
    fn entry() -> Result<keyring::Entry> {
        keyring::Entry::new(VAULT_SERVICE, VAULT_ACCOUNT).context("open vault keyring entry")
    }
}

impl BlobStore for KeychainBlob {
    fn read(&self) -> Result<Option<String>> {
        match Self::entry()?.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow::Error::new(e).context("vault read failed")),
        }
    }

    fn write(&self, blob: &str) -> Result<()> {
        Self::entry()?
            .set_password(blob)
            .context("vault write failed")
    }
}

pub struct VaultStore {
    blob: Box<dyn BlobStore>,
    /// Old one-item-per-profile store, read only to migrate entries.
    legacy: Option<Box<dyn SecretStore>>,
    cache: Mutex<Option<HashMap<String, String>>>,
}

impl VaultStore {
    pub fn new() -> Self {
        Self::with_parts(Box::new(KeychainBlob), Some(Box::new(KeyringStore::new())))
    }

    pub fn with_parts(blob: Box<dyn BlobStore>, legacy: Option<Box<dyn SecretStore>>) -> Self {
        Self {
            blob,
            legacy,
            cache: Mutex::new(None),
        }
    }

    fn load<'a>(
        &self,
        slot: &'a mut Option<HashMap<String, String>>,
    ) -> Result<&'a mut HashMap<String, String>> {
        if slot.is_none() {
            let map = match self.blob.read()? {
                // A corrupt vault is an error, never silently replaced by an empty one.
                Some(s) => serde_json::from_str(&s).context("vault is corrupt")?,
                None => HashMap::new(),
            };
            *slot = Some(map);
        }
        Ok(slot.as_mut().expect("just filled"))
    }

    fn persist(&self, map: &HashMap<String, String>) -> Result<()> {
        self.blob.write(&serde_json::to_string(map)?)
    }
}

impl Default for VaultStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for VaultStore {
    fn get(&self, profile_id: &str) -> Result<Option<String>> {
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let map = self.load(&mut guard)?;
        if let Some(pw) = map.get(profile_id) {
            return Ok(Some(pw.clone()));
        }
        // One-time migration of an old per-profile item into the vault.
        if let Some(legacy) = &self.legacy
            && let Some(pw) = legacy.get(profile_id)?
        {
            map.insert(profile_id.to_string(), pw.clone());
            self.persist(map)?;
            let _ = legacy.delete(profile_id);
            return Ok(Some(pw));
        }
        Ok(None)
    }

    fn set(&self, profile_id: &str, password: &str) -> Result<()> {
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let map = self.load(&mut guard)?;
        map.insert(profile_id.to_string(), password.to_string());
        self.persist(map)
    }

    fn delete(&self, profile_id: &str) -> Result<()> {
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let map = self.load(&mut guard)?;
        if map.remove(profile_id).is_some() {
            self.persist(map)?;
        }
        if let Some(legacy) = &self.legacy {
            let _ = legacy.delete(profile_id);
        }
        Ok(())
    }
}

#[cfg(test)]
mod vault_tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Default, Clone)]
    struct MemBlob {
        data: Arc<Mutex<Option<String>>>,
        reads: Arc<Mutex<u32>>,
    }

    impl BlobStore for MemBlob {
        fn read(&self) -> Result<Option<String>> {
            *self.reads.lock().unwrap() += 1;
            Ok(self.data.lock().unwrap().clone())
        }
        fn write(&self, blob: &str) -> Result<()> {
            *self.data.lock().unwrap() = Some(blob.to_string());
            Ok(())
        }
    }

    #[test]
    fn roundtrip_persists_across_instances_and_reads_blob_once() {
        let blob = MemBlob::default();
        let a = VaultStore::with_parts(Box::new(blob.clone()), None);
        a.set("p1", "pw!@#:/1").unwrap();
        a.set("p2", "pw2").unwrap();
        for _ in 0..5 {
            assert_eq!(a.get("p1").unwrap().as_deref(), Some("pw!@#:/1"));
        }
        assert_eq!(*blob.reads.lock().unwrap(), 1, "keychain read once per launch");

        // A new instance (= next app launch) sees the same data.
        let b = VaultStore::with_parts(Box::new(blob.clone()), None);
        assert_eq!(b.get("p2").unwrap().as_deref(), Some("pw2"));
        b.delete("p2").unwrap();
        assert_eq!(b.get("p2").unwrap(), None);
    }

    #[test]
    fn migrates_legacy_item_once() {
        let legacy = MockSecretStore::new();
        legacy.set("old", "legacy_pw").unwrap();
        let blob = MemBlob::default();
        let v = VaultStore::with_parts(Box::new(blob.clone()), Some(Box::new(legacy)));
        assert_eq!(v.get("old").unwrap().as_deref(), Some("legacy_pw"));
        let v2 = VaultStore::with_parts(Box::new(blob), None);
        assert_eq!(v2.get("old").unwrap().as_deref(), Some("legacy_pw"));
    }

    #[test]
    fn corrupt_vault_is_an_error_not_a_wipe() {
        let blob = MemBlob::default();
        *blob.data.lock().unwrap() = Some("{not json".into());
        let v = VaultStore::with_parts(Box::new(blob.clone()), None);
        assert!(v.get("x").is_err());
        assert!(v.set("x", "y").is_err());
        assert_eq!(blob.data.lock().unwrap().as_deref(), Some("{not json"));
    }
}
