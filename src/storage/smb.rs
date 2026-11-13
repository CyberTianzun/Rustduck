use crate::models::{join_remote_path, normalize_remote_path, FileEntry, FileKind, SmbConfig};
use crate::storage::{ProgressCallback, StorageBackend};
use anyhow::{anyhow, Context};
use futures::StreamExt;
use smb::{
    Client, ClientConfig, CreateDisposition, CreateOptions, Directory, FileAccessMask,
    FileAttributes, FileCreateArgs, FileDirectoryInformation, FileDispositionInformation,
    FileRenameInformation, Resource, UncPath,
};
use std::str::FromStr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

pub fn build_smb_unc_path(
    server: &str,
    share: &str,
    remote_path: &str,
) -> anyhow::Result<String> {
    let clean_server = server.trim().trim_matches(|c| c == '/' || c == '\\');
    if clean_server.is_empty() {
        return Err(anyhow!("SMB server hostname/IP cannot be empty"));
    }
    let clean_share = share.trim().trim_matches(|c| c == '/' || c == '\\');
    if clean_share.is_empty() {
        return Err(anyhow!("SMB share name cannot be empty"));
    }

    let norm_sub = normalize_remote_path(remote_path);
    if norm_sub == "/" {
        Ok(format!(r"\\{clean_server}\{clean_share}"))
    } else {
        let win_sub = norm_sub.trim_start_matches('/').replace('/', r"\");
        Ok(format!(r"\\{clean_server}\{clean_share}\{win_sub}"))
    }
}

pub fn format_smb_auth_username(domain: Option<&str>, username: &str) -> String {
    let clean_user = username.trim();
    let user = if clean_user.is_empty() {
        "guest"
    } else {
        clean_user
    };
    if let Some(dom) = domain.map(str::trim).filter(|d| !d.is_empty()) {
        if !user.contains('\\') && !user.contains('@') {
            return format!(r"{dom}\{user}");
        }
    }
    user.to_string()
}

pub struct SmbBackend {
    config: SmbConfig,
    password: String,
    client: Client,
}

impl SmbBackend {
    pub fn new(config: SmbConfig, password: String) -> anyhow::Result<Self> {
        let _ = build_smb_unc_path(&config.server, &config.share, "/")?;
        let client = Client::new(ClientConfig::default());
        Ok(Self {
            config,
            password,
            client,
        })
    }

    async fn ensure_share_connected(&self) -> anyhow::Result<UncPath> {
        let root_unc_str = build_smb_unc_path(&self.config.server, &self.config.share, "/")?;
        let unc = UncPath::from_str(&root_unc_str)
            .map_err(|e| anyhow!("Invalid SMB UNC path {root_unc_str}: {e}"))?;
        let user = format_smb_auth_username(self.config.domain.as_deref(), &self.config.username);
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.client
                .share_connect(&unc, &user, self.password.clone()),
        )
        .await
        .with_context(|| format!("Timed out connecting to SMB share {root_unc_str} (15s)"))?
        .with_context(|| format!("Failed to connect to SMB share {root_unc_str}"))?;
        Ok(unc)
    }

    fn resolve_unc(&self, remote_path: &str) -> anyhow::Result<UncPath> {
        let unc_str = build_smb_unc_path(&self.config.server, &self.config.share, remote_path)?;
        UncPath::from_str(&unc_str).map_err(|e| anyhow!("Invalid SMB UNC path {unc_str}: {e}"))
    }

    async fn remove_single_smb_item(&self, path: &str, is_dir: bool) -> anyhow::Result<()> {
        let unc = self.resolve_unc(path)?;
        let args = FileCreateArgs {
            disposition: CreateDisposition::Open,
            attributes: if is_dir {
                FileAttributes::new().with_directory(true)
            } else {
                FileAttributes::new()
            },
            options: if is_dir {
                CreateOptions::new().with_directory_file(true)
            } else {
                CreateOptions::new()
            },
            desired_access: FileAccessMask::new()
                .with_delete(true)
                .with_synchronize(true),
        };
        let resource = self
            .client
            .create_file(&unc, &args)
            .await
            .with_context(|| format!("Failed to open SMB path for removal: {unc}"))?;
        let disp = FileDispositionInformation::default();
        match resource {
            Resource::File(f) => {
                f.set_info(disp).await?;
                let _ = f.close().await;
            }
            Resource::Directory(d) => {
                d.set_info(disp).await?;
                let _ = d.close().await;
            }
            Resource::Pipe(_) => return Err(anyhow!("Cannot remove SMB pipe")),
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl StorageBackend for SmbBackend {
    fn protocol_name(&self) -> &'static str {
        "SMB"
    }

    async fn connect_and_verify(&self) -> anyhow::Result<String> {
        let _ = self.ensure_share_connected().await?;
        let initial = normalize_remote_path(&self.config.initial_path);
        Ok(initial)
    }

    async fn list_dir(&self, path: &str) -> anyhow::Result<Vec<FileEntry>> {
        let _ = self.ensure_share_connected().await?;
        let norm_path = normalize_remote_path(path);
        let unc = self.resolve_unc(&norm_path)?;

        let open_args = FileCreateArgs {
            disposition: CreateDisposition::Open,
            attributes: FileAttributes::new().with_directory(true),
            options: CreateOptions::new().with_directory_file(true),
            desired_access: FileAccessMask::new().with_generic_read(true),
        };

        let resource = self
            .client
            .create_file(&unc, &open_args)
            .await
            .with_context(|| format!("Failed to open SMB directory {unc}"))?;

        let dir = match resource {
            Resource::Directory(d) => Arc::new(d),
            _ => return Err(anyhow!("SMB target is not a directory: {unc}")),
        };

        let mut entries = Vec::new();
        {
            let mut stream = Directory::query::<FileDirectoryInformation>(&dir, "*")
                .await
                .with_context(|| format!("Failed to query SMB directory {unc}"))?;

            while let Some(item_res) = stream.next().await {
                let item = item_res?;
                let name = item.file_name.to_string();
                if name == "." || name == ".." || name.is_empty() {
                    continue;
                }
                let is_dir = item.file_attributes.directory();
                entries.push(FileEntry {
                    name: name.clone(),
                    path: join_remote_path(&norm_path, &name),
                    kind: if is_dir {
                        FileKind::Directory
                    } else {
                        FileKind::File
                    },
                    size: if is_dir { 0 } else { item.end_of_file },
                    modified_unix: None,
                    permissions: None,
                });
            }
        }
        let _ = dir.close().await;
        Ok(entries)
    }

    async fn create_dir(&self, path: &str) -> anyhow::Result<()> {
        let _ = self.ensure_share_connected().await?;
        let unc = self.resolve_unc(path)?;
        let args = FileCreateArgs {
            disposition: CreateDisposition::Create,
            attributes: FileAttributes::new().with_directory(true),
            options: CreateOptions::new().with_directory_file(true),
            desired_access: FileAccessMask::new().with_generic_all(true),
        };
        let resource = self
            .client
            .create_file(&unc, &args)
            .await
            .with_context(|| format!("Failed to create SMB directory {unc}"))?;
        if let Resource::Directory(d) = resource {
            let _ = d.close().await;
        }
        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> anyhow::Result<()> {
        let _ = self.ensure_share_connected().await?;
        let src_unc = self.resolve_unc(from)?;
        let args = FileCreateArgs::make_open_existing(
            FileAccessMask::new()
                .with_delete(true)
                .with_generic_write(true)
                .with_synchronize(true),
        );
        let resource = self
            .client
            .create_file(&src_unc, &args)
            .await
            .with_context(|| format!("Failed to open SMB path for rename: {src_unc}"))?;

        let new_rel = normalize_remote_path(to)
            .trim_start_matches('/')
            .replace('/', r"\");
        let rename_info = FileRenameInformation {
            replace_if_exists: true.into(),
            root_directory: 0,
            file_name: new_rel.as_str().into(),
        };

        match resource {
            Resource::File(f) => {
                f.set_info(rename_info).await?;
                let _ = f.close().await;
            }
            Resource::Directory(d) => {
                d.set_info(rename_info).await?;
                let _ = d.close().await;
            }
            Resource::Pipe(_) => return Err(anyhow!("Cannot rename SMB pipe")),
        }
        Ok(())
    }

    async fn remove(&self, path: &str, is_dir: bool) -> anyhow::Result<()> {
        let _ = self.ensure_share_connected().await?;
        if is_dir {
            let mut stack = vec![(normalize_remote_path(path), false)];
            while let Some((cur_dir, children_listed)) = stack.pop() {
                if !children_listed {
                    stack.push((cur_dir.clone(), true));
                    let children = self.list_dir(&cur_dir).await?;
                    for child in children {
                        if child.kind.is_dir_like() {
                            stack.push((child.path, false));
                        } else {
                            self.remove_single_smb_item(&child.path, false).await?;
                        }
                    }
                } else {
                    self.remove_single_smb_item(&cur_dir, true).await?;
                }
            }
            Ok(())
        } else {
            self.remove_single_smb_item(path, false).await
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
        let _ = self.ensure_share_connected().await?;
        let unc = self.resolve_unc(remote_path)?;
        let args = FileCreateArgs::make_open_existing(
            FileAccessMask::new()
                .with_generic_read(true)
                .with_synchronize(true),
        );
        let resource = self
            .client
            .create_file(&unc, &args)
            .await
            .with_context(|| format!("Failed to open SMB file for download: {unc}"))?;
        let file = match resource {
            Resource::File(f) => f,
            _ => return Err(anyhow!("SMB path is not a file: {unc}")),
        };

        let mut buf = vec![0u8; 64 * 1024];
        let mut pos = 0u64;
        loop {
            if cancel.is_cancelled() {
                let _ = file.close().await;
                return Err(anyhow!("Transfer cancelled"));
            }
            let n = file.read_block(&mut buf, pos, None, false).await?;
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n]).await?;
            pos += n as u64;
            progress(pos);
        }
        let _ = file.close().await;
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
        let _ = self.ensure_share_connected().await?;
        let unc = self.resolve_unc(remote_path)?;
        let args = FileCreateArgs::make_overwrite(FileAttributes::new(), CreateOptions::new());
        let resource = self
            .client
            .create_file(&unc, &args)
            .await
            .with_context(|| format!("Failed to create SMB file for upload: {unc}"))?;
        let file = match resource {
            Resource::File(f) => f,
            _ => return Err(anyhow!("SMB target is not a file: {unc}")),
        };

        let mut buf = vec![0u8; 64 * 1024];
        let mut pos = 0u64;
        loop {
            if cancel.is_cancelled() {
                let _ = file.close().await;
                return Err(anyhow!("Transfer cancelled"));
            }
            let n = reader.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            file.write_block(&buf[..n], pos, None).await?;
            pos += n as u64;
            progress(pos);
        }
        file.flush().await?;
        let _ = file.close().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_smb_unc_path_handles_slashes_cleanly() {
        let unc = build_smb_unc_path("192.168.1.50", "/team_share/", "/docs/report.pdf").unwrap();
        assert_eq!(unc, r"\\192.168.1.50\team_share\docs\report.pdf");

        let root_unc = build_smb_unc_path("nas.local", "public", "/").unwrap();
        assert_eq!(root_unc, r"\\nas.local\public");
    }

    #[test]
    fn test_build_smb_unc_rejects_empty_server_or_share() {
        assert!(build_smb_unc_path("", "public", "/").is_err());
        assert!(build_smb_unc_path("192.168.1.50", " / ", "/").is_err());
    }

    #[test]
    fn test_format_smb_auth_username_prepends_domain() {
        assert_eq!(
            format_smb_auth_username(Some("CORP"), "alice"),
            r"CORP\alice"
        );
        assert_eq!(format_smb_auth_username(Some("  "), "alice"), "alice");
        assert_eq!(format_smb_auth_username(None, ""), "guest");
        assert_eq!(
            format_smb_auth_username(Some("CORP"), r"OTHER\bob"),
            r"OTHER\bob"
        );
    }
}
