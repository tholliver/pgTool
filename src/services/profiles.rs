//! Profile repository: load/save/migrate connections.json.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use super::secrets::SecretStore;
use crate::connections::{ConnectionProfile, SslMode};

#[derive(Debug, Serialize, Deserialize)]
struct ProfilesFileV2 {
    version: u32,
    profiles: Vec<ConnectionProfile>,
}

#[derive(Debug, Deserialize)]
struct LegacyProfile {
    id: String,
    name: String,
    host: String,
    port: u16,
    #[serde(default)]
    database: Option<String>,
    username: String,
    ssl_mode: SslMode,
    #[serde(default)]
    password: String,
}

/// Profile repository that owns a SecretStore and a config directory.
pub struct ProfileRepo {
    store: Arc<dyn SecretStore>,
    path: PathBuf,
    backup_path: PathBuf,
}

impl ProfileRepo {
    pub fn new(store: Arc<dyn SecretStore>) -> Self {
        let path = Self::connections_path();
        let backup_path = path.with_extension("json.bak");
        Self {
            store,
            path,
            backup_path,
        }
    }

    fn connections_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("pgTool")
            .join("connections.json")
    }

    /// Load profiles, migrating v1 (plaintext array) to v2 (object with UUID IDs).
    /// Returns (profiles, warnings) where warnings are non-fatal migration issues.
    pub fn load(&self) -> Result<(Vec<ConnectionProfile>, Vec<String>)> {
        if !self.path.exists() {
            return Ok((Vec::new(), Vec::new()));
        }

        let data = std::fs::read_to_string(&self.path)
            .with_context(|| format!("read {}", self.path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&data)
            .with_context(|| format!("parse {}", self.path.display()))?;

        // Detect v1 (bare array) vs v2 (object with version).
        if let Some(arr) = value.as_array() {
            // v1 migration
            self.migrate_v1(arr)
        } else if let Some(obj) = value.as_object() {
            if obj.get("version").and_then(|v| v.as_u64()) == Some(2) {
                // v2
                let file: ProfilesFileV2 = serde_json::from_value(value)
                    .with_context(|| format!("parse v2 {}", self.path.display()))?;
                Ok((file.profiles, Vec::new()))
            } else {
                // Unknown version - treat as v2 but warn
                let file: ProfilesFileV2 = serde_json::from_value(value)
                    .with_context(|| format!("parse unknown {}", self.path.display()))?;
                Ok((
                    file.profiles,
                    vec!["Unknown connections.json version; assuming v2".into()],
                ))
            }
        } else {
            Err(anyhow::anyhow!(
                "connections.json is not an array or object"
            ))
        }
    }

    /// Migrate v1 bare array of profiles (with passwords) to v2.
    /// For each profile: generate UUID, store password in keyring, drop password from JSON.
    /// If any keyring write fails, abort and keep v1 file (return error).
    fn migrate_v1(
        &self,
        arr: &[serde_json::Value],
    ) -> Result<(Vec<ConnectionProfile>, Vec<String>)> {
        let mut profiles = Vec::with_capacity(arr.len());
        let mut warnings = Vec::new();

        // First pass: try to write all secrets. If any fails, we don't modify the file.
        let mut secret_ops = Vec::new();
        for v in arr {
            let legacy: LegacyProfile =
                serde_json::from_value(v.clone()).context("parse legacy profile")?;
            if !legacy.password.is_empty() {
                let new_id = Uuid::new_v4().to_string();
                let pw = legacy.password.clone();
                secret_ops.push((new_id, pw, legacy));
            } else {
                // No password: just generate new UUID, keep other fields
                let new_id = Uuid::new_v4().to_string();
                let profile = ConnectionProfile {
                    id: new_id,
                    name: legacy.name,
                    host: legacy.host,
                    port: legacy.port,
                    database: legacy.database,
                    username: legacy.username,
                    ssl_mode: legacy.ssl_mode,
                    ssl_ca_path: None,
                    initial_database: None,
                };
                profiles.push(profile);
            }
        }

        // Write all secrets. If any fails, return error and don't touch the file.
        for (id, pw, _) in &secret_ops {
            self.store
                .set(id, pw)
                .with_context(|| format!("store secret for profile {}", id))?;
        }

        // All secrets stored successfully. Now build final profiles.
        for (id, _, legacy) in secret_ops {
            let profile = ConnectionProfile {
                id,
                name: legacy.name,
                host: legacy.host,
                port: legacy.port,
                database: legacy.database,
                username: legacy.username,
                ssl_mode: legacy.ssl_mode,
                ssl_ca_path: None,
                initial_database: None,
            };
            profiles.push(profile);
        }

        // Backup v1 file and write v2 atomically.
        if self.path.exists() {
            std::fs::copy(&self.path, &self.backup_path).with_context(|| {
                format!(
                    "backup {} to {}",
                    self.path.display(),
                    self.backup_path.display()
                )
            })?;
        }
        self.save(&profiles)?;

        warnings.push(
            "Migrated connections.json from v1 to v2 (passwords moved to system keyring)".into(),
        );
        Ok((profiles, warnings))
    }

    /// Save profiles atomically. Never writes passwords.
    pub fn save(&self, profiles: &[ConnectionProfile]) -> Result<()> {
        let dir = self.path.parent().unwrap();
        std::fs::create_dir_all(dir).with_context(|| format!("create dir {}", dir.display()))?;

        let file = ProfilesFileV2 {
            version: 2,
            profiles: profiles.to_vec(),
        };
        let json = serde_json::to_string_pretty(&file).context("serialize profiles")?;

        let tmp_path = self.path.with_extension("json.tmp");
        std::fs::write(&tmp_path, json).with_context(|| format!("write {}", tmp_path.display()))?;
        std::fs::rename(&tmp_path, &self.path)
            .with_context(|| format!("rename {} -> {}", tmp_path.display(), self.path.display()))?;
        Ok(())
    }

    /// Delete a profile and its secret.
    pub fn delete(&self, profile_id: &str) -> Result<()> {
        self.store.delete(profile_id)?;
        // Note: actual profile removal from the list is done by the caller.
        Ok(())
    }
}
