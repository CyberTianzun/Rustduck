pub mod local;
pub mod s3;
pub mod sftp;
pub mod smb;

use crate::models::{FileEntry, ProtocolConfig};
use crate::storage::s3::S3Backend;
use crate::storage::sftp::SftpBackend;
use crate::storage::smb::SmbBackend;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

pub type ProgressCallback = Arc<dyn Fn(u64) + Send + Sync>;

#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    fn protocol_name(&self) -> &'static str;

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
        ProtocolConfig::S3(c) => Ok(Arc::new(S3Backend::new(c.clone(), secret.to_string())?)),
        ProtocolConfig::Smb(c) => Ok(Arc::new(SmbBackend::new(c.clone(), secret.to_string())?)),
    }
}
