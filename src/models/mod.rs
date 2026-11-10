use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEFAULT_GDRIVE_CLIENT_ID: &str = "407408718192.apps.googleusercontent.com";
pub const DEFAULT_ONEDRIVE_CLIENT_ID: &str = "b15665d9-eda6-4092-8539-0eec376afd59";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolKind {
    Sftp,
    Ftp,
    S3,
    Smb,
    WebDav,
    GoogleDrive,
    OneDrive,
}

impl ProtocolKind {
    pub fn label(&self) -> &'static str {
        match self {
            ProtocolKind::Sftp => "SSH / SFTP",
            ProtocolKind::Ftp => "FTP / FTPS",
            ProtocolKind::S3 => "S3 (通用对象存储)",
            ProtocolKind::Smb => "SMB",
            ProtocolKind::WebDav => "WebDAV",
            ProtocolKind::GoogleDrive => "Google Drive",
            ProtocolKind::OneDrive => "OneDrive",
        }
    }

    pub fn short_tag(&self) -> &'static str {
        match self {
            ProtocolKind::Sftp => "SFTP",
            ProtocolKind::Ftp => "FTP",
            ProtocolKind::S3 => "S3",
            ProtocolKind::Smb => "SMB",
            ProtocolKind::WebDav => "WebDAV",
            ProtocolKind::GoogleDrive => "GDrive",
            ProtocolKind::OneDrive => "OneDrive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum S3AddressingStyle {
    #[default]
    VirtualHosted,
    PathStyle,
}

impl S3AddressingStyle {
    pub fn label(&self) -> &'static str {
        match self {
            S3AddressingStyle::VirtualHosted => "Virtual-Hosted (阿里云/腾讯云/AWS)",
            S3AddressingStyle::PathStyle => "Path-Style (MinIO/自建)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum S3Preset {
    #[default]
    AlibabaOss,
    TencentCos,
    Minio,
    AwsS3,
    Custom,
}

impl S3Preset {
    pub fn label(&self) -> &'static str {
        match self {
            S3Preset::AlibabaOss => "阿里云 OSS",
            S3Preset::TencentCos => "腾讯云 COS",
            S3Preset::Minio => "MinIO / 私有云",
            S3Preset::AwsS3 => "AWS S3",
            S3Preset::Custom => "自定义 S3 Endpoint",
        }
    }

    pub fn default_endpoint(&self, region: &str) -> String {
        let r = if region.trim().is_empty() {
            "us-east-1"
        } else {
            region.trim()
        };
        match self {
            S3Preset::AlibabaOss => format!("https://oss-{r}.aliyuncs.com"),
            S3Preset::TencentCos => format!("https://cos.{r}.myqcloud.com"),
            S3Preset::Minio => "http://127.0.0.1:9000".to_string(),
            S3Preset::AwsS3 => format!("https://s3.{r}.amazonaws.com"),
            S3Preset::Custom => format!("https://s3.{r}.example.com"),
        }
    }

    pub fn default_addressing_style(&self) -> S3AddressingStyle {
        match self {
            S3Preset::Minio => S3AddressingStyle::PathStyle,
            _ => S3AddressingStyle::VirtualHosted,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SftpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub private_key_path: Option<String>,
    pub initial_path: String,
}

impl Default for SftpConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 22,
            username: "root".to_string(),
            private_key_path: None,
            initial_path: "/".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub use_tls: bool,
    pub initial_path: String,
}

impl Default for FtpConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 21,
            username: "anonymous".to_string(),
            use_tls: false,
            initial_path: "/".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct S3Config {
    pub preset: S3Preset,
    pub endpoint: String,
    pub region: String,
    pub access_key_id: String,
    pub bucket: Option<String>,
    pub initial_prefix: String,
    pub addressing_style: S3AddressingStyle,
}

impl Default for S3Config {
    fn default() -> Self {
        let region = "cn-hangzhou".to_string();
        let preset = S3Preset::AlibabaOss;
        Self {
            preset,
            endpoint: preset.default_endpoint(&region),
            region,
            access_key_id: String::new(),
            bucket: None,
            initial_prefix: "/".to_string(),
            addressing_style: preset.default_addressing_style(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmbConfig {
    pub server: String,
    pub port: u16,
    pub share: String,
    pub username: String,
    pub domain: Option<String>,
    pub initial_path: String,
}

impl Default for SmbConfig {
    fn default() -> Self {
        Self {
            server: String::new(),
            port: 445,
            share: String::new(),
            username: String::new(),
            domain: None,
            initial_path: "/".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebDavConfig {
    pub endpoint: String,
    pub username: String,
    pub initial_path: String,
}

impl Default for WebDavConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://dav.example.com/remote.php/webdav".to_string(),
            username: String::new(),
            initial_path: "/".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoogleDriveConfig {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub proxy_url: Option<String>,
    pub initial_path: String,
}

impl Default for GoogleDriveConfig {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            client_secret: None,
            proxy_url: None,
            initial_path: "/".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OneDriveConfig {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub proxy_url: Option<String>,
    pub initial_path: String,
}

impl Default for OneDriveConfig {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            proxy_url: None,
            initial_path: "/".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "protocol", rename_all = "snake_case")]
pub enum ProtocolConfig {
    Sftp(SftpConfig),
    Ftp(FtpConfig),
    S3(S3Config),
    Smb(SmbConfig),
    WebDav(WebDavConfig),
    GoogleDrive(GoogleDriveConfig),
    OneDrive(OneDriveConfig),
}

impl ProtocolConfig {
    pub fn kind(&self) -> ProtocolKind {
        match self {
            ProtocolConfig::Sftp(_) => ProtocolKind::Sftp,
            ProtocolConfig::Ftp(_) => ProtocolKind::Ftp,
            ProtocolConfig::S3(_) => ProtocolKind::S3,
            ProtocolConfig::Smb(_) => ProtocolKind::Smb,
            ProtocolConfig::WebDav(_) => ProtocolKind::WebDav,
            ProtocolConfig::GoogleDrive(_) => ProtocolKind::GoogleDrive,
            ProtocolConfig::OneDrive(_) => ProtocolKind::OneDrive,
        }
    }

    pub fn summary(&self) -> String {
        match self {
            ProtocolConfig::Sftp(c) => format!("SFTP · {}@{}:{}", c.username, c.host, c.port),
            ProtocolConfig::Ftp(c) => {
                let tag = if c.use_tls { "FTPS" } else { "FTP" };
                format!("{tag} · {}@{}:{}", c.username, c.host, c.port)
            }
            ProtocolConfig::S3(c) => {
                let bkt = c.bucket.as_deref().unwrap_or("全部 Bucket");
                format!("S3 · {} ({})", c.endpoint, bkt)
            }
            ProtocolConfig::Smb(c) => format!("SMB · //{}/{}", c.server, c.share),
            ProtocolConfig::WebDav(c) => {
                let usr = if c.username.trim().is_empty() {
                    "anonymous"
                } else {
                    c.username.trim()
                };
                format!("WebDAV · {} ({usr})", c.endpoint)
            }
            ProtocolConfig::GoogleDrive(c) => {
                let proxy = c
                    .proxy_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|p| format!(" [Proxy: {p}]"))
                    .unwrap_or_default();
                format!("Google Drive · {}{proxy}", normalize_remote_path(&c.initial_path))
            }
            ProtocolConfig::OneDrive(c) => {
                let proxy = c
                    .proxy_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|p| format!(" [Proxy: {p}]"))
                    .unwrap_or_default();
                format!("OneDrive · {}{proxy}", normalize_remote_path(&c.initial_path))
            }
        }
    }

    pub fn initial_path(&self) -> String {
        match self {
            ProtocolConfig::Sftp(c) => normalize_remote_path(&c.initial_path),
            ProtocolConfig::Ftp(c) => normalize_remote_path(&c.initial_path),
            ProtocolConfig::S3(c) => normalize_remote_path(&c.initial_prefix),
            ProtocolConfig::Smb(c) => normalize_remote_path(&c.initial_path),
            ProtocolConfig::WebDav(c) => normalize_remote_path(&c.initial_path),
            ProtocolConfig::GoogleDrive(c) => normalize_remote_path(&c.initial_path),
            ProtocolConfig::OneDrive(c) => normalize_remote_path(&c.initial_path),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: Uuid,
    pub name: String,
    pub config: ProtocolConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileKind {
    Directory,
    File,
    Bucket,
}

impl FileKind {
    pub fn is_dir_like(&self) -> bool {
        matches!(self, FileKind::Directory | FileKind::Bucket)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub kind: FileKind,
    pub size: u64,
    pub modified_unix: Option<i64>,
    pub permissions: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    Upload,
    Download,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferStatus {
    Queued,
    InProgress,
    Paused,
    Completed,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferTask {
    pub id: Uuid,
    pub file_name: String,
    pub source_path: String,
    pub dest_path: String,
    pub direction: TransferDirection,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub status: TransferStatus,
}

impl TransferTask {
    pub fn progress_percent(&self) -> u8 {
        if self.total_bytes == 0 {
            match self.status {
                TransferStatus::Completed => 100,
                _ => 0,
            }
        } else {
            ((self.transferred_bytes.saturating_mul(100)) / self.total_bytes).min(100) as u8
        }
    }
}

pub fn normalize_remote_path(path: &str) -> String {
    let normalized = path.trim().replace('\\', "/");
    let segments: Vec<&str> = normalized
        .split('/')
        .filter(|seg| !seg.is_empty() && *seg != ".")
        .collect();
    if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    }
}

pub fn join_remote_path(base: &str, name: &str) -> String {
    let base_norm = normalize_remote_path(base);
    let clean_name = name.trim_matches(|c| c == '/' || c == '\\');
    if clean_name.is_empty() {
        base_norm
    } else if base_norm == "/" {
        format!("/{clean_name}")
    } else {
        format!("{base_norm}/{clean_name}")
    }
}

pub fn parent_remote_path(path: &str) -> Option<String> {
    let norm = normalize_remote_path(path);
    if norm == "/" {
        return None;
    }
    match norm.rfind('/') {
        Some(0) => Some("/".to_string()),
        Some(idx) => Some(norm[..idx].to_string()),
        None => None,
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_and_join_remote_paths() {
        assert_eq!(normalize_remote_path(""), "/");
        assert_eq!(normalize_remote_path("///var//www/"), "/var/www");
        assert_eq!(join_remote_path("/", "releases"), "/releases");
        assert_eq!(join_remote_path("/var/www", "index.html"), "/var/www/index.html");
        assert_eq!(
            parent_remote_path("/var/www/index.html"),
            Some("/var/www".to_string())
        );
        assert_eq!(parent_remote_path("/var"), Some("/".to_string()));
        assert_eq!(parent_remote_path("/"), None);
    }

    #[test]
    fn test_s3_preset_default_endpoints() {
        let oss = S3Preset::AlibabaOss.default_endpoint("cn-hangzhou");
        assert_eq!(oss, "https://oss-cn-hangzhou.aliyuncs.com");
        let cos = S3Preset::TencentCos.default_endpoint("ap-shanghai");
        assert_eq!(cos, "https://cos.ap-shanghai.myqcloud.com");
    }

    #[test]
    fn test_extended_protocols_config_summary_and_serialization() {
        let ftp = ProtocolConfig::Ftp(FtpConfig {
            host: "ftp.example.com".to_string(),
            port: 21,
            username: "deploy".to_string(),
            use_tls: true,
            initial_path: "/pub/releases".to_string(),
        });
        assert_eq!(ftp.kind(), ProtocolKind::Ftp);
        assert_eq!(ftp.summary(), "FTPS · deploy@ftp.example.com:21");
        assert_eq!(ftp.initial_path(), "/pub/releases");

        let webdav = ProtocolConfig::WebDav(WebDavConfig {
            endpoint: "https://dav.example.com/remote.php/webdav".to_string(),
            username: "alice".to_string(),
            initial_path: "/docs".to_string(),
        });
        assert_eq!(webdav.kind(), ProtocolKind::WebDav);
        assert!(webdav.summary().contains("WebDAV"));
        assert_eq!(webdav.initial_path(), "/docs");

        let gdrive = ProtocolConfig::GoogleDrive(GoogleDriveConfig {
            client_id: "my-client-id".to_string(),
            client_secret: Some("my-secret".to_string()),
            proxy_url: Some("http://127.0.0.1:7890".to_string()),
            initial_path: "/Projects".to_string(),
        });
        assert_eq!(gdrive.kind(), ProtocolKind::GoogleDrive);
        assert!(gdrive.summary().contains("Google Drive"));
        assert!(gdrive.summary().contains("127.0.0.1:7890"));
        assert_eq!(gdrive.initial_path(), "/Projects");

        let onedrive = ProtocolConfig::OneDrive(OneDriveConfig {
            client_id: "od-client-id".to_string(),
            proxy_url: Some("socks5://127.0.0.1:1080".to_string()),
            initial_path: "/Backups".to_string(),
        });
        assert_eq!(onedrive.kind(), ProtocolKind::OneDrive);
        assert!(onedrive.summary().contains("OneDrive"));
        assert!(onedrive.summary().contains("127.0.0.1:1080"));
        assert_eq!(onedrive.initial_path(), "/Backups");

        let json = serde_json::to_string(&gdrive).unwrap();
        let roundtrip: ProtocolConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip, gdrive);
    }
}
