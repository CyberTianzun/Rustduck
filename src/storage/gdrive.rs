use crate::i18n::i18n;
use crate::models::{
    join_remote_path, normalize_remote_path, parent_remote_path, FileEntry, FileKind,
    GoogleDriveConfig,
};
use crate::storage::oauth::{
    authorize_via_browser_loopback, build_http_client, refresh_access_token,
    resolve_oauth_credential,
};
use crate::storage::{ProgressCallback, StorageBackend};
use anyhow::{anyhow, Context};
use chrono::DateTime;
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::RwLock;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub const GDRIVE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GDRIVE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GDRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";
const GDRIVE_FOLDER_MIME: &str = "application/vnd.google-apps.folder";

#[derive(Debug, Deserialize)]
struct GDriveFileListResponse {
    #[serde(default)]
    files: Vec<GDriveFileItem>,
}

#[derive(Debug, Deserialize)]
struct GDriveFileItem {
    id: String,
    name: String,
    #[serde(rename = "mimeType", default)]
    mime_type: String,
    #[serde(default)]
    size: Option<String>,
    #[serde(rename = "modifiedTime", default)]
    modified_time: Option<String>,
}

pub fn effective_gdrive_client_id(config: &GoogleDriveConfig) -> Option<String> {
    resolve_oauth_credential(
        Some(&config.client_id),
        "RUSTDUCK_GDRIVE_CLIENT_ID",
        option_env!("RUSTDUCK_GDRIVE_CLIENT_ID"),
    )
}

pub fn effective_gdrive_client_secret(config: &GoogleDriveConfig) -> Option<String> {
    resolve_oauth_credential(
        config.client_secret.as_deref(),
        "RUSTDUCK_GDRIVE_CLIENT_SECRET",
        option_env!("RUSTDUCK_GDRIVE_CLIENT_SECRET"),
    )
}

pub fn validate_gdrive_oauth_config(config: &GoogleDriveConfig) -> anyhow::Result<(String, String)> {
    let t = i18n();
    let client_id = effective_gdrive_client_id(config)
        .ok_or_else(|| anyhow!("{}", t.err_gdrive_client_id_missing()))?;
    let client_secret = effective_gdrive_client_secret(config)
        .ok_or_else(|| anyhow!("{}", t.err_gdrive_client_secret_missing()))?;
    Ok((client_id, client_secret))
}

pub fn has_default_gdrive_oauth_credentials() -> bool {
    validate_gdrive_oauth_config(&GoogleDriveConfig::default()).is_ok()
}

pub fn escape_gdrive_query_value(val: &str) -> String {
    val.replace('\\', "\\\\").replace('\'', "\\'")
}

pub fn parse_gdrive_files_json(
    json: &str,
    parent_path: &str,
) -> anyhow::Result<Vec<(String, FileEntry)>> {
    let parsed: GDriveFileListResponse = serde_json::from_str(json)
        .with_context(|| format!("Failed to parse Google Drive files list JSON: {json}"))?;

    let norm_parent = normalize_remote_path(parent_path);
    let mut out = Vec::with_capacity(parsed.files.len());
    for item in parsed.files {
        let is_dir = item.mime_type == GDRIVE_FOLDER_MIME;
        let size = if is_dir {
            0
        } else {
            item.size
                .as_deref()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        };
        let modified_unix = item
            .modified_time
            .as_deref()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|dt| dt.timestamp());

        let entry_path = join_remote_path(&norm_parent, &item.name);
        out.push((
            item.id,
            FileEntry {
                name: item.name,
                path: entry_path,
                kind: if is_dir {
                    FileKind::Directory
                } else {
                    FileKind::File
                },
                size,
                modified_unix,
                permissions: None,
            },
        ));
    }
    Ok(out)
}

pub async fn authorize_google_drive_in_browser(
    config: &GoogleDriveConfig,
    cancel: CancellationToken,
) -> anyhow::Result<String> {
    let (client_id, client_secret) = validate_gdrive_oauth_config(config)?;
    let client = build_http_client(config.proxy_url.as_deref())?;
    let token_res = authorize_via_browser_loopback(
        &client,
        GDRIVE_AUTH_URL,
        GDRIVE_TOKEN_URL,
        &client_id,
        Some(client_secret.as_str()),
        GDRIVE_SCOPE,
        &[("access_type", "offline"), ("prompt", "consent")],
        cancel,
    )
    .await?;

    Ok(token_res
        .refresh_token
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(token_res.access_token))
}

struct GDriveTokenState {
    access_token: Option<String>,
    refresh_token: String,
}

pub struct GoogleDriveBackend {
    config: GoogleDriveConfig,
    client: Client,
    token_state: Mutex<GDriveTokenState>,
    latest_refresh_token: RwLock<Option<String>>,
    path_id_cache: RwLock<HashMap<String, String>>,
}

impl GoogleDriveBackend {
    pub fn new(config: GoogleDriveConfig, secret: String) -> anyhow::Result<Self> {
        let initial_rt = secret.trim().to_string();
        if initial_rt.is_empty() {
            return Err(anyhow!("{}", i18n().err_oauth_token_empty("Google Drive")));
        }
        let client = build_http_client(config.proxy_url.as_deref())?;
        let mut cache = HashMap::new();
        cache.insert("/".to_string(), "root".to_string());
        Ok(Self {
            config,
            client,
            token_state: Mutex::new(GDriveTokenState {
                access_token: None,
                refresh_token: initial_rt.clone(),
            }),
            latest_refresh_token: RwLock::new(Some(initial_rt)),
            path_id_cache: RwLock::new(cache),
        })
    }

    async fn ensure_access_token(&self) -> anyhow::Result<String> {
        let mut guard = self.token_state.lock().await;
        if let Some(tok) = &guard.access_token {
            return Ok(tok.clone());
        }

        if guard.refresh_token.trim().is_empty() {
            return Err(anyhow!("{}", i18n().err_oauth_token_empty("Google Drive")));
        }

        if let Some(client_id) = effective_gdrive_client_id(&self.config) {
            let client_secret = effective_gdrive_client_secret(&self.config);
            if let Ok(refreshed) = refresh_access_token(
                &self.client,
                GDRIVE_TOKEN_URL,
                &client_id,
                client_secret.as_deref(),
                &guard.refresh_token,
            )
            .await
            {
                let access = refreshed.access_token;
                if let Some(new_rt) = refreshed.refresh_token.filter(|s| !s.trim().is_empty()) {
                    guard.refresh_token = new_rt.clone();
                    if let Ok(mut w) = self.latest_refresh_token.write() {
                        *w = Some(new_rt);
                    }
                }
                guard.access_token = Some(access.clone());
                return Ok(access);
            }
        }

        // Fallback: user may have pasted a direct access_token in secret field
        let direct = guard.refresh_token.trim().to_string();
        guard.access_token = Some(direct.clone());
        Ok(direct)
    }

    async fn resolve_file_id(&self, path: &str) -> anyhow::Result<String> {
        let norm = normalize_remote_path(path);
        if norm == "/" {
            return Ok("root".to_string());
        }
        if let Ok(cache) = self.path_id_cache.read() {
            if let Some(id) = cache.get(&norm) {
                return Ok(id.clone());
            }
        }

        let token = self.ensure_access_token().await?;
        let mut current_id = "root".to_string();
        let mut built_path = "/".to_string();

        for seg in norm.trim_start_matches('/').split('/').filter(|s| !s.is_empty()) {
            built_path = join_remote_path(&built_path, seg);
            if let Ok(cache) = self.path_id_cache.read() {
                if let Some(cached_id) = cache.get(&built_path) {
                    current_id = cached_id.clone();
                    continue;
                }
            }

            let escaped_name = escape_gdrive_query_value(seg);
            let q = format!(
                "'{current_id}' in parents and name = '{escaped_name}' and trashed = false"
            );
            let resp = self
                .client
                .get("https://www.googleapis.com/drive/v3/files")
                .bearer_auth(&token)
                .query(&[
                    ("q", q.as_str()),
                    ("fields", "files(id,name,mimeType)"),
                    ("pageSize", "10"),
                    ("supportsAllDrives", "true"),
                    ("includeItemsFromAllDrives", "true"),
                ])
                .send()
                .await
                .with_context(|| format!("Failed to resolve Google Drive path {built_path}"))?;

            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(anyhow!(
                    "Google Drive API error resolving {built_path} (HTTP {}): {}",
                    status.as_u16(),
                    text
                ));
            }

            let parsed: GDriveFileListResponse = serde_json::from_str(&text)?;
            let found = parsed
                .files
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("Google Drive path not found: {built_path}"))?;
            current_id = found.id;
            if let Ok(mut cache) = self.path_id_cache.write() {
                cache.insert(built_path.clone(), current_id.clone());
            }
        }

        Ok(current_id)
    }
}

#[async_trait::async_trait]
impl StorageBackend for GoogleDriveBackend {
    fn protocol_name(&self) -> &'static str {
        "Google Drive"
    }

    fn exported_secret(&self) -> Option<String> {
        self.latest_refresh_token
            .read()
            .ok()
            .and_then(|g| g.clone())
    }

    async fn connect_and_verify(&self) -> anyhow::Result<String> {
        let token = self.ensure_access_token().await?;
        let resp = self
            .client
            .get("https://www.googleapis.com/drive/v3/about")
            .bearer_auth(&token)
            .query(&[("fields", "user(displayName,emailAddress)")])
            .send()
            .await
            .context("Failed to connect to Google Drive API (check network or proxy settings)")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Google Drive verification failed (HTTP {}): {}",
                status.as_u16(),
                body
            ));
        }

        let initial = normalize_remote_path(&self.config.initial_path);
        let _ = self.resolve_file_id(&initial).await?;
        Ok(initial)
    }

    async fn list_dir(&self, path: &str) -> anyhow::Result<Vec<FileEntry>> {
        let token = self.ensure_access_token().await?;
        let norm_path = normalize_remote_path(path);
        let folder_id = self.resolve_file_id(&norm_path).await?;

        let q = format!("'{folder_id}' in parents and trashed = false");
        let resp = self
            .client
            .get("https://www.googleapis.com/drive/v3/files")
            .bearer_auth(&token)
            .query(&[
                ("q", q.as_str()),
                ("fields", "files(id,name,mimeType,size,modifiedTime)"),
                ("pageSize", "1000"),
                ("supportsAllDrives", "true"),
                ("includeItemsFromAllDrives", "true"),
            ])
            .send()
            .await
            .with_context(|| format!("Google Drive list_dir failed for {norm_path}"))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "Google Drive list_dir error (HTTP {}): {}",
                status.as_u16(),
                text
            ));
        }

        let pairs = parse_gdrive_files_json(&text, &norm_path)?;
        let mut entries = Vec::with_capacity(pairs.len());
        if let Ok(mut cache) = self.path_id_cache.write() {
            for (id, entry) in pairs {
                cache.insert(entry.path.clone(), id);
                entries.push(entry);
            }
        } else {
            for (_, entry) in pairs {
                entries.push(entry);
            }
        }
        Ok(entries)
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
        let parent_id = self.resolve_file_id(&parent_path).await?;

        let body = serde_json::json!({
            "name": folder_name,
            "mimeType": GDRIVE_FOLDER_MIME,
            "parents": [parent_id],
        });

        let resp = self
            .client
            .post("https://www.googleapis.com/drive/v3/files")
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Google Drive create_dir failed for {norm}"))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "Google Drive create_dir failed (HTTP {}): {}",
                status.as_u16(),
                text
            ));
        }
        if let Ok(item) = serde_json::from_str::<GDriveFileItem>(&text) {
            if let Ok(mut cache) = self.path_id_cache.write() {
                cache.insert(norm, item.id);
            }
        }
        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> anyhow::Result<()> {
        let token = self.ensure_access_token().await?;
        let src = normalize_remote_path(from);
        let dst = normalize_remote_path(to);
        let file_id = self.resolve_file_id(&src).await?;
        let new_name = dst
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("Invalid target name in {dst}"))?;

        let body = serde_json::json!({ "name": new_name });
        let url = format!("https://www.googleapis.com/drive/v3/files/{file_id}");
        let resp = self
            .client
            .patch(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Google Drive rename failed from {src} to {dst}"))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Google Drive rename failed (HTTP {}): {}",
                status.as_u16(),
                text
            ));
        }
        if let Ok(mut cache) = self.path_id_cache.write() {
            if let Some(id) = cache.remove(&src) {
                cache.insert(dst, id);
            }
        }
        Ok(())
    }

    async fn remove(&self, path: &str, _is_dir: bool) -> anyhow::Result<()> {
        let token = self.ensure_access_token().await?;
        let norm = normalize_remote_path(path);
        let file_id = self.resolve_file_id(&norm).await?;
        let url = format!("https://www.googleapis.com/drive/v3/files/{file_id}");
        let resp = self
            .client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .with_context(|| format!("Google Drive delete failed for {norm}"))?;

        let status = resp.status();
        if !status.is_success() && status.as_u16() != 404 {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Google Drive delete failed (HTTP {}): {}",
                status.as_u16(),
                text
            ));
        }
        if let Ok(mut cache) = self.path_id_cache.write() {
            cache.remove(&norm);
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
        let file_id = self.resolve_file_id(&norm).await?;
        let url = format!("https://www.googleapis.com/drive/v3/files/{file_id}?alt=media");

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .with_context(|| format!("Google Drive download failed for {norm}"))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Google Drive download failed (HTTP {}): {}",
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
        let parent_path = parent_remote_path(&norm).unwrap_or_else(|| "/".to_string());
        let file_name = norm
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("Invalid file name in {norm}"))?;
        let parent_id = self.resolve_file_id(&parent_path).await?;

        // Step 1: Initiate resumable upload session
        let meta = serde_json::json!({
            "name": file_name,
            "parents": [parent_id],
        });
        let init_resp = self
            .client
            .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable")
            .bearer_auth(&token)
            .json(&meta)
            .send()
            .await
            .with_context(|| format!("Google Drive resumable upload init failed for {norm}"))?;

        let status = init_resp.status();
        if !status.is_success() {
            let text = init_resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Google Drive upload init failed (HTTP {}): {}",
                status.as_u16(),
                text
            ));
        }

        let upload_url = init_resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Google Drive resumable upload missing Location header"))?;

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

        let put_resp = self
            .client
            .put(&upload_url)
            .bearer_auth(&token)
            .body(payload)
            .send()
            .await
            .with_context(|| format!("Google Drive upload PUT failed for {norm}"))?;

        let put_status = put_resp.status();
        if !put_status.is_success() {
            let text = put_resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Google Drive upload failed (HTTP {}): {}",
                put_status.as_u16(),
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
    fn test_parse_gdrive_files_json_and_credential_resolution() {
        let mut cfg = GoogleDriveConfig::default();
        if std::env::var("RUSTDUCK_GDRIVE_CLIENT_ID")
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            assert_eq!(effective_gdrive_client_id(&cfg), None);
        }
        cfg.client_id = "  my-desktop-cid.apps.googleusercontent.com  ".to_string();
        cfg.client_secret = Some("  GOCSPX-secret  ".to_string());
        assert_eq!(
            effective_gdrive_client_id(&cfg).as_deref(),
            Some("my-desktop-cid.apps.googleusercontent.com")
        );
        assert_eq!(
            effective_gdrive_client_secret(&cfg).as_deref(),
            Some("GOCSPX-secret")
        );

        let sample_json = r#"{
            "files": [
                {
                    "id": "folder_123",
                    "name": "Designs",
                    "mimeType": "application/vnd.google-apps.folder",
                    "modifiedTime": "2026-11-10T08:00:00Z"
                },
                {
                    "id": "file_456",
                    "name": "architecture.pdf",
                    "mimeType": "application/pdf",
                    "size": "1048576",
                    "modifiedTime": "2026-11-10T09:30:00Z"
                }
            ]
        }"#;

        let parsed = parse_gdrive_files_json(sample_json, "/Projects").unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "folder_123");
        assert_eq!(parsed[0].1.name, "Designs");
        assert_eq!(parsed[0].1.path, "/Projects/Designs");
        assert_eq!(parsed[0].1.kind, FileKind::Directory);
        assert_eq!(parsed[1].0, "file_456");
        assert_eq!(parsed[1].1.name, "architecture.pdf");
        assert_eq!(parsed[1].1.size, 1048576);
        assert_eq!(parsed[1].1.kind, FileKind::File);
    }

    #[tokio::test]
    async fn test_gdrive_oauth_and_backend_validation_rejects_missing_credentials_without_hanging() {
        if std::env::var("RUSTDUCK_GDRIVE_CLIENT_ID")
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            let empty_cfg = GoogleDriveConfig::default();
            let err = authorize_google_drive_in_browser(&empty_cfg, CancellationToken::new())
                .await
                .unwrap_err();
            assert!(
                err.to_string().contains("Client ID"),
                "Expected missing Client ID validation error, got: {err}"
            );
        }

        if std::env::var("RUSTDUCK_GDRIVE_CLIENT_SECRET")
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            let missing_secret_cfg = GoogleDriveConfig {
                client_id: "123456-abc.apps.googleusercontent.com".to_string(),
                client_secret: None,
                proxy_url: None,
                initial_path: "/".to_string(),
            };
            let err =
                authorize_google_drive_in_browser(&missing_secret_cfg, CancellationToken::new())
                    .await
                    .unwrap_err();
            assert!(
                err.to_string().contains("Client Secret"),
                "Expected missing Client Secret validation error, got: {err}"
            );
        }

        // Empty Refresh Token must fail fast on GoogleDriveBackend::new instead of silently launching browser on Connect
        let empty_token_res = GoogleDriveBackend::new(GoogleDriveConfig::default(), "   ".to_string());
        assert!(empty_token_res.is_err());
    }
}
