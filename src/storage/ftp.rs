use crate::models::{join_remote_path, normalize_remote_path, FileEntry, FileKind, FtpConfig};
use crate::storage::{ProgressCallback, StorageBackend};
use anyhow::{anyhow, Context};
use chrono::NaiveDateTime;
use rustls::pki_types::ServerName;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;
use tokio_util::sync::CancellationToken;

enum FtpStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for FtpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            FtpStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            FtpStream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for FtpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            FtpStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            FtpStream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            FtpStream::Plain(s) => Pin::new(s).poll_flush(cx),
            FtpStream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            FtpStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            FtpStream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

fn build_tls_connector() -> anyhow::Result<TlsConnector> {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

async fn read_ftp_response(reader: &mut BufReader<FtpStream>) -> anyhow::Result<(u16, String)> {
    let mut full_text = String::new();
    let mut expected_end_prefix: Option<String> = None;
    let mut code: u16 = 0;

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(anyhow!("FTP server closed connection unexpectedly"));
        }
        full_text.push_str(&line);
        let trimmed = line.trim_end_matches(&['\r', '\n'][..]);
        if trimmed.len() >= 3 {
            if let Ok(parsed_code) = trimmed[0..3].parse::<u16>() {
                let sep = trimmed.as_bytes().get(3).copied().unwrap_or(b' ');
                if expected_end_prefix.is_none() {
                    code = parsed_code;
                    if sep == b'-' {
                        expected_end_prefix = Some(format!("{parsed_code} "));
                        continue;
                    } else {
                        break;
                    }
                } else if Some(format!("{parsed_code} ")) == expected_end_prefix && sep == b' ' {
                    break;
                }
            }
        }
    }
    Ok((code, full_text))
}

async fn send_ftp_cmd(
    reader: &mut BufReader<FtpStream>,
    cmd: &str,
) -> anyhow::Result<(u16, String)> {
    let wire = format!("{cmd}\r\n");
    reader.get_mut().write_all(wire.as_bytes()).await?;
    reader.get_mut().flush().await?;
    read_ftp_response(reader).await
}

pub fn parse_epsv_port(resp: &str) -> Option<u16> {
    // 229 Entering Extended Passive Mode (|||12345|)
    let start = resp.find("|||")?;
    let rest = &resp[start + 3..];
    let end = rest.find('|')?;
    rest[..end].trim().parse::<u16>().ok()
}

pub fn parse_pasv_port(resp: &str) -> Option<u16> {
    // 227 Entering Passive Mode (192,168,1,1,195,149).
    let open = resp.find('(')?;
    let close = resp[open..].find(')')? + open;
    let nums: Vec<u16> = resp[open + 1..close]
        .split(',')
        .filter_map(|s| s.trim().parse::<u16>().ok())
        .collect();
    if nums.len() == 6 {
        Some(nums[4] * 256 + nums[5])
    } else {
        None
    }
}

pub fn parse_pwd_path(resp: &str) -> Option<String> {
    let first = resp.find('"')?;
    let rest = &resp[first + 1..];
    let second = rest.find('"')?;
    Some(normalize_remote_path(&rest[..second]))
}

pub fn parse_mlsd_line(parent_path: &str, line: &str) -> Option<FileEntry> {
    let trimmed = line.trim_matches(&['\r', '\n'][..]);
    if trimmed.is_empty() {
        return None;
    }
    let (facts_str, raw_name) = trimmed.split_once(' ')?;
    let name = raw_name.trim();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }

    let mut is_dir = false;
    let mut size = 0u64;
    let mut modified_unix = None;
    let mut permissions = None;

    for fact in facts_str.split(';') {
        if let Some((k, v)) = fact.split_once('=') {
            match k.to_ascii_lowercase().as_str() {
                "type" => {
                    let vl = v.to_ascii_lowercase();
                    if vl == "cdir" || vl == "pdir" {
                        return None;
                    }
                    is_dir = vl == "dir";
                }
                "size" => {
                    size = v.parse::<u64>().unwrap_or(0);
                }
                "modify" => {
                    if v.len() >= 14 {
                        if let Ok(dt) = NaiveDateTime::parse_from_str(&v[..14], "%Y%m%d%H%M%S") {
                            modified_unix = Some(dt.and_utc().timestamp());
                        }
                    }
                }
                "unix.mode" | "perm" => {
                    permissions = Some(v.to_string());
                }
                _ => {}
            }
        }
    }

    Some(FileEntry {
        name: name.to_string(),
        path: join_remote_path(parent_path, name),
        kind: if is_dir {
            FileKind::Directory
        } else {
            FileKind::File
        },
        size: if is_dir { 0 } else { size },
        modified_unix,
        permissions,
    })
}

pub fn parse_list_line(parent_path: &str, line: &str) -> Option<FileEntry> {
    let trimmed = line.trim_matches(&['\r', '\n'][..]).trim();
    if trimmed.is_empty() || trimmed.starts_with("total ") {
        return None;
    }

    // DOS / Windows IIS style: "11-10-26  12:30PM       <DIR>          FolderName"
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() >= 4
        && parts[0].chars().next().is_some_and(|c| c.is_ascii_digit())
        && (parts[2].eq_ignore_ascii_case("<DIR>") || parts[2].chars().all(|c| c.is_ascii_digit()))
    {
        let is_dir = parts[2].eq_ignore_ascii_case("<DIR>");
        let size = if is_dir {
            0
        } else {
            parts[2].parse::<u64>().unwrap_or(0)
        };
        let name = parts[3..].join(" ");
        if name == "." || name == ".." || name.is_empty() {
            return None;
        }
        return Some(FileEntry {
            name: name.clone(),
            path: join_remote_path(parent_path, &name),
            kind: if is_dir {
                FileKind::Directory
            } else {
                FileKind::File
            },
            size,
            modified_unix: None,
            permissions: None,
        });
    }

    // Unix style: "drwxr-xr-x    2 1000     1000         4096 Nov 10 12:00 releases"
    if parts.len() >= 9 {
        let perm = parts[0];
        let first_char = perm.chars().next()?;
        let is_dir = first_char == 'd';
        let size = if is_dir {
            0
        } else {
            parts[4].parse::<u64>().unwrap_or(0)
        };
        // Find where the 9th whitespace-separated token begins in `trimmed`
        let mut token_idx = 0;
        let mut in_ws = true;
        let mut name_start = 0;
        for (i, ch) in trimmed.char_indices() {
            if ch.is_whitespace() {
                in_ws = true;
            } else if in_ws {
                in_ws = false;
                token_idx += 1;
                if token_idx == 9 {
                    name_start = i;
                    break;
                }
            }
        }
        let raw_name = trimmed[name_start..].trim();
        let name = if first_char == 'l' {
            raw_name.split(" -> ").next().unwrap_or(raw_name).trim()
        } else {
            raw_name
        };
        if name == "." || name == ".." || name.is_empty() {
            return None;
        }
        return Some(FileEntry {
            name: name.to_string(),
            path: join_remote_path(parent_path, name),
            kind: if is_dir {
                FileKind::Directory
            } else {
                FileKind::File
            },
            size,
            modified_unix: None,
            permissions: Some(perm.to_string()),
        });
    }

    None
}

pub struct FtpBackend {
    config: FtpConfig,
    password: String,
}

impl FtpBackend {
    pub fn new(config: FtpConfig, password: String) -> anyhow::Result<Self> {
        if config.host.trim().is_empty() {
            return Err(anyhow!("FTP host cannot be empty"));
        }
        Ok(Self { config, password })
    }

    async fn connect_control(&self) -> anyhow::Result<BufReader<FtpStream>> {
        let host = self.config.host.trim();
        let port = self.config.port;
        let tcp = TcpStream::connect((host, port))
            .await
            .with_context(|| format!("Failed to connect to FTP server {host}:{port}"))?;

        let mut reader = if self.config.use_tls && port == 990 {
            let connector = build_tls_connector()?;
            let server_name = ServerName::try_from(host.to_string())
                .map_err(|_| anyhow!("Invalid TLS server name: {host}"))?;
            let tls = connector
                .connect(server_name, tcp)
                .await
                .context("Implicit FTPS TLS handshake failed")?;
            BufReader::new(FtpStream::Tls(Box::new(tls)))
        } else {
            BufReader::new(FtpStream::Plain(tcp))
        };

        let (greeting_code, greeting_txt) = read_ftp_response(&mut reader).await?;
        if greeting_code >= 400 {
            return Err(anyhow!("FTP greeting rejected: {}", greeting_txt.trim()));
        }

        if self.config.use_tls && port != 990 {
            let (auth_code, auth_txt) = send_ftp_cmd(&mut reader, "AUTH TLS").await?;
            if auth_code != 234 {
                return Err(anyhow!("FTPS AUTH TLS rejected: {}", auth_txt.trim()));
            }
            let inner_stream = reader.into_inner();
            let FtpStream::Plain(plain_tcp) = inner_stream else {
                return Err(anyhow!("Unexpected stream state during AUTH TLS"));
            };
            let connector = build_tls_connector()?;
            let server_name = ServerName::try_from(host.to_string())
                .map_err(|_| anyhow!("Invalid TLS server name: {host}"))?;
            let tls = connector
                .connect(server_name, plain_tcp)
                .await
                .context("Explicit FTPS TLS handshake failed")?;
            reader = BufReader::new(FtpStream::Tls(Box::new(tls)));
        }

        let user = if self.config.username.trim().is_empty() {
            "anonymous"
        } else {
            self.config.username.trim()
        };
        let (user_code, user_txt) = send_ftp_cmd(&mut reader, &format!("USER {user}")).await?;
        if user_code == 331 {
            let (pass_code, pass_txt) =
                send_ftp_cmd(&mut reader, &format!("PASS {}", self.password)).await?;
            if pass_code >= 400 {
                return Err(anyhow!("FTP login failed: {}", pass_txt.trim()));
            }
        } else if user_code >= 400 {
            return Err(anyhow!("FTP USER rejected: {}", user_txt.trim()));
        }

        if self.config.use_tls {
            let _ = send_ftp_cmd(&mut reader, "PBSZ 0").await;
            let _ = send_ftp_cmd(&mut reader, "PROT P").await;
        }
        let _ = send_ftp_cmd(&mut reader, "OPTS UTF8 ON").await;
        let _ = send_ftp_cmd(&mut reader, "TYPE I").await;

        Ok(reader)
    }

    async fn open_passive_data_stream(
        &self,
        control: &mut BufReader<FtpStream>,
    ) -> anyhow::Result<FtpStream> {
        let host = self.config.host.trim();
        let data_port = {
            let (epsv_code, epsv_txt) = send_ftp_cmd(control, "EPSV").await?;
            if epsv_code == 229 {
                parse_epsv_port(&epsv_txt)
                    .ok_or_else(|| anyhow!("Failed to parse EPSV response: {}", epsv_txt.trim()))?
            } else {
                let (pasv_code, pasv_txt) = send_ftp_cmd(control, "PASV").await?;
                if pasv_code != 227 {
                    return Err(anyhow!("FTP PASV failed: {}", pasv_txt.trim()));
                }
                parse_pasv_port(&pasv_txt)
                    .ok_or_else(|| anyhow!("Failed to parse PASV response: {}", pasv_txt.trim()))?
            }
        };

        let tcp = TcpStream::connect((host, data_port))
            .await
            .with_context(|| format!("Failed to connect FTP passive data channel {host}:{data_port}"))?;

        if self.config.use_tls {
            let connector = build_tls_connector()?;
            let server_name = ServerName::try_from(host.to_string())
                .map_err(|_| anyhow!("Invalid TLS server name: {host}"))?;
            let tls = connector
                .connect(server_name, tcp)
                .await
                .context("FTPS data channel TLS handshake failed")?;
            Ok(FtpStream::Tls(Box::new(tls)))
        } else {
            Ok(FtpStream::Plain(tcp))
        }
    }
}

#[async_trait::async_trait]
impl StorageBackend for FtpBackend {
    fn protocol_name(&self) -> &'static str {
        if self.config.use_tls {
            "FTPS"
        } else {
            "FTP"
        }
    }

    async fn connect_and_verify(&self) -> anyhow::Result<String> {
        let mut control = self.connect_control().await?;
        let initial = normalize_remote_path(&self.config.initial_path);
        if initial != "/" {
            let (code, txt) = send_ftp_cmd(&mut control, &format!("CWD {initial}")).await?;
            if code >= 400 {
                return Err(anyhow!("FTP CWD {initial} failed: {}", txt.trim()));
            }
        }
        let (pwd_code, pwd_txt) = send_ftp_cmd(&mut control, "PWD").await?;
        let _ = send_ftp_cmd(&mut control, "QUIT").await;
        if pwd_code == 257 {
            if let Some(p) = parse_pwd_path(&pwd_txt) {
                return Ok(p);
            }
        }
        Ok(initial)
    }

    async fn list_dir(&self, path: &str) -> anyhow::Result<Vec<FileEntry>> {
        let mut control = self.connect_control().await?;
        let norm_path = normalize_remote_path(path);
        let (cwd_code, cwd_txt) = send_ftp_cmd(&mut control, &format!("CWD {norm_path}")).await?;
        if cwd_code >= 400 {
            return Err(anyhow!("FTP CWD {norm_path} failed: {}", cwd_txt.trim()));
        }

        let mut data_stream = self.open_passive_data_stream(&mut control).await?;
        let (cmd_code, _) = send_ftp_cmd(&mut control, "MLSD").await?;
        let used_mlsd = if cmd_code == 150 || cmd_code == 125 {
            true
        } else {
            let (list_code, list_txt) = send_ftp_cmd(&mut control, "LIST -a").await?;
            if list_code != 150 && list_code != 125 {
                let (list2_code, list2_txt) = send_ftp_cmd(&mut control, "LIST").await?;
                if list2_code != 150 && list2_code != 125 {
                    return Err(anyhow!(
                        "FTP LIST failed: {} / {}",
                        list_txt.trim(),
                        list2_txt.trim()
                    ));
                }
            }
            false
        };

        let mut raw_listing = Vec::new();
        data_stream.read_to_end(&mut raw_listing).await?;
        drop(data_stream);
        let _ = read_ftp_response(&mut control).await;
        let _ = send_ftp_cmd(&mut control, "QUIT").await;

        let text = String::from_utf8_lossy(&raw_listing);
        let mut entries = Vec::new();
        for line in text.lines() {
            let parsed = if used_mlsd {
                parse_mlsd_line(&norm_path, line).or_else(|| parse_list_line(&norm_path, line))
            } else {
                parse_list_line(&norm_path, line).or_else(|| parse_mlsd_line(&norm_path, line))
            };
            if let Some(entry) = parsed {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    async fn create_dir(&self, path: &str) -> anyhow::Result<()> {
        let mut control = self.connect_control().await?;
        let norm = normalize_remote_path(path);
        let (code, txt) = send_ftp_cmd(&mut control, &format!("MKD {norm}")).await?;
        let _ = send_ftp_cmd(&mut control, "QUIT").await;
        if code >= 400 {
            return Err(anyhow!("FTP MKD {norm} failed: {}", txt.trim()));
        }
        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> anyhow::Result<()> {
        let mut control = self.connect_control().await?;
        let src = normalize_remote_path(from);
        let dst = normalize_remote_path(to);
        let (c1, t1) = send_ftp_cmd(&mut control, &format!("RNFR {src}")).await?;
        if c1 != 350 {
            let _ = send_ftp_cmd(&mut control, "QUIT").await;
            return Err(anyhow!("FTP RNFR {src} failed: {}", t1.trim()));
        }
        let (c2, t2) = send_ftp_cmd(&mut control, &format!("RNTO {dst}")).await?;
        let _ = send_ftp_cmd(&mut control, "QUIT").await;
        if c2 >= 400 {
            return Err(anyhow!("FTP RNTO {dst} failed: {}", t2.trim()));
        }
        Ok(())
    }

    async fn remove(&self, path: &str, is_dir: bool) -> anyhow::Result<()> {
        let norm = normalize_remote_path(path);
        if is_dir {
            let mut stack = vec![(norm, false)];
            while let Some((cur_dir, listed)) = stack.pop() {
                if !listed {
                    stack.push((cur_dir.clone(), true));
                    let children = self.list_dir(&cur_dir).await?;
                    for child in children {
                        if child.kind.is_dir_like() {
                            stack.push((child.path, false));
                        } else {
                            self.remove(&child.path, false).await?;
                        }
                    }
                } else {
                    let mut control = self.connect_control().await?;
                    let (code, txt) = send_ftp_cmd(&mut control, &format!("RMD {cur_dir}")).await?;
                    let _ = send_ftp_cmd(&mut control, "QUIT").await;
                    if code >= 400 {
                        return Err(anyhow!("FTP RMD {cur_dir} failed: {}", txt.trim()));
                    }
                }
            }
            Ok(())
        } else {
            let mut control = self.connect_control().await?;
            let (code, txt) = send_ftp_cmd(&mut control, &format!("DELE {norm}")).await?;
            let _ = send_ftp_cmd(&mut control, "QUIT").await;
            if code >= 400 {
                return Err(anyhow!("FTP DELE {norm} failed: {}", txt.trim()));
            }
            Ok(())
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
        let mut control = self.connect_control().await?;
        let norm = normalize_remote_path(remote_path);
        let mut data_stream = self.open_passive_data_stream(&mut control).await?;
        let (code, txt) = send_ftp_cmd(&mut control, &format!("RETR {norm}")).await?;
        if code != 150 && code != 125 {
            return Err(anyhow!("FTP RETR {norm} failed: {}", txt.trim()));
        }

        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(anyhow!("Transfer cancelled"));
            }
            let n = data_stream.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n]).await?;
            total += n as u64;
            progress(total);
        }
        drop(data_stream);
        writer.flush().await?;
        let _ = read_ftp_response(&mut control).await;
        let _ = send_ftp_cmd(&mut control, "QUIT").await;
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
        let mut control = self.connect_control().await?;
        let norm = normalize_remote_path(remote_path);
        let mut data_stream = self.open_passive_data_stream(&mut control).await?;
        let (code, txt) = send_ftp_cmd(&mut control, &format!("STOR {norm}")).await?;
        if code != 150 && code != 125 {
            return Err(anyhow!("FTP STOR {norm} failed: {}", txt.trim()));
        }

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
            data_stream.write_all(&buf[..n]).await?;
            total += n as u64;
            progress(total);
        }
        data_stream.flush().await?;
        data_stream.shutdown().await?;
        drop(data_stream);
        let (done_code, done_txt) = read_ftp_response(&mut control).await?;
        let _ = send_ftp_cmd(&mut control, "QUIT").await;
        if done_code >= 400 {
            return Err(anyhow!("FTP STOR completion failed: {}", done_txt.trim()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pasv_and_epsv_and_pwd() {
        assert_eq!(
            parse_epsv_port("229 Entering Extended Passive Mode (|||21210|)"),
            Some(21210)
        );
        assert_eq!(
            parse_pasv_port("227 Entering Passive Mode (192,168,1,50,195,149)."),
            Some(195 * 256 + 149)
        );
        assert_eq!(
            parse_pwd_path("257 \"/pub/releases\" is the current directory"),
            Some("/pub/releases".to_string())
        );
    }

    #[test]
    fn test_parse_mlsd_and_unix_and_dos_list_lines() {
        let mlsd_dir =
            parse_mlsd_line("/pub", "type=dir;modify=20261110120000;unix.mode=0755; assets")
                .unwrap();
        assert_eq!(mlsd_dir.name, "assets");
        assert_eq!(mlsd_dir.path, "/pub/assets");
        assert_eq!(mlsd_dir.kind, FileKind::Directory);

        let mlsd_file =
            parse_mlsd_line("/pub", "type=file;size=4096;modify=20261110120000; readme.md")
                .unwrap();
        assert_eq!(mlsd_file.name, "readme.md");
        assert_eq!(mlsd_file.size, 4096);
        assert_eq!(mlsd_file.kind, FileKind::File);

        let unix_file = parse_list_line(
            "/var",
            "-rw-r--r--    1 1000     1000        12345 Nov 10 15:30 archive.tar.gz",
        )
        .unwrap();
        assert_eq!(unix_file.name, "archive.tar.gz");
        assert_eq!(unix_file.path, "/var/archive.tar.gz");
        assert_eq!(unix_file.size, 12345);
        assert_eq!(unix_file.kind, FileKind::File);

        let dos_dir =
            parse_list_line("/", "11-10-26  12:30PM       <DIR>          Documents").unwrap();
        assert_eq!(dos_dir.name, "Documents");
        assert_eq!(dos_dir.kind, FileKind::Directory);
    }
}
