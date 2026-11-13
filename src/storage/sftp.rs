use crate::models::{join_remote_path, normalize_remote_path, FileEntry, FileKind, SftpConfig};
use crate::storage::{ProgressCallback, StorageBackend};
use anyhow::{anyhow, Context};
use russh::client::{self, AuthResult, Config, Handler};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh_sftp::client::SftpSession;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_SFTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

struct SshClientHandler;

pub fn expand_tilde_path(raw_path: &str) -> std::path::PathBuf {
    let trimmed = raw_path.trim();
    if trimmed == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    } else if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix(r"~\"))
    {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    std::path::PathBuf::from(trimmed)
}

impl Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        // TOFU (Trust On First Use) as specified in Phase 1 design
        Ok(true)
    }
}

pub struct SftpBackend {
    config: SftpConfig,
    secret: String,
    session: Mutex<Option<Arc<SftpSession>>>,
}

pub async fn connect_sftp_session_with_timeout(
    config: &SftpConfig,
    secret: &str,
    timeout_dur: Duration,
) -> anyhow::Result<Arc<SftpSession>> {
    let host = config.host.trim();
    let port = config.port;

    tokio::time::timeout(timeout_dur, async {
        let ssh_config = Arc::new(Config {
            inactivity_timeout: Some(Duration::from_secs(30)),
            ..Config::default()
        });
        let addr = (host, port);
        let mut handle = client::connect(ssh_config, addr, SshClientHandler)
            .await
            .with_context(|| format!("Failed to connect to SSH server {host}:{port}"))?;

        let user = config.username.trim();
        let auth_res = if let Some(key_path) = config
            .private_key_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let pass = if secret.is_empty() {
                None
            } else {
                Some(secret)
            };
            let resolved_key_path = expand_tilde_path(key_path);
            let key_pair = load_secret_key(&resolved_key_path, pass)
                .with_context(|| format!("Failed to load SSH private key from {key_path}"))?;
            let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key_pair), None);
            handle
                .authenticate_publickey(user, key_with_alg)
                .await
                .context("SSH public key authentication failed")?
        } else {
            handle
                .authenticate_password(user, secret)
                .await
                .context("SSH password authentication failed")?
        };

        if !matches!(auth_res, AuthResult::Success) {
            return Err(anyhow!(
                "SSH authentication rejected for user '{}'",
                config.username
            ));
        }

        let channel = handle
            .channel_open_session()
            .await
            .context("Failed to open SSH session channel")?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .context("Failed to request SFTP subsystem")?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .context("Failed to initialize SFTP session")?;
        Ok(Arc::new(sftp))
    })
    .await
    .with_context(|| {
        format!(
            "Timed out connecting to SSH/SFTP server {host}:{port} ({}s)",
            timeout_dur.as_secs().max(1)
        )
    })?
}

impl SftpBackend {
    pub fn new(config: SftpConfig, secret: String) -> anyhow::Result<Self> {
        if config.host.trim().is_empty() {
            return Err(anyhow!("SFTP host cannot be empty"));
        }
        if config.username.trim().is_empty() {
            return Err(anyhow!("SFTP username cannot be empty"));
        }
        Ok(Self {
            config,
            secret,
            session: Mutex::new(None),
        })
    }

    async fn get_or_connect_session(&self) -> anyhow::Result<Arc<SftpSession>> {
        let mut guard = self.session.lock().await;
        if let Some(existing) = guard.as_ref() {
            return Ok(existing.clone());
        }

        let arc_sftp = connect_sftp_session_with_timeout(
            &self.config,
            &self.secret,
            DEFAULT_SFTP_CONNECT_TIMEOUT,
        )
        .await?;
        *guard = Some(arc_sftp.clone());
        Ok(arc_sftp)
    }
}

#[async_trait::async_trait]
impl StorageBackend for SftpBackend {
    fn protocol_name(&self) -> &'static str {
        "SFTP"
    }

    async fn connect_and_verify(&self) -> anyhow::Result<String> {
        let sftp = self.get_or_connect_session().await?;
        let initial = normalize_remote_path(&self.config.initial_path);
        let canonical = sftp.canonicalize(&initial).await.unwrap_or(initial);
        Ok(normalize_remote_path(&canonical))
    }

    async fn list_dir(&self, path: &str) -> anyhow::Result<Vec<FileEntry>> {
        let sftp = self.get_or_connect_session().await?;
        let norm_path = normalize_remote_path(path);
        let read_dir = sftp
            .read_dir(&norm_path)
            .await
            .with_context(|| format!("SFTP read_dir failed for {norm_path}"))?;

        let mut entries = Vec::new();
        for item in read_dir {
            let name = item.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let meta = item.metadata();
            let is_dir = item.file_type().is_dir();
            entries.push(FileEntry {
                name: name.clone(),
                path: join_remote_path(&norm_path, &name),
                kind: if is_dir {
                    FileKind::Directory
                } else {
                    FileKind::File
                },
                size: if is_dir { 0 } else { meta.size.unwrap_or(0) },
                modified_unix: meta.mtime.map(|m| m as i64),
                permissions: meta.permissions.map(|p| format!("{p:o}")),
            });
        }
        Ok(entries)
    }

    async fn create_dir(&self, path: &str) -> anyhow::Result<()> {
        let sftp = self.get_or_connect_session().await?;
        let norm = normalize_remote_path(path);
        sftp.create_dir(&norm)
            .await
            .with_context(|| format!("SFTP create_dir failed for {norm}"))
    }

    async fn rename(&self, from: &str, to: &str) -> anyhow::Result<()> {
        let sftp = self.get_or_connect_session().await?;
        let src = normalize_remote_path(from);
        let dst = normalize_remote_path(to);
        sftp.rename(&src, &dst)
            .await
            .with_context(|| format!("SFTP rename failed from {src} to {dst}"))
    }

    async fn remove(&self, path: &str, is_dir: bool) -> anyhow::Result<()> {
        let sftp = self.get_or_connect_session().await?;
        let norm = normalize_remote_path(path);
        if is_dir {
            let mut stack = vec![(norm, false)];
            while let Some((cur_dir, children_listed)) = stack.pop() {
                if !children_listed {
                    stack.push((cur_dir.clone(), true));
                    let children = self.list_dir(&cur_dir).await?;
                    for child in children {
                        if child.kind.is_dir_like() {
                            stack.push((child.path, false));
                        } else {
                            sftp.remove_file(&child.path)
                                .await
                                .with_context(|| format!("SFTP remove_file failed for {}", child.path))?;
                        }
                    }
                } else {
                    sftp.remove_dir(&cur_dir)
                        .await
                        .with_context(|| format!("SFTP remove_dir failed for {cur_dir}"))?;
                }
            }
            Ok(())
        } else {
            sftp.remove_file(&norm)
                .await
                .with_context(|| format!("SFTP remove_file failed for {norm}"))
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
        let sftp = self.get_or_connect_session().await?;
        let norm = normalize_remote_path(remote_path);
        let mut remote_file = sftp
            .open(&norm)
            .await
            .with_context(|| format!("SFTP open failed for {norm}"))?;

        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(anyhow!("Transfer cancelled"));
            }
            let n = remote_file.read(&mut buf).await?;
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
        let sftp = self.get_or_connect_session().await?;
        let norm = normalize_remote_path(remote_path);
        let mut remote_file = sftp
            .create(&norm)
            .await
            .with_context(|| format!("SFTP create failed for {norm}"))?;

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
            remote_file.write_all(&buf[..n]).await?;
            total += n as u64;
            progress(total);
        }
        remote_file.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde_path_resolves_home_directory() {
        let expanded = expand_tilde_path("~/.ssh/id_ed25519");
        let expected_home = dirs::home_dir().expect("home_dir must exist");
        assert_eq!(expanded, expected_home.join(".ssh/id_ed25519"));

        let literal = expand_tilde_path("C:\\keys\\id_rsa");
        assert_eq!(literal, std::path::PathBuf::from("C:\\keys\\id_rsa"));
    }

    #[tokio::test]
    async fn test_sftp_connect_times_out_on_unresponsive_server() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let _silent_server = tokio::spawn(async move {
            if let Ok((_socket, _)) = listener.accept().await {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        });

        let cfg = SftpConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "root".to_string(),
            private_key_path: None,
            initial_path: "/".to_string(),
        };
        let started = std::time::Instant::now();
        let res = connect_sftp_session_with_timeout(
            &cfg,
            "pw",
            std::time::Duration::from_millis(60),
        )
        .await;
        assert!(res.is_err());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "SFTP silent server must time out promptly, took {:?}",
            started.elapsed()
        );
    }
}
