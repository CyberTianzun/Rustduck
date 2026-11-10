use crate::models::{Bookmark, ProtocolConfig};
use anyhow::Context;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

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
                if b.name.to_lowercase().contains(&q) {
                    return true;
                }
                match &b.config {
                    ProtocolConfig::Sftp(c) => {
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

    pub fn remove(&mut self, id: Uuid) -> anyhow::Result<()> {
        self.bookmarks.retain(|b| b.id != id);
        let _ = self.vault.delete_secret(id);
        self.save()
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
        assert_eq!(vault.get_secret(id).unwrap().as_deref(), Some("SUPER_SECRET_SK_12345"));

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
}
