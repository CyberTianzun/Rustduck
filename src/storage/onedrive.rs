use crate::models::{
    join_remote_path, normalize_remote_path, parent_remote_path, FileEntry, FileKind,
    OneDriveConfig, DEFAULT_ONEDRIVE_CLIENT_ID,
};
use crate::storage::oauth::{
    authorize_via_browser_loopback, build_http_client, refresh_access_token,
};
use crate::storage::s3::encode_s3_key_path;
use crate::storage::{ProgressCallback, StorageBackend};
use anyhow::{anyhow, Context};
use chrono::DateTime;
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use std::sync::RwLock;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub const ONEDRIVE_AUTH_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
pub const ONEDRIVE_TOKEN_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/token";
pub const ONEDRIVE_SCOPE: &str = "Files.ReadWrite.All offline_access User.Read";

#[derive(Debug, Deserialize)]
struct OneDriveChildrenResponse {
    #[serde(default)]
    value: Vec<OneDriveItem>,
}

#[derive(Debug, Deserialize)]
struct OneDriveItem {
    name: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(rename = "lastModifiedDateTime", default)]
    last_modified: Option<String>,
    #[serde(default)]
    folder: Option<serde_json::Value>,
}

pub fn effective_onedrive_client_id(config: &OneDriveConfig) -> &str {
    let cid = config.client_id.trim();
    if cid.is_empty() {
        DEFAULT_ONEDRIVE_CLIENT_ID
    } else {
        cid
    }
}

pub fn build_onedrive_item_url(remote_path: &str, suffix: &str) -> String {
    let norm = normalize_remote_path(remote_path);
    let clean_suffix = suffix.trim_start_matches('/');
    if norm == "/" {
        if clean_suffix.is_empty() {
            "https://graph.microsoft.com/v1.0/me/drive/root".to_string()
        } else {
            format!("https://graph.microsoft.com/v1.0/me/drive/root/{clean_suffix}")
        }
    } else {
        let encoded = encode_s3_key_path(&norm);
        if clean_suffix.is_empty() {
            format!("https://graph.microsoft.com/v1.0/me/drive/root:/{encoded}")
        } else {
            format!("https://graph.microsoft.com/v1.0/me/drive/root:/{encoded}:/{clean_suffix}")
        }
    }
}

pub fn parse_onedrive_children_json(
    json: &str,
    parent_path: &str,
) -> anyhow::Result<Vec<FileEntry>> {
    let parsed: OneDriveChildrenResponse = serde_json::from_str(json)
        .with_context(|| format!("Failed to parse OneDrive children JSON: {json}"))?;

    let norm_parent = normalize_remote_path(parent_path);
    let mut entries = Vec::with_capacity(parsed.value.len());
    for item in parsed.value {
        let is_dir = item.folder.is_some();
        let modified_unix = item
            .last_modified
            .as_deref()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|dt| dt.timestamp());

        entries.push(FileEntry {
            path: join_remote_path(&norm_parent, &item.name),
            name: item.name,
            kind: if is_dir {
                FileKind::Directory
            } else {
                FileKind::File
            },
            size: if is_dir { 0 } else { item.size.unwrap_or(0) },
            modified_unix,
            permissions: None,
        });
    }
    Ok(entries)
}

pub async fn authorize_onedrive_in_browser(config: &OneDriveConfig) -> anyhow::Result<String> {
    let client = build_http_client(config.proxy_url.as_deref())?;
    let client_id = effective_onedrive_client_id(config);
    let token_res = authorize_via_browser_loopback(
        &client,
        ONEDRIVE_AUTH_URL,
        ONEDRIVE_TOKEN_URL,
        client_id,
        None,
        ONEDRIVE_SCOPE,
        &[("prompt", "select_account")],
    )
    .await?;

    Ok(token_res
        .refresh_token
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(token_res.access_token))
}

struct OneDriveTokenState {
    access_token: Option<String>,
    refresh_token: String,
}

pub struct OneDriveBackend {
    config: OneDriveConfig,
    client: Client,
    token_state: Mutex<OneDriveTokenState>,
    latest_refresh_token: RwLock<Option<String>>,
}

impl OneDriveBackend {
    pub fn new(config: OneDriveConfig, secret: String) -> anyhow::Result<Self> {
        let client = build_http_client(config.proxy_url.as_deref())?;
        let initial_rt = secret.trim().to_string();
        Ok(Self {
            config,
            client,
            token_state: Mutex::new(OneDriveTokenState {
                access_token: None,
                refresh_token: initial_rt.clone(),
            }),
            latest_refresh_token: RwLock::new(
                (!initial_rt.is_empty()).then_some(initial_rt),
            ),
        })
    }

    async fn ensure_access_token(&self) -> anyhow::Result<String> {
        let mut guard = self.token_state.lock().await;
        if let Some(tok) = &guard.access_token {
            return Ok(tok.clone());
        }

        let client_id = effective_onedrive_client_id(&self.config);
        if guard.refresh_token.trim().is_empty() {
            let token_res = authorize_via_browser_loopback(
                &self.client,
                ONEDRIVE_AUTH_URL,
                ONEDRIVE_TOKEN_URL,
                client_id,
                None,
                ONEDRIVE_SCOPE,
                &[("prompt", "select_account")],
            )
            .await?;

            let access = token_res.access_token.clone();
            let refresh = token_res
                .refresh_token
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| access.clone());
            guard.access_token = Some(access.clone());
            guard.refresh_token = refresh.clone();
            if let Ok(mut w) = self.latest_refresh_token.write() {
                *w = Some(refresh);
            }
            return Ok(access);
        }

        match refresh_access_token(
            &self.client,
            ONEDRIVE_TOKEN_URL,
            client_id,
            None,
            &guard.refresh_token,
        )
        .await
        {
            Ok(refreshed) => {
                let access = refreshed.access_token;
                if let Some(new_rt) = refreshed.refresh_token.filter(|s| !s.trim().is_empty()) {
                    guard.refresh_token = new_rt.clone();
                    if let Ok(mut w) = self.latest_refresh_token.write() {
                        *w = Some(new_rt);
                    }
                }
                guard.access_token = Some(access.clone());
                Ok(access)
            }
            Err(_) => {
                let direct = guard.refresh_token.trim().to_string();
                guard.access_token = Some(direct.clone());
                Ok(direct)
            }
        }
    }
}

#[async_trait::async_trait]
impl StorageBackend for OneDriveBackend {
    fn protocol_name(&self) -> &'static str {
        "OneDrive"
    }

    fn exported_secret(&self) -> Option<String> {
        self.latest_refresh_token
            .read()
            .ok()
            .and_then(|g| g.clone())
    }

    async fn connect_and_verify(&self) -> anyhow::Result<String> {
        let token = self.ensure_access_token().await?;
        let initial = normalize_remote_path(&self.config.initial_path);
        let url = build_onedrive_item_url(&initial, "");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .context("Failed to connect to Microsoft OneDrive Graph API (check network or proxy settings)")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "OneDrive verification failed (HTTP {}): {}",
                status.as_u16(),
                body
            ));
        }
        Ok(initial)
    }

    async fn list_dir(&self, path: &str) -> anyhow::Result<Vec<FileEntry>> {
        let token = self.ensure_access_token().await?;
        let norm = normalize_remote_path(path);
        let url = build_onedrive_item_url(&norm, "children");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .query(&[("$top", "999")])
            .send()
            .await
            .with_context(|| format!("OneDrive list_dir failed for {norm}"))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "OneDrive list_dir failed (HTTP {}): {}",
                status.as_u16(),
                text
            ));
        }
        parse_onedrive_children_json(&text, &norm)
    }

    async fn create_dir(&self, path: &str) -> anyhow::Result<()> {
        let token = self.ensure_access_token().await?;
        let norm = normalize_remote_path(path);
        let parent_path = parent_remote_path(&norm).unwrap_or_else(|| "/".to_string());
        let folder_name = norm
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("Invalid folder name in {norm}"))?;

        let url = build_onedrive_item_url(&parent_path, "children");
        let body = serde_json::json!({
            "name": folder_name,
            "folder": {},
            "@microsoft.graph.conflictBehavior": "replace"
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("OneDrive create_dir failed for {norm}"))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "OneDrive create_dir failed (HTTP {}): {}",
                status.as_u16(),
                text
            ));
        }
        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> anyhow::Result<()> {
        let token = self.ensure_access_token().await?;
        let src = normalize_remote_path(from);
        let dst = normalize_remote_path(to);
        let new_name = dst
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("Invalid target name in {dst}"))?;

        let url = build_onedrive_item_url(&src, "");
        let body = serde_json::json!({ "name": new_name });
        let resp = self
            .client
            .patch(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("OneDrive rename failed from {src} to {dst}"))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "OneDrive rename failed (HTTP {}): {}",
                status.as_u16(),
                text
            ));
        }
        Ok(())
    }

    async fn remove(&self, path: &str, _is_dir: bool) -> anyhow::Result<()> {
        let token = self.ensure_access_token().await?;
        let norm = normalize_remote_path(path);
        let url = build_onedrive_item_url(&norm, "");
        let resp = self
            .client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .with_context(|| format!("OneDrive delete failed for {norm}"))?;

        let status = resp.status();
        if !status.is_success() && status.as_u16() != 404 {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "OneDrive delete failed (HTTP {}): {}",
                status.as_u16(),
                text
            ));
        }
        Ok(())
    }

    async fn download_stream(
        &self,
        remote_path: &str,
        writer: &mut (dyn AsyncWrite + Unpin + Send),
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        if cancel.is_cancelled() {
            return Err(anyhow!("Transfer cancelled"));
        }
        let token = self.ensure_access_token().await?;
        let norm = normalize_remote_path(remote_path);
        let url = build_onedrive_item_url(&norm, "content");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .with_context(|| format!("OneDrive download failed for {norm}"))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "OneDrive download failed (HTTP {}): {}",
                status.as_u16(),
                text
            ));
        }

        let mut stream = resp.bytes_stream();
        let mut total = 0u64;
        while let Some(chunk_res) = stream.next().await {
            if cancel.is_cancelled() {
                return Err(anyhow!("Transfer cancelled"));
            }
            let chunk = chunk_res?;
            writer.write_all(&chunk).await?;
            total += chunk.len() as u64;
            progress(total);
        }
        writer.flush().await?;
        Ok(())
    }

    async fn upload_stream(
        &self,
        reader: &mut (dyn AsyncRead + Unpin + Send),
        total_size: Option<u64>,
        remote_path: &str,
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        if cancel.is_cancelled() {
            return Err(anyhow!("Transfer cancelled"));
        }
        let token = self.ensure_access_token().await?;
        let norm = normalize_remote_path(remote_path);
        let url = build_onedrive_item_url(&norm, "content");

        let mut payload = Vec::with_capacity(total_size.unwrap_or(0) as usize);
        let mut buf = vec![0u8; 64 * 1024];
        let mut read_total = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(anyhow!("Transfer cancelled"));
            }
            let n = reader.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            payload.extend_from_slice(&buf[..n]);
            read_total += n as u64;
            progress(read_total);
        }

        let resp = self
            .client
            .put(&url)
            .bearer_auth(&token)
            .body(payload)
            .send()
            .await
            .with_context(|| format!("OneDrive upload failed for {norm}"))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "OneDrive upload failed (HTTP {}): {}",
                status.as_u16(),
                text
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_onedrive_item_url_and_parse_children() {
        assert_eq!(
            build_onedrive_item_url("/", "children"),
            "https://graph.microsoft.com/v1.0/me/drive/root/children"
        );
        assert_eq!(
            build_onedrive_item_url("/Photos/2026 Trip", "children"),
            "https://graph.microsoft.com/v1.0/me/drive/root:/Photos/2026%20Trip:/children"
        );
        assert_eq!(
            build_onedrive_item_url("/Photos/image.png", "content"),
            "https://graph.microsoft.com/v1.0/me/drive/root:/Photos/image.png:/content"
        );

        let sample_json = r#"{
            "value": [
                {
                    "name": "Documents",
                    "folder": { "childCount": 4 },
                    "lastModifiedDateTime": "2026-11-10T10:00:00Z"
                },
                {
                    "name": "budget.xlsx",
                    "size": 65536,
                    "lastModifiedDateTime": "2026-11-10T11:15:00Z"
                }
            ]
        }"#;

        let items = parse_onedrive_children_json(sample_json, "/Work").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "Documents");
        assert_eq!(items[0].path, "/Work/Documents");
        assert_eq!(items[0].kind, FileKind::Directory);
        assert_eq!(items[1].name, "budget.xlsx");
        assert_eq!(items[1].path, "/Work/budget.xlsx");
        assert_eq!(items[1].size, 65536);
        assert_eq!(items[1].kind, FileKind::File);
    }
}
