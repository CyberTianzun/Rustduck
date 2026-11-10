use crate::models::{join_remote_path, normalize_remote_path, FileEntry, FileKind, WebDavConfig};
use crate::storage::s3::encode_s3_key_path;
use crate::storage::{ProgressCallback, StorageBackend};
use anyhow::{anyhow, Context};
use chrono::DateTime;
use futures::StreamExt;
use percent_encoding::percent_decode_str;
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::{Client, Method, Url};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

pub fn build_webdav_url(endpoint: &str, remote_path: &str, is_dir: bool) -> anyhow::Result<Url> {
    let raw = endpoint.trim();
    if raw.is_empty() {
        return Err(anyhow!("WebDAV endpoint URL cannot be empty"));
    }
    let with_scheme = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let mut url =
        Url::parse(&with_scheme).with_context(|| format!("Invalid WebDAV endpoint URL: {raw}"))?;

    let base_path = url.path().trim_end_matches('/').to_string();
    let encoded_sub = encode_s3_key_path(remote_path);
    let combined = if encoded_sub.is_empty() {
        if base_path.is_empty() {
            "/".to_string()
        } else if is_dir {
            format!("{base_path}/")
        } else {
            base_path
        }
    } else if is_dir && !encoded_sub.ends_with('/') {
        format!("{base_path}/{encoded_sub}/")
    } else {
        format!("{base_path}/{encoded_sub}")
    };
    url.set_path(&combined);
    Ok(url)
}

fn local_xml_tag(raw: &[u8]) -> String {
    let full = String::from_utf8_lossy(raw);
    full.rsplit(':')
        .next()
        .unwrap_or(&full)
        .to_ascii_lowercase()
}

pub fn parse_propfind_xml(
    xml: &str,
    requested_remote_path: &str,
    endpoint_base_path: &str,
) -> anyhow::Result<Vec<FileEntry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let norm_req = normalize_remote_path(requested_remote_path);
    let clean_base = endpoint_base_path.trim_end_matches('/');

    let mut in_response = false;
    let mut in_resourcetype = false;
    let mut current_tag = String::new();

    let mut href = String::new();
    let mut display_name = String::new();
    let mut is_collection = false;
    let mut size = 0u64;
    let mut modified_unix = None;
    let mut entries = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = local_xml_tag(e.name().as_ref());
                match tag.as_str() {
                    "response" => {
                        in_response = true;
                        in_resourcetype = false;
                        href.clear();
                        display_name.clear();
                        is_collection = false;
                        size = 0;
                        modified_unix = None;
                    }
                    "resourcetype" if in_response => {
                        in_resourcetype = true;
                    }
                    "collection" if in_resourcetype => {
                        is_collection = true;
                    }
                    _ => {}
                }
                current_tag = tag;
            }
            Ok(Event::Empty(e)) => {
                let tag = local_xml_tag(e.name().as_ref());
                if in_resourcetype && tag == "collection" {
                    is_collection = true;
                }
            }
            Ok(Event::Text(e)) if in_response => {
                let txt = e.unescape().unwrap_or_default().to_string();
                match current_tag.as_str() {
                    "href" if href.is_empty() => {
                        href = txt;
                    }
                    "displayname" if display_name.is_empty() => {
                        display_name = txt;
                    }
                    "getcontentlength" => {
                        size = txt.trim().parse::<u64>().unwrap_or(0);
                    }
                    "getlastmodified" => {
                        if let Ok(dt) = DateTime::parse_from_rfc2822(txt.trim()) {
                            modified_unix = Some(dt.timestamp());
                        } else if let Ok(dt) = DateTime::parse_from_rfc3339(txt.trim()) {
                            modified_unix = Some(dt.timestamp());
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let tag = local_xml_tag(e.name().as_ref());
                if tag == "resourcetype" {
                    in_resourcetype = false;
                } else if tag == "response" && in_response {
                    in_response = false;
                    if let Some(entry) = finalize_webdav_response_item(
                        &href,
                        &display_name,
                        is_collection,
                        size,
                        modified_unix,
                        &norm_req,
                        clean_base,
                    ) {
                        entries.push(entry);
                    }
                }
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("Failed to parse WebDAV PROPFIND XML: {e}")),
            _ => {}
        }
        buf.clear();
    }

    Ok(entries)
}

fn finalize_webdav_response_item(
    raw_href: &str,
    _display_name: &str,
    is_collection: bool,
    size: u64,
    modified_unix: Option<i64>,
    norm_req: &str,
    clean_base: &str,
) -> Option<FileEntry> {
    let href_path = if raw_href.starts_with("http://") || raw_href.starts_with("https://") {
        Url::parse(raw_href)
            .ok()
            .map(|u| u.path().to_string())
            .unwrap_or_else(|| raw_href.to_string())
    } else {
        raw_href.to_string()
    };

    let decoded_path = percent_decode_str(&href_path).decode_utf8_lossy().to_string();
    let rel_to_base = if !clean_base.is_empty() && decoded_path.starts_with(clean_base) {
        &decoded_path[clean_base.len()..]
    } else {
        decoded_path.as_str()
    };

    let norm_item = normalize_remote_path(rel_to_base);
    if norm_item == norm_req {
        return None;
    }

    let trimmed = decoded_path.trim_end_matches('/');
    let name = trimmed.rsplit('/').next().unwrap_or("").trim();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }

    let kind = if is_collection || decoded_path.ends_with('/') {
        FileKind::Directory
    } else {
        FileKind::File
    };

    Some(FileEntry {
        name: name.to_string(),
        path: join_remote_path(norm_req, name),
        kind,
        size: if matches!(kind, FileKind::Directory) {
            0
        } else {
            size
        },
        modified_unix,
        permissions: None,
    })
}

pub struct WebDavBackend {
    config: WebDavConfig,
    password: String,
    client: Client,
}

impl WebDavBackend {
    pub fn new(config: WebDavConfig, password: String) -> anyhow::Result<Self> {
        let _ = build_webdav_url(&config.endpoint, "/", true)?;
        Ok(Self {
            config,
            password,
            client: Client::new(),
        })
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let user = self.config.username.trim();
        if user.is_empty() && self.password.is_empty() {
            req
        } else {
            req.basic_auth(user, Some(&self.password))
        }
    }
}

#[async_trait::async_trait]
impl StorageBackend for WebDavBackend {
    fn protocol_name(&self) -> &'static str {
        "WebDAV"
    }

    async fn connect_and_verify(&self) -> anyhow::Result<String> {
        let initial = normalize_remote_path(&self.config.initial_path);
        let url = build_webdav_url(&self.config.endpoint, &initial, true)?;
        let method = Method::from_bytes(b"PROPFIND")?;
        let resp = self
            .apply_auth(self.client.request(method, url))
            .header("Depth", "0")
            .send()
            .await
            .context("Failed to connect to WebDAV server")?;

        let status = resp.status();
        if !status.is_success() && status.as_u16() != 207 {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "WebDAV authentication/verify failed (HTTP {}): {}",
                status.as_u16(),
                body
            ));
        }
        Ok(initial)
    }

    async fn list_dir(&self, path: &str) -> anyhow::Result<Vec<FileEntry>> {
        let norm_path = normalize_remote_path(path);
        let url = build_webdav_url(&self.config.endpoint, &norm_path, true)?;
        let base_url = build_webdav_url(&self.config.endpoint, "/", false)?;
        let base_path = base_url.path().to_string();

        let method = Method::from_bytes(b"PROPFIND")?;
        let body_xml = r#"<?xml version="1.0" encoding="utf-8" ?><D:propfind xmlns:D="DAV:"><D:allprop/></D:propfind>"#;
        let resp = self
            .apply_auth(self.client.request(method, url))
            .header("Depth", "1")
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(body_xml)
            .send()
            .await
            .with_context(|| format!("WebDAV PROPFIND failed for {norm_path}"))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() && status.as_u16() != 207 {
            return Err(anyhow!(
                "WebDAV PROPFIND {norm_path} failed (HTTP {}): {}",
                status.as_u16(),
                text
            ));
        }

        parse_propfind_xml(&text, &norm_path, &base_path)
    }

    async fn create_dir(&self, path: &str) -> anyhow::Result<()> {
        let norm = normalize_remote_path(path);
        let url = build_webdav_url(&self.config.endpoint, &norm, true)?;
        let method = Method::from_bytes(b"MKCOL")?;
        let resp = self
            .apply_auth(self.client.request(method, url))
            .send()
            .await
            .with_context(|| format!("WebDAV MKCOL failed for {norm}"))?;
        let status = resp.status();
        if !status.is_success() && status.as_u16() != 405 {
            let txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "WebDAV MKCOL {norm} failed (HTTP {}): {}",
                status.as_u16(),
                txt
            ));
        }
        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> anyhow::Result<()> {
        let src = normalize_remote_path(from);
        let dst = normalize_remote_path(to);
        let src_url = build_webdav_url(&self.config.endpoint, &src, false)?;
        let dst_url = build_webdav_url(&self.config.endpoint, &dst, false)?;
        let method = Method::from_bytes(b"MOVE")?;
        let resp = self
            .apply_auth(self.client.request(method, src_url))
            .header("Destination", dst_url.as_str())
            .header("Overwrite", "T")
            .send()
            .await
            .with_context(|| format!("WebDAV MOVE failed from {src} to {dst}"))?;
        let status = resp.status();
        if !status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "WebDAV MOVE failed (HTTP {}): {}",
                status.as_u16(),
                txt
            ));
        }
        Ok(())
    }

    async fn remove(&self, path: &str, is_dir: bool) -> anyhow::Result<()> {
        let norm = normalize_remote_path(path);
        let url = build_webdav_url(&self.config.endpoint, &norm, is_dir)?;
        let resp = self
            .apply_auth(self.client.delete(url))
            .send()
            .await
            .with_context(|| format!("WebDAV DELETE failed for {norm}"))?;
        let status = resp.status();
        if !status.is_success() && status.as_u16() != 404 {
            let txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "WebDAV DELETE {norm} failed (HTTP {}): {}",
                status.as_u16(),
                txt
            ));
        }
        Ok(())
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
        let url = build_webdav_url(&self.config.endpoint, &norm, false)?;
        let resp = self
            .apply_auth(self.client.get(url))
            .send()
            .await
            .with_context(|| format!("WebDAV GET failed for {norm}"))?;
        let status = resp.status();
        if !status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "WebDAV GET {norm} failed (HTTP {}): {}",
                status.as_u16(),
                txt
            ));
        }

        let mut stream = resp.bytes_stream();
        let mut total = 0u64;
        while let Some(chunk_res) = stream.next().await {
            if cancel.is_cancelled() {
                return Err(anyhow!("Transfer cancelled"));
            }
            let chunk = chunk_res?;
            writer.write_all(&chunk).await?;
            total += chunk.len() as u64;
            progress(total);
        }
        writer.flush().await?;
        Ok(())
    }

    async fn upload_stream(
        &self,
        reader: &mut (dyn AsyncRead + Unpin + Send),
        total_size: Option<u64>,
        remote_path: &str,
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        if cancel.is_cancelled() {
            return Err(anyhow!("Transfer cancelled"));
        }
        let norm = normalize_remote_path(remote_path);
        let url = build_webdav_url(&self.config.endpoint, &norm, false)?;

        let mut payload = Vec::with_capacity(total_size.unwrap_or(0) as usize);
        let mut buf = vec![0u8; 64 * 1024];
        let mut read_total = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(anyhow!("Transfer cancelled"));
            }
            let n = reader.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            payload.extend_from_slice(&buf[..n]);
            read_total += n as u64;
            progress(read_total);
        }

        let resp = self
            .apply_auth(self.client.put(url))
            .body(payload)
            .send()
            .await
            .with_context(|| format!("WebDAV PUT failed for {norm}"))?;
        let status = resp.status();
        if !status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "WebDAV PUT {norm} failed (HTTP {}): {}",
                status.as_u16(),
                txt
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_webdav_url_and_parse_multistatus_propfind_xml() {
        let u = build_webdav_url(
            "https://dav.example.com/remote.php/webdav",
            "/docs/report 2026.pdf",
            false,
        )
        .unwrap();
        assert_eq!(
            u.as_str(),
            "https://dav.example.com/remote.php/webdav/docs/report%202026.pdf"
        );

        let sample_xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/remote.php/webdav/docs/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/></d:resourcetype>
      </d:prop>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/webdav/docs/subfolder/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>subfolder</d:displayname>
        <d:resourcetype><d:collection/></d:resourcetype>
      </d:prop>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/webdav/docs/notes%20final.txt</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>notes final.txt</d:displayname>
        <d:getcontentlength>2048</d:getcontentlength>
        <d:getlastmodified>Tue, 10 Nov 2026 08:30:00 GMT</d:getlastmodified>
        <d:resourcetype/>
      </d:prop>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        let items = parse_propfind_xml(sample_xml, "/docs", "/remote.php/webdav").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "subfolder");
        assert_eq!(items[0].path, "/docs/subfolder");
        assert_eq!(items[0].kind, FileKind::Directory);
        assert_eq!(items[1].name, "notes final.txt");
        assert_eq!(items[1].path, "/docs/notes final.txt");
        assert_eq!(items[1].kind, FileKind::File);
        assert_eq!(items[1].size, 2048);
        assert!(items[1].modified_unix.is_some());
    }
}
