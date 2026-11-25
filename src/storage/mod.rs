pub mod ftp;
pub mod gdrive;
pub mod local;
pub mod oauth;
pub mod onedrive;
pub mod proxy;
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

pub const S3_PRESIGN_15_MIN: u64 = 900;
pub const S3_PRESIGN_1_HOUR: u64 = 3_600;
pub const S3_PRESIGN_24_HOURS: u64 = 86_400;
pub const S3_PRESIGN_7_DAYS: u64 = 604_800;
pub const S3_PRESIGN_MAX_SECS: u64 = 604_800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteUrlKind {
    Default,
    S3Http,
    S3Uri,
    S3Presigned { expires_secs: u64 },
    SftpScp,
    SftpUrl,
    SmbUrl,
    SmbUnc,
}

const USERINFO_ENCODE_SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Format a host or IP literal for URL authority placement, bracketing bare IPv6 literals.
pub fn format_host_for_url(host: &str) -> String {
    let trimmed = host.trim();
    if trimmed.contains(':') && !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
        format!("[{trimmed}]")
    } else {
        trimmed.to_string()
    }
}

/// Percent-encode a URL userinfo component using the RFC 3986 unreserved character set.
pub fn encode_userinfo_component(raw: &str) -> String {
    percent_encoding::utf8_percent_encode(raw, USERINFO_ENCODE_SET).to_string()
}

#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    fn protocol_name(&self) -> &'static str;

    fn exported_secret(&self) -> Option<String> {
        None
    }

    fn build_item_url(
        &self,
        _entry: &FileEntry,
        _kind: RemoteUrlKind,
    ) -> anyhow::Result<Option<String>> {
        Ok(None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_host_and_userinfo_formatting_handles_ipv6_and_special_chars() {
        assert_eq!(format_host_for_url("example.com"), "example.com");
        assert_eq!(format_host_for_url("  example.com  "), "example.com");
        assert_eq!(format_host_for_url("2001:db8::1"), "[2001:db8::1]");
        assert_eq!(format_host_for_url("[2001:db8::1]"), "[2001:db8::1]");

        assert_eq!(encode_userinfo_component("alice@corp"), "alice%40corp");
        assert_eq!(encode_userinfo_component("john doe"), "john%20doe");
        assert_eq!(encode_userinfo_component("a-b.c_d~1"), "a-b.c_d~1");

        assert_eq!(S3_PRESIGN_15_MIN, 900);
        assert_eq!(S3_PRESIGN_1_HOUR, 3_600);
        assert_eq!(S3_PRESIGN_24_HOURS, 86_400);
        assert_eq!(S3_PRESIGN_7_DAYS, 604_800);
        assert_eq!(S3_PRESIGN_MAX_SECS, 604_800);
    }
}
