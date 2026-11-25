use crate::models::{join_remote_path, normalize_remote_path, FileEntry, FileKind, SftpConfig};
use crate::storage::proxy::{connect_tcp_via_proxy, resolve_proxy_for_connection};
use crate::storage::s3::encode_s3_key_path;
use crate::storage::{
    encode_userinfo_component, format_host_for_url, ProgressCallback, RemoteUrlKind, StorageBackend,
};
use anyhow::{anyhow, Context};
use russh::client::{self, AuthResult, Config, Handle, Handler};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh_sftp::client::error::Error as SftpClientError;
use russh_sftp::client::SftpSession;
use std::future::Future;
use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_SFTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
pub const DEFAULT_SFTP_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
pub const DEFAULT_SFTP_KEEPALIVE_MAX: usize = 3;

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

pub fn build_sftp_ssh_config() -> Config {
    Config {
        // Never garbage-collect active tabs on idle; keep NAT/firewalls alive via SSH keepalives instead.
        inactivity_timeout: None,
        keepalive_interval: Some(DEFAULT_SFTP_KEEPALIVE_INTERVAL),
        keepalive_max: DEFAULT_SFTP_KEEPALIVE_MAX,
        ..Config::default()
    }
}

pub fn is_sftp_session_disconnect_error(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        if let Some(sftp_err) = cause.downcast_ref::<SftpClientError>() {
            return match sftp_err {
                SftpClientError::Status(_) | SftpClientError::Limited(_) => false,
                SftpClientError::IO(_)
                | SftpClientError::Timeout
                | SftpClientError::UnexpectedPacket
                | SftpClientError::UnexpectedBehavior(_) => true,
            };
        }
        if let Some(ssh_err) = cause.downcast_ref::<russh::Error>() {
            return matches!(
                ssh_err,
                russh::Error::Disconnect
                    | russh::Error::InactivityTimeout
                    | russh::Error::KeepaliveTimeout
                    | russh::Error::SendError
                    | russh::Error::HUP
                    | russh::Error::IO(_)
                    | russh::Error::ConnectionTimeout
            );
        }
        if let Some(io_err) = cause.downcast_ref::<std::io::Error>()
            && matches!(
                io_err.kind(),
                ErrorKind::UnexpectedEof
                    | ErrorKind::BrokenPipe
                    | ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::NotConnected
                    | ErrorKind::TimedOut
            )
        {
            return true;
        }
    }

    let msg = err.to_string().to_ascii_lowercase();
    msg.contains("channel closed")
        || msg.contains("senderror")
        || msg.contains("recverror")
        || msg.contains("broken pipe")
        || msg.contains("connection reset")
        || msg.contains("unexpected eof")
}

#[derive(Clone)]
struct SftpLiveSession {
    handle: Arc<Handle<SshClientHandler>>,
    sftp: Arc<SftpSession>,
}

impl SftpLiveSession {
    fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }
}

pub(crate) struct ReconnectSlot<T: Clone> {
    inner: Mutex<(u64, Option<T>)>,
    is_closed_check: fn(&T) -> bool,
}

impl<T: Clone> ReconnectSlot<T> {
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new((0, None)),
            is_closed_check: |_| false,
        }
    }

    pub fn with_liveness_check(is_closed_check: fn(&T) -> bool) -> Self {
        Self {
            inner: Mutex::new((0, None)),
            is_closed_check,
        }
    }

    pub async fn get_or_connect<CF, CFut>(&self, mut connect: CF) -> anyhow::Result<(u64, T)>
    where
        CF: FnMut() -> CFut,
        CFut: Future<Output = anyhow::Result<(T, bool)>>,
    {
        let mut guard = self.inner.lock().await;
        if let Some(existing) = guard.1.as_ref() {
            if !(self.is_closed_check)(existing) {
                return Ok((guard.0, existing.clone()));
            }
            guard.1 = None;
        }

        let (fresh, _) = connect().await?;
        guard.0 = guard.0.wrapping_add(1).max(1);
        let id = guard.0;
        guard.1 = Some(fresh.clone());
        Ok((id, fresh))
    }

    pub async fn evict_if_id(&self, failed_id: u64) {
        let mut guard = self.inner.lock().await;
        if guard.1.is_some() && guard.0 == failed_id {
            guard.1 = None;
        }
    }

    pub async fn run_with_retry_with_id<CF, CFut, OF, OFut, R>(
        &self,
        mut connect: CF,
        mut op: OF,
    ) -> anyhow::Result<(u64, R)>
    where
        CF: FnMut() -> CFut,
        CFut: Future<Output = anyhow::Result<(T, bool)>>,
        OF: FnMut(&T) -> OFut,
        OFut: Future<Output = anyhow::Result<R>>,
    {
        for attempt in 0..2 {
            let (session_id, session) = self.get_or_connect(&mut connect).await?;
            match op(&session).await {
                Ok(val) => return Ok((session_id, val)),
                Err(err) => {
                    if is_sftp_session_disconnect_error(&err) {
                        self.evict_if_id(session_id).await;
                        if attempt == 0 {
                            continue;
                        }
                    }
                    return Err(err);
                }
            }
        }
        unreachable!("retry loop always returns on attempt 0 or 1")
    }

    #[cfg(test)]
    pub async fn run_with_retry<CF, CFut, OF, OFut, R>(
        &self,
        connect: CF,
        op: OF,
    ) -> anyhow::Result<R>
    where
        CF: FnMut() -> CFut,
        CFut: Future<Output = anyhow::Result<(T, bool)>>,
        OF: FnMut(&T) -> OFut,
        OFut: Future<Output = anyhow::Result<R>>,
    {
        self.run_with_retry_with_id(connect, op)
            .await
            .map(|(_, val)| val)
    }
}

async fn connect_sftp_live_session_with_timeout(
    config: &SftpConfig,
    secret: &str,
    timeout_dur: Duration,
) -> anyhow::Result<SftpLiveSession> {
    let host = config.host.trim();
    let port = config.port;
    let resolved_proxy =
        resolve_proxy_for_connection(config.proxy_mode, config.proxy_url.as_deref());

    tokio::time::timeout(timeout_dur, async {
        let ssh_config = Arc::new(build_sftp_ssh_config());
        let tcp_stream = connect_tcp_via_proxy(host, port, &resolved_proxy, timeout_dur)
            .await
            .with_context(|| format!("Failed to connect to SSH server {host}:{port}"))?;

        let mut handle = client::connect_stream(ssh_config, tcp_stream, SshClientHandler)
            .await
            .with_context(|| format!("Failed SSH handshake with {host}:{port}"))?;

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
        Ok(SftpLiveSession {
            handle: Arc::new(handle),
            sftp: Arc::new(sftp),
        })
    })
    .await
    .with_context(|| {
        format!(
            "Timed out connecting to SSH/SFTP server {host}:{port} ({}s)",
            timeout_dur.as_secs().max(1)
        )
    })?
}

pub async fn connect_sftp_session_with_timeout(
    config: &SftpConfig,
    secret: &str,
    timeout_dur: Duration,
) -> anyhow::Result<Arc<SftpSession>> {
    let live = connect_sftp_live_session_with_timeout(config, secret, timeout_dur).await?;
    Ok(live.sftp)
}

pub struct SftpBackend {
    config: SftpConfig,
    secret: String,
    session_slot: ReconnectSlot<SftpLiveSession>,
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
            session_slot: ReconnectSlot::with_liveness_check(SftpLiveSession::is_closed),
        })
    }

    async fn run_sftp_with_id<OF, OFut, R>(&self, mut op: OF) -> anyhow::Result<(u64, R)>
    where
        OF: FnMut(Arc<SftpSession>) -> OFut,
        OFut: Future<Output = anyhow::Result<R>>,
    {
        let cfg = &self.config;
        let sec = &self.secret;
        self.session_slot
            .run_with_retry_with_id(
                || async move {
                    let live =
                        connect_sftp_live_session_with_timeout(cfg, sec, DEFAULT_SFTP_CONNECT_TIMEOUT)
                            .await?;
                    Ok((live, false))
                },
                |live| op(Arc::clone(&live.sftp)),
            )
            .await
    }

    async fn run_sftp<OF, OFut, R>(&self, op: OF) -> anyhow::Result<R>
    where
        OF: FnMut(Arc<SftpSession>) -> OFut,
        OFut: Future<Output = anyhow::Result<R>>,
    {
        self.run_sftp_with_id(op).await.map(|(_, val)| val)
    }
}

pub fn format_sftp_scp_path(config: &SftpConfig, remote_path: &str) -> String {
    let user = config.username.trim();
    let host = format_host_for_url(&config.host);
    let norm_path = normalize_remote_path(remote_path);
    if user.is_empty() {
        format!("{host}:{norm_path}")
    } else {
        format!("{user}@{host}:{norm_path}")
    }
}

pub fn build_sftp_url(config: &SftpConfig, remote_path: &str) -> String {
    let user = config.username.trim();
    let userinfo = if user.is_empty() {
        String::new()
    } else {
        format!("{}@", encode_userinfo_component(user))
    };
    let host = format_host_for_url(&config.host);
    let port_part = if config.port == 22 || config.port == 0 {
        String::new()
    } else {
        format!(":{}", config.port)
    };
    let norm_path = normalize_remote_path(remote_path);
    let encoded_path = encode_s3_key_path(&norm_path);
    format!("sftp://{userinfo}{host}{port_part}/{encoded_path}")
}

#[async_trait::async_trait]
impl StorageBackend for SftpBackend {
    fn protocol_name(&self) -> &'static str {
        "SFTP"
    }

    fn build_item_url(
        &self,
        entry: &FileEntry,
        kind: RemoteUrlKind,
    ) -> anyhow::Result<Option<String>> {
        match kind {
            RemoteUrlKind::SftpScp | RemoteUrlKind::Default => {
                Ok(Some(format_sftp_scp_path(&self.config, &entry.path)))
            }
            RemoteUrlKind::SftpUrl => Ok(Some(build_sftp_url(&self.config, &entry.path))),
            _ => Ok(None),
        }
    }

    async fn connect_and_verify(&self) -> anyhow::Result<String> {
        let initial = normalize_remote_path(&self.config.initial_path);
        self.run_sftp(|sftp| {
            let initial = initial.clone();
            async move {
                let canonical = sftp.canonicalize(&initial).await.unwrap_or(initial);
                Ok(normalize_remote_path(&canonical))
            }
        })
        .await
    }

    async fn list_dir(&self, path: &str) -> anyhow::Result<Vec<FileEntry>> {
        let norm_path = normalize_remote_path(path);
        let read_dir = self
            .run_sftp(|sftp| {
                let p = norm_path.clone();
                async move {
                    sftp.read_dir(&p)
                        .await
                        .with_context(|| format!("SFTP read_dir failed for {p}"))
                }
            })
            .await?;

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
        let norm = normalize_remote_path(path);
        self.run_sftp(|sftp| {
            let p = norm.clone();
            async move {
                sftp.create_dir(&p)
                    .await
                    .with_context(|| format!("SFTP create_dir failed for {p}"))
            }
        })
        .await
    }

    async fn rename(&self, from: &str, to: &str) -> anyhow::Result<()> {
        let src = normalize_remote_path(from);
        let dst = normalize_remote_path(to);
        self.run_sftp(|sftp| {
            let s = src.clone();
            let d = dst.clone();
            async move {
                sftp.rename(&s, &d)
                    .await
                    .with_context(|| format!("SFTP rename failed from {s} to {d}"))
            }
        })
        .await
    }

    async fn remove(&self, path: &str, is_dir: bool) -> anyhow::Result<()> {
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
                            let file_path = child.path;
                            self.run_sftp(|sftp| {
                                let fp = file_path.clone();
                                async move {
                                    sftp.remove_file(&fp)
                                        .await
                                        .with_context(|| format!("SFTP remove_file failed for {fp}"))
                                }
                            })
                            .await?;
                        }
                    }
                } else {
                    self.run_sftp(|sftp| {
                        let cd = cur_dir.clone();
                        async move {
                            sftp.remove_dir(&cd)
                                .await
                                .with_context(|| format!("SFTP remove_dir failed for {cd}"))
                        }
                    })
                    .await?;
                }
            }
            Ok(())
        } else {
            self.run_sftp(|sftp| {
                let p = norm.clone();
                async move {
                    sftp.remove_file(&p)
                        .await
                        .with_context(|| format!("SFTP remove_file failed for {p}"))
                }
            })
            .await
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
        let norm = normalize_remote_path(remote_path);
        let (session_id, mut remote_file) = self
            .run_sftp_with_id(|sftp| {
                let p = norm.clone();
                async move {
                    sftp.open(&p)
                        .await
                        .with_context(|| format!("SFTP open failed for {p}"))
                }
            })
            .await?;

        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(anyhow!("Transfer cancelled"));
            }
            let n = match remote_file.read(&mut buf).await {
                Ok(n) => n,
                Err(e) => {
                    let err = anyhow::Error::new(e).context(format!("SFTP read failed for {norm}"));
                    if is_sftp_session_disconnect_error(&err) {
                        self.session_slot.evict_if_id(session_id).await;
                    }
                    return Err(err);
                }
            };
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
        let norm = normalize_remote_path(remote_path);
        let (session_id, mut remote_file) = self
            .run_sftp_with_id(|sftp| {
                let p = norm.clone();
                async move {
                    sftp.create(&p)
                        .await
                        .with_context(|| format!("SFTP create failed for {p}"))
                }
            })
            .await?;

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
            if let Err(e) = remote_file.write_all(&buf[..n]).await {
                let err = anyhow::Error::new(e).context(format!("SFTP write failed for {norm}"));
                if is_sftp_session_disconnect_error(&err) {
                    self.session_slot.evict_if_id(session_id).await;
                }
                return Err(err);
            }
            total += n as u64;
            progress(total);
        }
        if let Err(e) = remote_file.flush().await {
            let err = anyhow::Error::new(e).context(format!("SFTP flush failed for {norm}"));
            if is_sftp_session_disconnect_error(&err) {
                self.session_slot.evict_if_id(session_id).await;
            }
            return Err(err);
        }
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
            proxy_mode: crate::models::ConnectionProxyMode::Direct,
            ..SftpConfig::default()
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

    #[test]
    fn test_sftp_ssh_config_enables_keepalive_and_disables_idle_gc() {
        let cfg = build_sftp_ssh_config();
        assert_eq!(cfg.inactivity_timeout, None);
        assert_eq!(cfg.keepalive_interval, Some(Duration::from_secs(15)));
        assert_eq!(cfg.keepalive_max, 3);
    }

    #[test]
    fn test_sftp_disconnect_error_classification_spares_application_status_errors() {
        use russh_sftp::client::error::Error as SftpErr;
        use russh_sftp::protocol::{Status, StatusCode};

        let chan_closed = anyhow::Error::new(SftpErr::UnexpectedBehavior(
            "SendError: channel closed".to_string(),
        ))
        .context("SFTP read_dir failed for /var");
        assert!(is_sftp_session_disconnect_error(&chan_closed));

        let io_err = anyhow::Error::new(SftpErr::IO("broken pipe".to_string()));
        assert!(is_sftp_session_disconnect_error(&io_err));

        let timeout_err = anyhow::Error::new(SftpErr::Timeout);
        assert!(is_sftp_session_disconnect_error(&timeout_err));

        let not_found_status = anyhow::Error::new(SftpErr::Status(Status {
            id: 1,
            status_code: StatusCode::NoSuchFile,
            error_message: "No such file".to_string(),
            language_tag: "en-US".to_string(),
        }))
        .context("SFTP read_dir failed for /missing");
        assert!(!is_sftp_session_disconnect_error(&not_found_status));
    }

    #[tokio::test]
    async fn test_reconnecting_session_slot_evicts_dead_session_once_and_spares_status_errors() {
        use russh_sftp::client::error::Error as SftpErr;
        use russh_sftp::protocol::{Status, StatusCode};
        use std::sync::atomic::{AtomicUsize, Ordering};

        let slot: ReconnectSlot<usize> = ReconnectSlot::new();
        let connect_calls = Arc::new(AtomicUsize::new(0));

        // 1. First session (conn #1) fails with closed channel -> auto-evicts and reconnects (conn #2) -> succeeds!
        let cc = connect_calls.clone();
        let val = slot
            .run_with_retry(
                || {
                    let cc = cc.clone();
                    async move {
                        let n = cc.fetch_add(1, Ordering::SeqCst) + 1;
                        Ok((n, false))
                    }
                },
                |&conn_num| async move {
                    if conn_num == 1 {
                        Err(anyhow::Error::new(SftpErr::UnexpectedBehavior(
                            "RecvError: channel closed".to_string(),
                        )))
                    } else {
                        Ok(format!("listed-on-conn-{conn_num}"))
                    }
                },
            )
            .await
            .unwrap();
        assert_eq!(val, "listed-on-conn-2");
        assert_eq!(connect_calls.load(Ordering::SeqCst), 2);

        // 2. Application error (NoSuchFile) on healthy conn #2 must NOT evict or reconnect!
        let cc2 = connect_calls.clone();
        let err = slot
            .run_with_retry(
                || {
                    let cc2 = cc2.clone();
                    async move {
                        let n = cc2.fetch_add(1, Ordering::SeqCst) + 1;
                        Ok((n, false))
                    }
                },
                |_conn| async move {
                    Err::<(), _>(anyhow::Error::new(SftpErr::Status(Status {
                        id: 2,
                        status_code: StatusCode::NoSuchFile,
                        error_message: "No such file".to_string(),
                        language_tag: "en".to_string(),
                    })))
                },
            )
            .await
            .unwrap_err();
        assert!(!is_sftp_session_disconnect_error(&err));
        assert_eq!(connect_calls.load(Ordering::SeqCst), 2);

        // 3. Stale session_id eviction (conn #1) must NOT evict healthy conn #2!
        slot.evict_if_id(1).await;
        let cc3 = connect_calls.clone();
        let (active_id, active_conn) = slot
            .run_with_retry_with_id(
                || {
                    let cc3 = cc3.clone();
                    async move {
                        let n = cc3.fetch_add(1, Ordering::SeqCst) + 1;
                        Ok((n, false))
                    }
                },
                |&conn_num| async move { Ok(conn_num) },
            )
            .await
            .unwrap();
        assert_eq!(active_id, 2);
        assert_eq!(active_conn, 2);
        assert_eq!(connect_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_sftp_build_item_url_scp_and_sftp_url_variants() {
        let cfg_default = SftpConfig {
            host: "10.0.0.5".to_string(),
            port: 22,
            username: "deploy".to_string(),
            ..SftpConfig::default()
        };
        let file_entry = FileEntry {
            name: "my file#1.txt".to_string(),
            path: "/var/www/my file#1.txt".to_string(),
            kind: FileKind::File,
            size: 128,
            modified_unix: None,
            permissions: None,
        };
        assert_eq!(
            format_sftp_scp_path(&cfg_default, &file_entry.path),
            "deploy@10.0.0.5:/var/www/my file#1.txt"
        );
        assert_eq!(
            build_sftp_url(&cfg_default, &file_entry.path),
            "sftp://deploy@10.0.0.5/var/www/my%20file%231.txt"
        );

        let backend_default = SftpBackend::new(cfg_default, String::new()).unwrap();
        assert_eq!(
            backend_default
                .build_item_url(&file_entry, RemoteUrlKind::SftpScp)
                .unwrap(),
            Some("deploy@10.0.0.5:/var/www/my file#1.txt".to_string())
        );
        assert_eq!(
            backend_default
                .build_item_url(&file_entry, RemoteUrlKind::Default)
                .unwrap(),
            Some("deploy@10.0.0.5:/var/www/my file#1.txt".to_string())
        );
        assert_eq!(
            backend_default
                .build_item_url(&file_entry, RemoteUrlKind::SftpUrl)
                .unwrap(),
            Some("sftp://deploy@10.0.0.5/var/www/my%20file%231.txt".to_string())
        );
        assert_eq!(
            backend_default
                .build_item_url(&file_entry, RemoteUrlKind::S3Http)
                .unwrap(),
            None
        );

        let root_entry = FileEntry {
            name: "/".to_string(),
            path: "/".to_string(),
            kind: FileKind::Directory,
            size: 0,
            modified_unix: None,
            permissions: None,
        };
        assert_eq!(
            backend_default
                .build_item_url(&root_entry, RemoteUrlKind::SftpUrl)
                .unwrap(),
            Some("sftp://deploy@10.0.0.5/".to_string())
        );

        let cfg_ipv6 = SftpConfig {
            host: "2001:db8::1".to_string(),
            port: 2222,
            username: "ops@team".to_string(),
            ..SftpConfig::default()
        };
        let nginx_entry = FileEntry {
            name: "nginx".to_string(),
            path: "/etc/nginx".to_string(),
            kind: FileKind::Directory,
            size: 0,
            modified_unix: None,
            permissions: None,
        };
        let backend_ipv6 = SftpBackend::new(cfg_ipv6, String::new()).unwrap();
        assert_eq!(
            backend_ipv6
                .build_item_url(&nginx_entry, RemoteUrlKind::SftpScp)
                .unwrap(),
            Some("ops@team@[2001:db8::1]:/etc/nginx".to_string())
        );
        assert_eq!(
            backend_ipv6
                .build_item_url(&nginx_entry, RemoteUrlKind::SftpUrl)
                .unwrap(),
            Some("sftp://ops%40team@[2001:db8::1]:2222/etc/nginx".to_string())
        );
    }
}
