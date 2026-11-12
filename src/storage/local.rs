use crate::models::{FileEntry, FileKind};
use crate::storage::{ProgressCallback, StorageBackend};
use anyhow::{anyhow, Context};
use std::path::PathBuf;
use std::time::UNIX_EPOCH;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Default, Clone)]
pub struct LocalFileSystem;

impl LocalFileSystem {
    pub fn new() -> Self {
        Self
    }

    pub fn initial_local_dir() -> String {
        dirs::home_dir()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" }))
            .to_string_lossy()
            .to_string()
    }
}

#[async_trait::async_trait]
impl StorageBackend for LocalFileSystem {
    fn protocol_name(&self) -> &'static str {
        "Local"
    }

    async fn connect_and_verify(&self) -> anyhow::Result<String> {
        Ok(Self::initial_local_dir())
    }

    async fn list_dir(&self, path: &str) -> anyhow::Result<Vec<FileEntry>> {
        let trimmed = path.trim();
        #[cfg(target_os = "windows")]
        if trimmed.is_empty() || trimmed == "/" || trimmed == "\\" {
            let mut drives = Vec::new();
            for letter in b'A'..=b'Z' {
                let drive = format!("{}:\\", letter as char);
                if std::path::Path::new(&drive).exists() {
                    drives.push(FileEntry {
                        name: drive.clone(),
                        path: drive,
                        kind: FileKind::Directory,
                        size: 0,
                        modified_unix: None,
                        permissions: None,
                    });
                }
            }
            return Ok(drives);
        }

        let target = if trimmed.is_empty() { "/" } else { trimmed };
        let mut dir = tokio::fs::read_dir(target)
            .await
            .with_context(|| format!("Failed to read local directory: {target}"))?;

        let mut entries = Vec::new();
        while let Some(entry) = dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            let entry_path = entry.path().to_string_lossy().to_string();
            let meta = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => continue,
            };
            let kind = if meta.is_dir() {
                FileKind::Directory
            } else {
                FileKind::File
            };
            let modified_unix = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            let permissions = Some(if meta.permissions().readonly() {
                "r--r--r--".to_string()
            } else {
                "rw-rw-rw-".to_string()
            });

            entries.push(FileEntry {
                name,
                path: entry_path,
                kind,
                size: if meta.is_dir() { 0 } else { meta.len() },
                modified_unix,
                permissions,
            });
        }
        Ok(entries)
    }

    async fn create_dir(&self, path: &str) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(path)
            .await
            .with_context(|| format!("Failed to create directory: {path}"))
    }

    async fn rename(&self, from: &str, to: &str) -> anyhow::Result<()> {
        tokio::fs::rename(from, to)
            .await
            .with_context(|| format!("Failed to rename {from} to {to}"))
    }

    async fn remove(&self, path: &str, is_dir: bool) -> anyhow::Result<()> {
        if is_dir {
            tokio::fs::remove_dir_all(path)
                .await
                .with_context(|| format!("Failed to remove directory: {path}"))
        } else {
            tokio::fs::remove_file(path)
                .await
                .with_context(|| format!("Failed to remove file: {path}"))
        }
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
        let mut file = tokio::fs::File::open(remote_path)
            .await
            .with_context(|| format!("Failed to open local file for reading: {remote_path}"))?;

        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(anyhow!("Transfer cancelled"));
            }
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n]).await?;
            total += n as u64;
            progress(total);
        }
        writer.flush().await?;
        Ok(())
    }

    async fn upload_stream(
        &self,
        reader: &mut (dyn AsyncRead + Unpin + Send),
        _total_size: Option<u64>,
        remote_path: &str,
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        if cancel.is_cancelled() {
            return Err(anyhow!("Transfer cancelled"));
        }
        if let Some(parent) = std::path::Path::new(remote_path).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        let mut file = tokio::fs::File::create(remote_path)
            .await
            .with_context(|| format!("Failed to create local file: {remote_path}"))?;

        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(anyhow!("Transfer cancelled"));
            }
            let n = reader.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).await?;
            total += n as u64;
            progress(total);
        }
        file.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn test_local_fs_crud_and_streaming_with_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().to_string();
        let fs = LocalFileSystem::new();

        let sub = format!("{}/ sub_folder", root);
        fs.create_dir(&sub).await.unwrap();
        let entries = fs.list_dir(&root).await.unwrap();
        assert_eq!(entries.len(), 1);

        let file_path = format!("{}/payload.bin", sub);
        let data = vec![42u8; 128 * 1024];
        let reported = Arc::new(AtomicU64::new(0));
        let reported_clone = reported.clone();
        let mut reader = std::io::Cursor::new(data.clone());

        fs.upload_stream(
            &mut reader,
            Some(data.len() as u64),
            &file_path,
            Arc::new(move |bytes| {
                reported_clone.store(bytes, Ordering::SeqCst);
            }),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(reported.load(Ordering::SeqCst), data.len() as u64);

        // Verify mid-stream cancellation aborts promptly
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut sink = Vec::new();
        let err = fs
            .download_stream(&file_path, &mut sink, Arc::new(|_| {}), cancel)
            .await;
        assert!(err.is_err());
    }
}
