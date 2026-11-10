use crate::models::{Bookmark, ProtocolConfig};
use anyhow::{anyhow, Context};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedBookmarkEntry {
    #[serde(flatten)]
    pub bookmark: Bookmark,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkExportBundle {
    pub version: u32,
    pub app: String,
    pub exported_at_unix: i64,
    pub bookmarks: Vec<ExportedBookmarkEntry>,
}

pub trait CredentialVault: Send + Sync {
    fn set_secret(&self, bookmark_id: Uuid, secret: &str) -> anyhow::Result<()>;
    fn get_secret(&self, bookmark_id: Uuid) -> anyhow::Result<Option<String>>;
    fn delete_secret(&self, bookmark_id: Uuid) -> anyhow::Result<()>;
}

#[derive(Default)]
pub struct MemoryCredentialVault {
    secrets: RwLock<HashMap<Uuid, String>>,
}

impl CredentialVault for MemoryCredentialVault {
    fn set_secret(&self, bookmark_id: Uuid, secret: &str) -> anyhow::Result<()> {
        self.secrets
            .write()
            .expect("vault lock poisoned")
            .insert(bookmark_id, secret.to_string());
        Ok(())
    }

    fn get_secret(&self, bookmark_id: Uuid) -> anyhow::Result<Option<String>> {
        Ok(self
            .secrets
            .read()
            .expect("vault lock poisoned")
            .get(&bookmark_id)
            .cloned())
    }

    fn delete_secret(&self, bookmark_id: Uuid) -> anyhow::Result<()> {
        self.secrets
            .write()
            .expect("vault lock poisoned")
            .remove(&bookmark_id);
        Ok(())
    }
}

#[derive(Default)]
pub struct KeyringCredentialVault {
    fallback: MemoryCredentialVault,
}

impl KeyringCredentialVault {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CredentialVault for KeyringCredentialVault {
    fn set_secret(&self, bookmark_id: Uuid, secret: &str) -> anyhow::Result<()> {
        let key = bookmark_id.to_string();
        if let Ok(entry) = keyring::Entry::new("rustduck", &key) {
            if entry.set_password(secret).is_ok() {
                let _ = self.fallback.set_secret(bookmark_id, secret);
                return Ok(());
            }
        }
        self.fallback.set_secret(bookmark_id, secret)
    }

    fn get_secret(&self, bookmark_id: Uuid) -> anyhow::Result<Option<String>> {
        let key = bookmark_id.to_string();
        if let Ok(entry) = keyring::Entry::new("rustduck", &key) {
            if let Ok(pw) = entry.get_password() {
                return Ok(Some(pw));
            }
        }
        self.fallback.get_secret(bookmark_id)
    }

    fn delete_secret(&self, bookmark_id: Uuid) -> anyhow::Result<()> {
        let key = bookmark_id.to_string();
        if let Ok(entry) = keyring::Entry::new("rustduck", &key) {
            let _ = entry.delete_credential();
        }
        self.fallback.delete_secret(bookmark_id)
    }
}

pub struct BookmarkStore {
    path: PathBuf,
    bookmarks: Vec<Bookmark>,
    vault: Arc<dyn CredentialVault>,
}

impl BookmarkStore {
    pub fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rustduck")
            .join("bookmarks.json")
    }

    pub fn load_or_default(
        path: PathBuf,
        vault: Arc<dyn CredentialVault>,
    ) -> anyhow::Result<Self> {
        let bookmarks = if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read bookmarks file: {}", path.display()))?;
            serde_json::from_str::<Vec<Bookmark>>(&content).unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(Self {
            path,
            bookmarks,
            vault,
        })
    }

    pub fn list(&self) -> &[Bookmark] {
        &self.bookmarks
    }

    pub fn filter(&self, query: &str) -> Vec<Bookmark> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.bookmarks.clone();
        }
        self.bookmarks
            .iter()
            .filter(|b| {
                if b.name.to_lowercase().contains(&q)
                    || b.config.summary().to_lowercase().contains(&q)
                {
                    return true;
                }
                match &b.config {
                    ProtocolConfig::Sftp(c) => {
                        c.host.to_lowercase().contains(&q)
                            || c.username.to_lowercase().contains(&q)
                    }
                    ProtocolConfig::Ftp(c) => {
                        c.host.to_lowercase().contains(&q)
                            || c.username.to_lowercase().contains(&q)
                    }
                    ProtocolConfig::S3(c) => {
                        c.endpoint.to_lowercase().contains(&q)
                            || c.region.to_lowercase().contains(&q)
                            || c.bucket
                                .as_deref()
                                .unwrap_or_default()
                                .to_lowercase()
                                .contains(&q)
                    }
                    ProtocolConfig::Smb(c) => {
                        c.server.to_lowercase().contains(&q)
                            || c.share.to_lowercase().contains(&q)
                    }
                    ProtocolConfig::WebDav(c) => {
                        c.endpoint.to_lowercase().contains(&q)
                            || c.username.to_lowercase().contains(&q)
                    }
                    ProtocolConfig::GoogleDrive(c) => {
                        c.client_id.to_lowercase().contains(&q)
                            || c.proxy_url
                                .as_deref()
                                .unwrap_or_default()
                                .to_lowercase()
                                .contains(&q)
                    }
                    ProtocolConfig::OneDrive(c) => {
                        c.client_id.to_lowercase().contains(&q)
                            || c.proxy_url
                                .as_deref()
                                .unwrap_or_default()
                                .to_lowercase()
                                .contains(&q)
                    }
                }
            })
            .cloned()
            .collect()
    }

    pub fn upsert(&mut self, bookmark: Bookmark, secret: Option<&str>) -> anyhow::Result<()> {
        let id = bookmark.id;
        if let Some(existing) = self.bookmarks.iter_mut().find(|b| b.id == id) {
            *existing = bookmark;
        } else {
            self.bookmarks.push(bookmark);
        }
        if let Some(sec) = secret {
            if !sec.is_empty() {
                self.vault.set_secret(id, sec)?;
            }
        }
        self.save()
    }

    pub fn get_secret(&self, id: Uuid) -> anyhow::Result<Option<String>> {
        self.vault.get_secret(id)
    }

    pub fn set_secret(&self, id: Uuid, secret: &str) -> anyhow::Result<()> {
        self.vault.set_secret(id, secret)
    }

    pub fn remove(&mut self, id: Uuid) -> anyhow::Result<()> {
        self.bookmarks.retain(|b| b.id != id);
        let _ = self.vault.delete_secret(id);
        self.save()
    }

    pub fn export_to_json(&self) -> anyhow::Result<(String, usize)> {
        let entries: Vec<ExportedBookmarkEntry> = self
            .bookmarks
            .iter()
            .map(|bm| {
                let secret = self
                    .vault
                    .get_secret(bm.id)
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty());
                ExportedBookmarkEntry {
                    bookmark: bm.clone(),
                    secret,
                }
            })
            .collect();
        let count = entries.len();
        let bundle = BookmarkExportBundle {
            version: 1,
            app: "rustduck".to_string(),
            exported_at_unix: Utc::now().timestamp(),
            bookmarks: entries,
        };
        let json = serde_json::to_string_pretty(&bundle)?;
        Ok((json, count))
    }

    pub fn export_to_path(&self, path: &Path) -> anyhow::Result<usize> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let (json, count) = self.export_to_json()?;
        std::fs::write(path, json)
            .with_context(|| format!("Failed to write export file: {}", path.display()))?;
        Ok(count)
    }

    pub fn import_from_json(&mut self, json: &str) -> anyhow::Result<usize> {
        let entries: Vec<ExportedBookmarkEntry> =
            if let Ok(bundle) = serde_json::from_str::<BookmarkExportBundle>(json) {
                bundle.bookmarks
            } else if let Ok(list) = serde_json::from_str::<Vec<ExportedBookmarkEntry>>(json) {
                list
            } else {
                return Err(anyhow!("Invalid Rustduck bookmarks export JSON format"));
            };

        let mut imported = 0usize;
        for entry in entries {
            let id = entry.bookmark.id;
            if let Some(existing) = self.bookmarks.iter_mut().find(|b| b.id == id) {
                *existing = entry.bookmark;
            } else {
                self.bookmarks.push(entry.bookmark);
            }
            if let Some(sec) = entry.secret.as_deref().filter(|s| !s.is_empty()) {
                let _ = self.vault.set_secret(id, sec);
            }
            imported += 1;
        }
        self.save()?;
        Ok(imported)
    }

    pub fn import_from_path(&mut self, path: &Path) -> anyhow::Result<usize> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read import file: {}", path.display()))?;
        self.import_from_json(&content)
    }

    fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.bookmarks)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Bookmark, ProtocolConfig, S3AddressingStyle, S3Config, S3Preset};
    use uuid::Uuid;

    #[test]
    fn test_bookmark_persistence_never_writes_secret_to_json_file() {
        let dir = tempfile::tempdir().unwrap();
        let json_path = dir.path().join("bookmarks.json");
        let vault = Arc::new(MemoryCredentialVault::default());
        let mut store = BookmarkStore::load_or_default(json_path.clone(), vault.clone()).unwrap();

        let id = Uuid::new_v4();
        let bm = Bookmark {
            id,
            name: "阿里云生产桶".to_string(),
            config: ProtocolConfig::S3(S3Config {
                preset: S3Preset::AlibabaOss,
                endpoint: "https://oss-cn-hangzhou.aliyuncs.com".to_string(),
                region: "cn-hangzhou".to_string(),
                access_key_id: "LTAI_TEST_KEY".to_string(),
                bucket: Some("prod-bucket".to_string()),
                initial_prefix: "/".to_string(),
                addressing_style: S3AddressingStyle::VirtualHosted,
            }),
        };

        store.upsert(bm, Some("SUPER_SECRET_SK_12345")).unwrap();

        let raw_json = std::fs::read_to_string(&json_path).unwrap();
        assert!(!raw_json.contains("SUPER_SECRET_SK_12345"));
        assert_eq!(
            vault.get_secret(id).unwrap().as_deref(),
            Some("SUPER_SECRET_SK_12345")
        );

        let reloaded = BookmarkStore::load_or_default(json_path, vault).unwrap();
        assert_eq!(reloaded.filter("hangzhou").len(), 1);
        assert_eq!(reloaded.filter("prod-bucket").len(), 1);
    }

    #[test]
    fn test_keyring_credential_vault_persists_across_separate_instances() {
        let id = Uuid::new_v4();
        let secret_val = "COS_TEST_SECRET_PERSISTENCE_CHECK";

        {
            let vault_writer = KeyringCredentialVault::new();
            vault_writer.set_secret(id, secret_val).unwrap();
        }

        let vault_reader = KeyringCredentialVault::new();
        let loaded = vault_reader.get_secret(id).unwrap();
        let _ = vault_reader.delete_secret(id);

        assert_eq!(
            loaded.as_deref(),
            Some(secret_val),
            "KeyringCredentialVault must persist secrets in OS credential store across instances"
        );
        assert_eq!(vault_reader.get_secret(id).unwrap(), None);
    }

    #[test]
    fn test_bookmark_export_and_import_transfers_configs_and_secrets_across_machines() {
        let dir_a = tempfile::tempdir().unwrap();
        let vault_a = Arc::new(MemoryCredentialVault::default());
        let mut store_a =
            BookmarkStore::load_or_default(dir_a.path().join("bookmarks.json"), vault_a.clone())
                .unwrap();

        let id1 = Uuid::new_v4();
        let bm1 = Bookmark {
            id: id1,
            name: "Prod S3".to_string(),
            config: ProtocolConfig::S3(S3Config::default()),
        };
        store_a.upsert(bm1.clone(), Some("S3_SECRET_KEY_ABC")).unwrap();

        let export_file = dir_a.path().join("exported_connections.json");
        let exported_count = store_a.export_to_path(&export_file).unwrap();
        assert_eq!(exported_count, 1);

        let dir_b = tempfile::tempdir().unwrap();
        let vault_b = Arc::new(MemoryCredentialVault::default());
        let mut store_b =
            BookmarkStore::load_or_default(dir_b.path().join("bookmarks.json"), vault_b.clone())
                .unwrap();
        assert!(store_b.list().is_empty());

        let imported_count = store_b.import_from_path(&export_file).unwrap();
        assert_eq!(imported_count, 1);
        assert_eq!(store_b.list().len(), 1);
        assert_eq!(store_b.list()[0], bm1);
        assert_eq!(
            store_b.get_secret(id1).unwrap().as_deref(),
            Some("S3_SECRET_KEY_ABC")
        );
    }
}
