use crate::models::ConnectionProxyMode;
use crate::services::preferences::{current_global_proxy_config, GlobalProxyMode};
use anyhow::{anyhow, Context};
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use base64::Engine as _;
use percent_encoding::percent_decode_str;
use reqwest::{Client, Proxy, Url};
use std::io;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedProxy {
    System,
    Direct,
    Custom(String),
}

pub fn normalize_proxy_url(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("socks5://") {
        Some(format!("socks5h://{}", &trimmed[9..]))
    } else if lower.starts_with("socks://") {
        Some(format!("socks5h://{}", &trimmed[8..]))
    } else if lower.starts_with("socks5h://") {
        Some(format!("socks5h://{}", &trimmed[10..]))
    } else if lower.starts_with("socks4://") {
        Some(format!("socks4a://{}", &trimmed[9..]))
    } else if lower.starts_with("socks4a://") {
        Some(format!("socks4a://{}", &trimmed[10..]))
    } else if lower.starts_with("http://") {
        Some(format!("http://{}", &trimmed[7..]))
    } else if lower.starts_with("https://") {
        Some(format!("https://{}", &trimmed[8..]))
    } else if trimmed.contains("://") {
        Some(trimmed.to_string())
    } else {
        Some(format!("http://{trimmed}"))
    }
}

pub fn resolve_global_proxy_from_parts(
    global_mode: GlobalProxyMode,
    global_url: Option<&str>,
) -> ResolvedProxy {
    match global_mode {
        GlobalProxyMode::System => ResolvedProxy::System,
        GlobalProxyMode::Direct => ResolvedProxy::Direct,
        GlobalProxyMode::Custom => match normalize_proxy_url(global_url) {
            Some(url) => ResolvedProxy::Custom(url),
            None => ResolvedProxy::System,
        },
    }
}

pub fn resolve_global_proxy() -> ResolvedProxy {
    let (mode, url) = current_global_proxy_config();
    resolve_global_proxy_from_parts(mode, url.as_deref())
}

pub fn resolve_effective_proxy(
    conn_mode: ConnectionProxyMode,
    conn_url: Option<&str>,
    global_mode: GlobalProxyMode,
    global_url: Option<&str>,
) -> ResolvedProxy {
    match conn_mode.effective_mode(conn_url) {
        ConnectionProxyMode::Direct => ResolvedProxy::Direct,
        ConnectionProxyMode::Custom => match normalize_proxy_url(conn_url) {
            Some(url) => ResolvedProxy::Custom(url),
            None => resolve_global_proxy_from_parts(global_mode, global_url),
        },
        ConnectionProxyMode::Default => resolve_global_proxy_from_parts(global_mode, global_url),
    }
}

pub fn resolve_proxy_for_connection(
    conn_mode: ConnectionProxyMode,
    conn_url: Option<&str>,
) -> ResolvedProxy {
    let (global_mode, global_url) = current_global_proxy_config();
    resolve_effective_proxy(conn_mode, conn_url, global_mode, global_url.as_deref())
}

pub fn build_http_client_for_proxy(
    resolved: &ResolvedProxy,
    connect_timeout: Duration,
    user_agent: Option<&str>,
) -> anyhow::Result<Client> {
    let mut builder = Client::builder().connect_timeout(connect_timeout);
    if let Some(ua) = user_agent {
        builder = builder.user_agent(ua);
    }
    match resolved {
        ResolvedProxy::System => {}
        ResolvedProxy::Direct => {
            builder = builder.no_proxy();
        }
        ResolvedProxy::Custom(proxy_url) => {
            let proxy = Proxy::all(proxy_url)
                .with_context(|| format!("Invalid proxy URL '{proxy_url}'"))?;
            builder = builder.proxy(proxy);
        }
    }
    builder.build().context("Failed to initialize HTTP client")
}

enum ProxyTransport {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl AsyncRead for ProxyTransport {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            ProxyTransport::Plain(s) => Pin::new(s).poll_read(cx, buf),
            ProxyTransport::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ProxyTransport {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            ProxyTransport::Plain(s) => Pin::new(s).poll_write(cx, buf),
            ProxyTransport::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            ProxyTransport::Plain(s) => Pin::new(s).poll_flush(cx),
            ProxyTransport::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            ProxyTransport::Plain(s) => Pin::new(s).poll_shutdown(cx),
            ProxyTransport::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

pub struct ProxyStream {
    prefix: Vec<u8>,
    pos: usize,
    inner: ProxyTransport,
}

impl ProxyStream {
    pub fn from_plain(inner: TcpStream) -> Self {
        Self {
            prefix: Vec::new(),
            pos: 0,
            inner: ProxyTransport::Plain(inner),
        }
    }

    fn with_prefix(prefix: Vec<u8>, inner: ProxyTransport) -> Self {
        Self {
            prefix,
            pos: 0,
            inner,
        }
    }
}

fn strip_ipv6_brackets(host: &str) -> &str {
    let h = host.trim();
    h.strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(h)
}

fn build_proxy_tls_connector() -> anyhow::Result<TlsConnector> {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .context("Failed to configure TLS protocol versions for HTTPS proxy")?
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

impl AsyncRead for ProxyStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.pos < self.prefix.len() {
            let remaining = &self.prefix[self.pos..];
            let to_copy = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            self.pos += to_copy;
            if self.pos >= self.prefix.len() {
                self.prefix.clear();
                self.pos = 0;
            }
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for ProxyStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

fn is_loopback_host(host: &str) -> bool {
    let h = strip_ipv6_brackets(host);
    h.eq_ignore_ascii_case("localhost") || h == "127.0.0.1" || h == "::1"
}

fn detect_env_proxy_url() -> Option<String> {
    for key in [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Ok(val) = std::env::var(key)
            && let Some(norm) = normalize_proxy_url(Some(&val))
        {
            return Some(norm);
        }
    }
    None
}

pub async fn connect_tcp_via_proxy(
    target_host: &str,
    target_port: u16,
    proxy: &ResolvedProxy,
    timeout: Duration,
) -> anyhow::Result<ProxyStream> {
    let clean_host = strip_ipv6_brackets(target_host);
    if clean_host.is_empty() {
        return Err(anyhow!("Target host cannot be empty"));
    }

    let effective_proxy_url = match proxy {
        ResolvedProxy::Direct => None,
        ResolvedProxy::System => {
            if is_loopback_host(clean_host) {
                None
            } else {
                detect_env_proxy_url()
            }
        }
        ResolvedProxy::Custom(url) => Some(url.clone()),
    };

    tokio::time::timeout(timeout, async move {
        match effective_proxy_url {
            None => {
                let sock = TcpStream::connect((clean_host, target_port))
                    .await
                    .with_context(|| format!("Failed to connect to {clean_host}:{target_port}"))?;
                Ok(ProxyStream::from_plain(sock))
            }
            Some(proxy_url) => connect_tunnel(&proxy_url, clean_host, target_port).await,
        }
    })
    .await
    .with_context(|| {
        format!(
            "Timed out connecting to {clean_host}:{target_port} ({}s)",
            timeout.as_secs().max(1)
        )
    })?
}

async fn connect_tunnel(
    proxy_url: &str,
    target_host: &str,
    target_port: u16,
) -> anyhow::Result<ProxyStream> {
    let parsed =
        Url::parse(proxy_url).with_context(|| format!("Invalid proxy URL '{proxy_url}'"))?;
    let raw_proxy_host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("Proxy URL missing host: '{proxy_url}'"))?;
    let proxy_host = strip_ipv6_brackets(raw_proxy_host);
    if proxy_host.is_empty() {
        return Err(anyhow!("Proxy URL missing host: '{proxy_url}'"));
    }
    let scheme = parsed.scheme().to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "socks5" | "socks5h" | "socks4" | "socks4a" => 1080,
        "https" => 443,
        _ => 8080,
    };
    let proxy_port = parsed.port().unwrap_or(default_port);

    let sock = TcpStream::connect((proxy_host, proxy_port))
        .await
        .with_context(|| format!("Failed to connect to proxy server {proxy_host}:{proxy_port}"))?;

    let username = if parsed.username().is_empty() {
        None
    } else {
        Some(
            percent_decode_str(parsed.username())
                .decode_utf8_lossy()
                .into_owned(),
        )
    };
    let password = parsed
        .password()
        .map(|p| percent_decode_str(p).decode_utf8_lossy().into_owned());

    match scheme.as_str() {
        "socks5" | "socks5h" => {
            connect_socks5_tunnel(
                sock,
                target_host,
                target_port,
                username.as_deref(),
                password.as_deref(),
            )
            .await
        }
        "http" => {
            connect_http_connect_tunnel(
                ProxyTransport::Plain(sock),
                target_host,
                target_port,
                username.as_deref(),
                password.as_deref(),
            )
            .await
        }
        "https" => {
            let connector = build_proxy_tls_connector()?;
            let server_name = rustls::pki_types::ServerName::try_from(proxy_host.to_string())
                .map_err(|_| anyhow!("Invalid TLS server name for HTTPS proxy '{proxy_host}'"))?;
            let tls_stream = connector
                .connect(server_name, sock)
                .await
                .with_context(|| {
                    format!("TLS handshake with HTTPS proxy {proxy_host}:{proxy_port} failed")
                })?;
            connect_http_connect_tunnel(
                ProxyTransport::Tls(Box::new(tls_stream)),
                target_host,
                target_port,
                username.as_deref(),
                password.as_deref(),
            )
            .await
        }
        other => Err(anyhow!(
            "Unsupported TCP tunnel proxy scheme '{other}' (use http://, https://, or socks5://)"
        )),
    }
}

async fn connect_socks5_tunnel(
    mut sock: TcpStream,
    target_host: &str,
    target_port: u16,
    username: Option<&str>,
    password: Option<&str>,
) -> anyhow::Result<ProxyStream> {
    if username.is_some() {
        sock.write_all(&[0x05, 0x02, 0x00, 0x02]).await?;
    } else {
        sock.write_all(&[0x05, 0x01, 0x00]).await?;
    }
    sock.flush().await?;

    let mut method_resp = [0u8; 2];
    sock.read_exact(&mut method_resp)
        .await
        .context("Failed to read SOCKS5 greeting response")?;
    if method_resp[0] != 0x05 {
        return Err(anyhow!(
            "Invalid SOCKS5 proxy version byte: 0x{:02x}",
            method_resp[0]
        ));
    }

    match method_resp[1] {
        0x00 => {}
        0x02 => {
            let user = username.unwrap_or_default().as_bytes();
            let pass = password.unwrap_or_default().as_bytes();
            if user.len() > 255 || pass.len() > 255 {
                return Err(anyhow!("SOCKS5 credentials exceed 255 bytes"));
            }
            let mut auth_pkt = Vec::with_capacity(3 + user.len() + pass.len());
            auth_pkt.push(0x01);
            auth_pkt.push(user.len() as u8);
            auth_pkt.extend_from_slice(user);
            auth_pkt.push(pass.len() as u8);
            auth_pkt.extend_from_slice(pass);
            sock.write_all(&auth_pkt).await?;
            sock.flush().await?;

            let mut auth_resp = [0u8; 2];
            sock.read_exact(&mut auth_resp)
                .await
                .context("Failed to read SOCKS5 auth response")?;
            if auth_resp[1] != 0x00 {
                return Err(anyhow!("SOCKS5 proxy authentication failed"));
            }
        }
        0xFF => {
            return Err(anyhow!(
                "SOCKS5 proxy rejected authentication methods (0xFF)"
            ));
        }
        m => {
            return Err(anyhow!(
                "Unsupported SOCKS5 authentication method selected: 0x{m:02x}"
            ));
        }
    }

    let mut req = Vec::with_capacity(7 + target_host.len());
    req.extend_from_slice(&[0x05, 0x01, 0x00]);
    if let Ok(ip) = target_host.parse::<IpAddr>() {
        match ip {
            IpAddr::V4(v4) => {
                req.push(0x01);
                req.extend_from_slice(&v4.octets());
            }
            IpAddr::V6(v6) => {
                req.push(0x04);
                req.extend_from_slice(&v6.octets());
            }
        }
    } else {
        let host_bytes = target_host.as_bytes();
        if host_bytes.len() > 255 {
            return Err(anyhow!("Target hostname exceeds 255 bytes for SOCKS5"));
        }
        req.push(0x03);
        req.push(host_bytes.len() as u8);
        req.extend_from_slice(host_bytes);
    }
    req.extend_from_slice(&target_port.to_be_bytes());
    sock.write_all(&req).await?;
    sock.flush().await?;

    let mut rep_hdr = [0u8; 4];
    sock.read_exact(&mut rep_hdr)
        .await
        .context("Failed to read SOCKS5 CONNECT reply header")?;
    if rep_hdr[0] != 0x05 {
        return Err(anyhow!(
            "Invalid SOCKS5 reply version: 0x{:02x}",
            rep_hdr[0]
        ));
    }
    if rep_hdr[1] != 0x00 {
        return Err(anyhow!(
            "SOCKS5 CONNECT to {target_host}:{target_port} failed (reply code 0x{:02x})",
            rep_hdr[1]
        ));
    }

    match rep_hdr[3] {
        0x01 => {
            let mut tail = [0u8; 6];
            sock.read_exact(&mut tail).await?;
        }
        0x04 => {
            let mut tail = [0u8; 18];
            sock.read_exact(&mut tail).await?;
        }
        0x03 => {
            let mut len_b = [0u8; 1];
            sock.read_exact(&mut len_b).await?;
            let mut tail = vec![0u8; (len_b[0] as usize) + 2];
            sock.read_exact(&mut tail).await?;
        }
        atyp => {
            return Err(anyhow!("Unsupported SOCKS5 reply ATYP: 0x{atyp:02x}"));
        }
    }

    Ok(ProxyStream::from_plain(sock))
}

async fn connect_http_connect_tunnel(
    mut sock: ProxyTransport,
    target_host: &str,
    target_port: u16,
    username: Option<&str>,
    password: Option<&str>,
) -> anyhow::Result<ProxyStream> {
    let clean_target = strip_ipv6_brackets(target_host);
    let authority = if clean_target.contains(':') {
        format!("[{clean_target}]:{target_port}")
    } else {
        format!("{clean_target}:{target_port}")
    };

    let mut req = format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: keep-alive\r\n"
    );
    if let Some(user) = username {
        let pass = password.unwrap_or_default();
        let encoded = BASE64_STD.encode(format!("{user}:{pass}"));
        req.push_str(&format!("Proxy-Authorization: Basic {encoded}\r\n"));
    }
    req.push_str("\r\n");

    sock.write_all(req.as_bytes()).await?;
    sock.flush().await?;

    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 512];
    let header_end = loop {
        if buf.len() > 16 * 1024 {
            return Err(anyhow!("HTTP CONNECT proxy response header exceeded 16 KiB"));
        }
        let n = sock
            .read(&mut chunk)
            .await
            .context("Failed reading HTTP CONNECT proxy response")?;
        if n == 0 {
            return Err(anyhow!("HTTP proxy closed connection during CONNECT"));
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };

    let header_str = String::from_utf8_lossy(&buf[..header_end]);
    let status_line = header_str.lines().next().unwrap_or_default().trim();
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    if status_code != 200 {
        return Err(anyhow!(
            "HTTP proxy CONNECT failed for {authority}: {status_line}"
        ));
    }

    let remainder = buf[header_end..].to_vec();
    Ok(ProxyStream::with_prefix(remainder, sock))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ConnectionProxyMode;
    use crate::services::preferences::GlobalProxyMode;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn test_normalize_and_resolve_proxy_precedence_and_legacy_compat() {
        assert_eq!(normalize_proxy_url(None), None);
        assert_eq!(normalize_proxy_url(Some("   ")), None);
        assert_eq!(
            normalize_proxy_url(Some("127.0.0.1:7890")),
            Some("http://127.0.0.1:7890".to_string())
        );
        assert_eq!(
            normalize_proxy_url(Some("http://127.0.0.1:7890")),
            Some("http://127.0.0.1:7890".to_string())
        );
        // socks5:// and socks4:// upgrade to remote DNS resolution (socks5h:// / socks4a://)
        assert_eq!(
            normalize_proxy_url(Some("socks5://127.0.0.1:1080")),
            Some("socks5h://127.0.0.1:1080".to_string())
        );
        assert_eq!(
            normalize_proxy_url(Some("SOCKS5://user:pass@127.0.0.1:1080")),
            Some("socks5h://user:pass@127.0.0.1:1080".to_string())
        );
        assert_eq!(
            normalize_proxy_url(Some("socks4://127.0.0.1:1080")),
            Some("socks4a://127.0.0.1:1080".to_string())
        );

        // Legacy bookmark compat: Default mode + non-empty proxy_url -> Custom
        assert_eq!(
            resolve_effective_proxy(
                ConnectionProxyMode::Default,
                Some("127.0.0.1:7890"),
                GlobalProxyMode::System,
                None,
            ),
            ResolvedProxy::Custom("http://127.0.0.1:7890".to_string())
        );

        // Per-connection Direct overrides Global Custom
        assert_eq!(
            resolve_effective_proxy(
                ConnectionProxyMode::Direct,
                Some("127.0.0.1:9999"),
                GlobalProxyMode::Custom,
                Some("http://127.0.0.1:7890"),
            ),
            ResolvedProxy::Direct
        );

        // Per-connection Default inherits Global Custom
        assert_eq!(
            resolve_effective_proxy(
                ConnectionProxyMode::Default,
                None,
                GlobalProxyMode::Custom,
                Some("socks5://127.0.0.1:1080"),
            ),
            ResolvedProxy::Custom("socks5h://127.0.0.1:1080".to_string())
        );

        // Per-connection Default inherits Global System when no URLs set
        assert_eq!(
            resolve_effective_proxy(
                ConnectionProxyMode::Default,
                None,
                GlobalProxyMode::System,
                None,
            ),
            ResolvedProxy::System
        );
    }

    #[tokio::test]
    async fn test_connect_tcp_via_socks5_and_http_connect_preserves_exact_stream_framing() {
        // 1. Mock SOCKS5 server testing username/password auth + ATYP=0x03 domain + domain BND.ADDR + immediate banner
        let socks_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socks_port = socks_listener.local_addr().unwrap().port();
        let socks_task = tokio::spawn(async move {
            let (mut sock, _) = socks_listener.accept().await.unwrap();
            let mut greet = [0u8; 4];
            sock.read_exact(&mut greet).await.unwrap();
            assert_eq!(greet[0], 0x05);
            // Select username/password auth (0x02)
            sock.write_all(&[0x05, 0x02]).await.unwrap();

            let mut auth_hdr = [0u8; 2];
            sock.read_exact(&mut auth_hdr).await.unwrap();
            assert_eq!(auth_hdr[0], 0x01);
            let ulen = auth_hdr[1] as usize;
            let mut ubuf = vec![0u8; ulen];
            sock.read_exact(&mut ubuf).await.unwrap();
            assert_eq!(&ubuf, b"alice");
            let mut plen_b = [0u8; 1];
            sock.read_exact(&mut plen_b).await.unwrap();
            let mut pbuf = vec![0u8; plen_b[0] as usize];
            sock.read_exact(&mut pbuf).await.unwrap();
            assert_eq!(&pbuf, b"secret");
            sock.write_all(&[0x01, 0x00]).await.unwrap();

            // Read CONNECT request with ATYP=0x03 domain
            let mut req_hdr = [0x00u8; 4];
            sock.read_exact(&mut req_hdr).await.unwrap();
            assert_eq!(req_hdr, [0x05, 0x01, 0x00, 0x03]);
            let mut dlen = [0u8; 1];
            sock.read_exact(&mut dlen).await.unwrap();
            let mut domain = vec![0u8; dlen[0] as usize];
            sock.read_exact(&mut domain).await.unwrap();
            assert_eq!(&domain, b"sftp.internal.example");
            let mut port_b = [0u8; 2];
            sock.read_exact(&mut port_b).await.unwrap();
            assert_eq!(u16::from_be_bytes(port_b), 2222);

            // Reply with domain BND.ADDR + coalesced SSH banner in the exact same write!
            let mut reply = vec![0x05, 0x00, 0x00, 0x03, 4];
            reply.extend_from_slice(b"bind");
            reply.extend_from_slice(&2222u16.to_be_bytes());
            reply.extend_from_slice(b"SSH-2.0-MockSocks\r\n");
            sock.write_all(&reply).await.unwrap();
        });

        let mut stream = connect_tcp_via_proxy(
            "sftp.internal.example",
            2222,
            &ResolvedProxy::Custom(format!("socks5h://alice:secret@127.0.0.1:{socks_port}")),
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        let mut banner = vec![0u8; 19];
        stream.read_exact(&mut banner).await.unwrap();
        assert_eq!(&banner, b"SSH-2.0-MockSocks\r\n");
        socks_task.await.unwrap();

        // 2. Mock HTTP CONNECT server sending 200 OK + coalesced SSH banner in single segment
        let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_port = http_listener.local_addr().unwrap().port();
        let http_task = tokio::spawn(async move {
            let (mut sock, _) = http_listener.accept().await.unwrap();
            let mut req_buf = vec![0u8; 1024];
            let n = sock.read(&mut req_buf).await.unwrap();
            let req_txt = String::from_utf8_lossy(&req_buf[..n]);
            assert!(req_txt.starts_with("CONNECT sftp.internal.example:22 HTTP/1.1\r\n"));
            assert!(req_txt.contains("Proxy-Authorization: Basic "));
            sock.write_all(
                b"HTTP/1.1 200 Connection Established\r\nProxy-Agent: Mock\r\n\r\nSSH-2.0-MockHttp\r\n",
            )
            .await
            .unwrap();
        });

        let mut http_stream = connect_tcp_via_proxy(
            "sftp.internal.example",
            22,
            &ResolvedProxy::Custom(format!("http://bob:pw@127.0.0.1:{http_port}")),
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        let mut http_banner = vec![0u8; 18];
        http_stream.read_exact(&mut http_banner).await.unwrap();
        assert_eq!(
            http_banner.as_slice(),
            b"SSH-2.0-MockHttp\r\n"
        );
        http_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_https_proxy_initiates_tls_handshake_and_never_sends_plaintext_credentials() {
        let raw_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = raw_listener.local_addr().unwrap().port();
        let captured = tokio::spawn(async move {
            let (mut sock, _) = raw_listener.accept().await.unwrap();
            let mut buf = vec![0u8; 2048];
            let n = sock.read(&mut buf).await.unwrap();
            buf.truncate(n);
            let _ = sock
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await;
            buf
        });

        let res = connect_tcp_via_proxy(
            "sftp.internal.example",
            22,
            &ResolvedProxy::Custom(format!("https://alice:supersecret@127.0.0.1:{port}")),
            Duration::from_secs(2),
        )
        .await;

        assert!(
            res.is_err(),
            "https:// proxy against non-TLS server must fail TLS handshake"
        );
        let wire_bytes = captured.await.unwrap();
        assert!(!wire_bytes.is_empty(), "expected TLS ClientHello bytes");
        assert_eq!(
            wire_bytes[0], 0x16,
            "expected TLS Handshake record (0x16), got 0x{:02x}",
            wire_bytes[0]
        );
        let lossy = String::from_utf8_lossy(&wire_bytes);
        let b64_creds = BASE64_STD.encode("alice:supersecret");
        assert!(!lossy.contains("CONNECT "));
        assert!(!lossy.contains("Proxy-Authorization"));
        assert!(!lossy.contains("supersecret"));
        assert!(!lossy.contains(&b64_creds));
    }

    #[tokio::test]
    async fn test_ipv6_bracketed_proxy_and_target_hosts_and_scheme_normalization() {
        assert_eq!(
            normalize_proxy_url(Some("socks://127.0.0.1:1080")),
            Some("socks5h://127.0.0.1:1080".to_string())
        );
        assert_eq!(
            normalize_proxy_url(Some("ftp://127.0.0.1:21")),
            Some("ftp://127.0.0.1:21".to_string())
        );

        let Ok(v6_listener) = TcpListener::bind("[::1]:0").await else {
            return;
        };
        let v6_port = v6_listener.local_addr().unwrap().port();
        let v6_task = tokio::spawn(async move {
            let (mut sock, _) = v6_listener.accept().await.unwrap();
            let mut greet = [0u8; 3];
            sock.read_exact(&mut greet).await.unwrap();
            sock.write_all(&[0x05, 0x00]).await.unwrap();

            let mut req_hdr = [0u8; 4];
            sock.read_exact(&mut req_hdr).await.unwrap();
            // ATYP must be 0x04 (16-byte IPv6), NOT 0x03 ("[2001:db8::42]")
            assert_eq!(req_hdr, [0x05, 0x01, 0x00, 0x04]);
            let mut v6_octets = [0u8; 16];
            sock.read_exact(&mut v6_octets).await.unwrap();
            let expected: std::net::Ipv6Addr = "2001:db8::42".parse().unwrap();
            assert_eq!(v6_octets, expected.octets());
            let mut port_b = [0u8; 2];
            sock.read_exact(&mut port_b).await.unwrap();
            assert_eq!(u16::from_be_bytes(port_b), 2222);

            let mut reply = vec![0x05, 0x00, 0x00, 0x04];
            reply.extend_from_slice(&std::net::Ipv6Addr::LOCALHOST.octets());
            reply.extend_from_slice(&2222u16.to_be_bytes());
            sock.write_all(&reply).await.unwrap();
        });

        let proxy_url = normalize_proxy_url(Some(&format!("socks5://[::1]:{v6_port}"))).unwrap();
        let _stream = connect_tcp_via_proxy(
            "[2001:db8::42]",
            2222,
            &ResolvedProxy::Custom(proxy_url),
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        v6_task.await.unwrap();
    }
}
