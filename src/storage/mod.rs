pub mod ftp;
pub mod gdrive;
pub mod local;
pub mod oauth;
pub mod onedrive;
pub mod s3;
pub mod sftp;
pub mod smb;
pub mod webdav;

use crate::models::{FileEntry, ProtocolConfig};
use crate::storage::ftp::FtpBackend;
use crate::storage::gdrive::GoogleDriveBackend;
use crate::storage::onedrive::OneDriveBackend;
use crate::storage::s3::S3Backend;
use crate::storage::sftp::SftpBackend;
use crate::storage::smb::SmbBackend;
use crate::storage::webdav::WebDavBackend;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

pub type ProgressCallback = Arc<dyn Fn(u64) + Send + Sync>;

#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    fn protocol_name(&self) -> &'static str;

    fn exported_secret(&self) -> Option<String> {
        None
    }

    async fn connect_and_verify(&self) -> anyhow::Result<String>;

    async fn list_dir(&self, path: &str) -> anyhow::Result<Vec<FileEntry>>;

    async fn create_dir(&self, path: &str) -> anyhow::Result<()>;

    async fn rename(&self, from: &str, to: &str) -> anyhow::Result<()>;

    async fn remove(&self, path: &str, is_dir: bool) -> anyhow::Result<()>;

    async fn download_stream(
        &self,
        remote_path: &str,
        writer: &mut (dyn AsyncWrite + Unpin + Send),
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> anyhow::Result<()>;

    async fn upload_stream(
        &self,
        reader: &mut (dyn AsyncRead + Unpin + Send),
        total_size: Option<u64>,
        remote_path: &str,
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> anyhow::Result<()>;
}

pub fn create_backend(
    config: &ProtocolConfig,
    secret: &str,
) -> anyhow::Result<Arc<dyn StorageBackend>> {
    match config {
        ProtocolConfig::Sftp(c) => Ok(Arc::new(SftpBackend::new(c.clone(), secret.to_string())?)),
        ProtocolConfig::Ftp(c) => Ok(Arc::new(FtpBackend::new(c.clone(), secret.to_string())?)),
        ProtocolConfig::S3(c) => Ok(Arc::new(S3Backend::new(c.clone(), secret.to_string())?)),
        ProtocolConfig::Smb(c) => Ok(Arc::new(SmbBackend::new(c.clone(), secret.to_string())?)),
        ProtocolConfig::WebDav(c) => {
            Ok(Arc::new(WebDavBackend::new(c.clone(), secret.to_string())?))
        }
        ProtocolConfig::GoogleDrive(c) => Ok(Arc::new(GoogleDriveBackend::new(
            c.clone(),
            secret.to_string(),
        )?)),
        ProtocolConfig::OneDrive(c) => Ok(Arc::new(OneDriveBackend::new(
            c.clone(),
            secret.to_string(),
        )?)),
    }
}
