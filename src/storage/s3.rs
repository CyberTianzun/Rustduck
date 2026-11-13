use crate::models::{
    join_remote_path, normalize_remote_path, FileEntry, FileKind, S3AddressingStyle, S3Config,
};
use crate::storage::{ProgressCallback, StorageBackend};
use anyhow::{anyhow, Context};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, HOST};
use reqwest::{Client, Method, Url};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

type HmacSha256 = Hmac<Sha256>;

/// RFC 3986 unreserved characters: ALPHA / DIGIT / "-" / "." / "_" / "~"
const S3_SEGMENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'!')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

pub fn encode_s3_key_path(key: &str) -> String {
    let trimmed = key.trim_start_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    let has_trailing_slash = trimmed.ends_with('/');
    let encoded_segments: Vec<String> = trimmed
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|seg| utf8_percent_encode(seg, S3_SEGMENT_ENCODE_SET).to_string())
        .collect();
    if encoded_segments.is_empty() {
        String::new()
    } else if has_trailing_slash {
        format!("{}/", encoded_segments.join("/"))
    } else {
        encoded_segments.join("/")
    }
}

pub fn build_s3_url(
    endpoint: &str,
    style: S3AddressingStyle,
    bucket: Option<&str>,
    key: &str,
) -> anyhow::Result<Url> {
    let raw = endpoint.trim();
    let with_scheme = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let mut url =
        Url::parse(&with_scheme).with_context(|| format!("Invalid S3 endpoint URL: {endpoint}"))?;

    let encoded_key = encode_s3_key_path(key);
    let clean_bucket = bucket.map(str::trim).filter(|b| !b.is_empty());

    match (style, clean_bucket) {
        (S3AddressingStyle::VirtualHosted, Some(bkt)) => {
            if let Some(host) = url.host_str().map(|s| s.to_string()) {
                let prefix = format!("{bkt}.");
                if !host.to_lowercase().starts_with(&prefix.to_lowercase()) {
                    url.set_host(Some(&format!("{bkt}.{host}")))
                        .map_err(|_| anyhow!("Invalid virtual-hosted S3 bucket host: {bkt}.{host}"))?;
                }
            }
            url.set_path(&format!("/{encoded_key}"));
        }
        (S3AddressingStyle::PathStyle, Some(bkt)) => {
            let encoded_bkt = utf8_percent_encode(bkt, S3_SEGMENT_ENCODE_SET).to_string();
            if encoded_key.is_empty() {
                url.set_path(&format!("/{encoded_bkt}"));
            } else {
                url.set_path(&format!("/{encoded_bkt}/{encoded_key}"));
            }
        }
        (_, None) => {
            url.set_path("/");
        }
    }

    Ok(url)
}

pub fn resolve_bucket_and_prefix(
    configured_bucket: Option<&str>,
    path: &str,
) -> (Option<String>, String) {
    let norm = normalize_remote_path(path);
    let configured = configured_bucket.map(str::trim).filter(|b| !b.is_empty());

    if let Some(bkt) = configured {
        if norm == "/" {
            (Some(bkt.to_string()), String::new())
        } else {
            let trimmed = norm.trim_matches('/');
            (Some(bkt.to_string()), format!("{trimmed}/"))
        }
    } else {
        if norm == "/" {
            (None, String::new())
        } else {
            let trimmed = norm.trim_matches('/');
            match trimmed.split_once('/') {
                Some((bkt, rest)) => {
                    let clean_rest = rest.trim_matches('/');
                    if clean_rest.is_empty() {
                        (Some(bkt.to_string()), String::new())
                    } else {
                        (Some(bkt.to_string()), format!("{clean_rest}/"))
                    }
                }
                None => (Some(trimmed.to_string()), String::new()),
            }
        }
    }
}

pub fn resolve_bucket_and_object_key(
    configured_bucket: Option<&str>,
    remote_path: &str,
) -> anyhow::Result<(String, String)> {
    let norm = normalize_remote_path(remote_path);
    let configured = configured_bucket.map(str::trim).filter(|b| !b.is_empty());

    if let Some(bkt) = configured {
        let key = norm.trim_start_matches('/').to_string();
        if key.is_empty() {
            return Err(anyhow!("S3 object key cannot be empty"));
        }
        Ok((bkt.to_string(), key))
    } else {
        let trimmed = norm.trim_start_matches('/');
        let (bkt, key) = trimmed
            .split_once('/')
            .ok_or_else(|| anyhow!("Path must include bucket and object key: {remote_path}"))?;
        if bkt.is_empty() || key.is_empty() {
            return Err(anyhow!("Invalid S3 bucket or object key in {remote_path}"));
        }
        Ok((bkt.to_string(), key.to_string()))
    }
}

pub fn parse_list_buckets_xml(xml: &str) -> anyhow::Result<Vec<FileEntry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_bucket = false;
    let mut current_tag = String::new();
    let mut bucket_name = String::new();
    let mut entries = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "Bucket" {
                    in_bucket = true;
                    bucket_name.clear();
                }
                current_tag = tag;
            }
            Ok(Event::Text(e)) if in_bucket => {
                let txt = e.unescape().unwrap_or_default().to_string();
                if current_tag == "Name" {
                    bucket_name = txt;
                }
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "Bucket" && in_bucket {
                    in_bucket = false;
                    if !bucket_name.is_empty() {
                        entries.push(FileEntry {
                            name: bucket_name.clone(),
                            path: format!("/{bucket_name}"),
                            kind: FileKind::Bucket,
                            size: 0,
                            modified_unix: None,
                            permissions: Some("Bucket".to_string()),
                        });
                    }
                }
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("Failed to parse S3 ListBuckets XML: {e}")),
            _ => {}
        }
        buf.clear();
    }
    Ok(entries)
}

pub fn parse_list_objects_xml(
    xml: &str,
    prefix: &str,
    ui_base_path: &str,
) -> anyhow::Result<Vec<FileEntry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_contents = false;
    let mut in_common_prefixes = false;
    let mut current_tag = String::new();

    let mut key = String::new();
    let mut size = 0u64;
    let mut last_modified: Option<i64> = None;
    let mut common_prefix = String::new();

    let mut entries = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "Contents" {
                    in_contents = true;
                    key.clear();
                    size = 0;
                    last_modified = None;
                } else if tag == "CommonPrefixes" {
                    in_common_prefixes = true;
                    common_prefix.clear();
                }
                current_tag = tag;
            }
            Ok(Event::Text(e)) => {
                let txt = e.unescape().unwrap_or_default().to_string();
                if in_contents {
                    match current_tag.as_str() {
                        "Key" => key = txt,
                        "Size" => size = txt.parse::<u64>().unwrap_or(0),
                        "LastModified" => {
                            last_modified = DateTime::parse_from_rfc3339(&txt)
                                .ok()
                                .map(|dt| dt.timestamp());
                        }
                        _ => {}
                    }
                } else if in_common_prefixes && current_tag == "Prefix" {
                    common_prefix = txt;
                }
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "Contents" && in_contents {
                    in_contents = false;
                    let rel = key.strip_prefix(prefix).unwrap_or(&key);
                    let clean_name = rel.trim_matches('/');
                    if !clean_name.is_empty() {
                        let is_dir = rel.ends_with('/');
                        entries.push(FileEntry {
                            name: clean_name.to_string(),
                            path: join_remote_path(ui_base_path, clean_name),
                            kind: if is_dir {
                                FileKind::Directory
                            } else {
                                FileKind::File
                            },
                            size: if is_dir { 0 } else { size },
                            modified_unix: last_modified,
                            permissions: None,
                        });
                    }
                } else if tag == "CommonPrefixes" && in_common_prefixes {
                    in_common_prefixes = false;
                    let rel = common_prefix.strip_prefix(prefix).unwrap_or(&common_prefix);
                    let clean_name = rel.trim_matches('/');
                    if !clean_name.is_empty() {
                        entries.push(FileEntry {
                            name: clean_name.to_string(),
                            path: join_remote_path(ui_base_path, clean_name),
                            kind: FileKind::Directory,
                            size: 0,
                            modified_unix: None,
                            permissions: Some("Prefix".to_string()),
                        });
                    }
                }
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("Failed to parse S3 ListObjectsV2 XML: {e}")),
            _ => {}
        }
        buf.clear();
    }
    Ok(entries)
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

pub fn infer_s3_region_from_endpoint(endpoint: &str) -> Option<String> {
    let raw = endpoint.trim();
    if raw.is_empty() {
        return None;
    }
    let with_scheme = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let url = Url::parse(&with_scheme).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();

    // Tencent Cloud COS: cos.<region>.myqcloud.com or <bucket>.cos.<region>.myqcloud.com
    if let Some(prefix) = host.strip_suffix(".myqcloud.com") {
        let parts: Vec<&str> = prefix.split('.').collect();
        if parts.len() >= 2 {
            let prev = parts[parts.len() - 2];
            let candidate = parts[parts.len() - 1];
            if (prev == "cos" || prev == "cos-internal")
                && !candidate.is_empty()
                && candidate != "accelerate"
            {
                return Some(candidate.to_string());
            }
        }
        return None;
    }

    // Alibaba Cloud OSS: oss-<region>.aliyuncs.com or <bucket>.oss-<region>[-internal].aliyuncs.com
    if let Some(prefix) = host.strip_suffix(".aliyuncs.com") {
        let last_label = prefix.rsplit('.').next()?;
        if let Some(rest) = last_label.strip_prefix("oss-") {
            let region = rest.strip_suffix("-internal").unwrap_or(rest);
            if !region.is_empty() && region != "accelerate" {
                return Some(region.to_string());
            }
        }
        return None;
    }

    // AWS S3: s3.<region>.amazonaws.com[.cn], <bucket>.s3.<region>.amazonaws.com[.cn], s3-<region>.amazonaws.com
    let aws_prefix = host
        .strip_suffix(".amazonaws.com.cn")
        .or_else(|| host.strip_suffix(".amazonaws.com"));
    if let Some(prefix) = aws_prefix {
        let parts: Vec<&str> = prefix.split('.').collect();
        if parts.len() >= 2 {
            let prev = parts[parts.len() - 2];
            let candidate = parts[parts.len() - 1];
            if (prev == "s3" || prev == "dualstack")
                && candidate.contains('-')
                && candidate != "accelerate"
            {
                return Some(candidate.to_string());
            }
        }
        if let Some(last_label) = parts.last() {
            if let Some(region) = last_label.strip_prefix("s3-") {
                if region.contains('-') && region != "accelerate" && region != "external-1" {
                    return Some(region.to_string());
                }
            }
        }
        return None;
    }

    None
}

pub fn resolve_effective_s3_region(config: &S3Config) -> String {
    if let Some(inferred) = infer_s3_region_from_endpoint(&config.endpoint) {
        return inferred;
    }
    if config.region.trim().is_empty() {
        "us-east-1".to_string()
    } else {
        config.region.trim().to_string()
    }
}

pub fn build_put_object_acl_headers(
    acl: crate::services::preferences::S3UploadAcl,
) -> Vec<(String, String)> {
    match acl.header_value() {
        Some(val) => vec![("x-amz-acl".to_string(), val.to_string())],
        None => Vec::new(),
    }
}

pub fn summarize_s3_error_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if trimmed.starts_with('<') {
        let mut reader = Reader::from_str(trimmed);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        let mut current_tag = String::new();
        let mut code = String::new();
        let mut message = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    current_tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                }
                Ok(Event::Text(e)) => {
                    let txt = e.unescape().unwrap_or_default().trim().to_string();
                    match current_tag.as_str() {
                        "Code" if code.is_empty() => code = txt,
                        "Message" if message.is_empty() => message = txt,
                        _ => {}
                    }
                }
                Ok(Event::End(_)) => {
                    current_tag.clear();
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }

        if !code.is_empty() && !message.is_empty() {
            return format!("{code}: {message}");
        } else if !code.is_empty() {
            return code;
        } else if !message.is_empty() {
            return message;
        }
    }

    let collapsed = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 220 {
        let mut short: String = collapsed.chars().take(220).collect();
        short.push('…');
        short
    } else {
        collapsed
    }
}

pub struct S3Backend {
    config: S3Config,
    secret_access_key: String,
    client: Client,
}

impl S3Backend {
    pub fn new(config: S3Config, secret_access_key: String) -> anyhow::Result<Self> {
        let t = crate::i18n::i18n();
        if config.endpoint.trim().is_empty() {
            return Err(anyhow!("{}", t.err_s3_endpoint_empty()));
        }
        if config.access_key_id.trim().is_empty() {
            return Err(anyhow!("{}", t.err_s3_access_key_empty()));
        }
        let clean_secret = secret_access_key.trim().to_string();
        if clean_secret.is_empty() {
            return Err(anyhow!("{}", t.err_s3_secret_key_empty()));
        }
        let client = Client::builder()
            .user_agent("Rustduck/0.1.0")
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .context("Failed to initialize S3 HTTP client")?;
        Ok(Self {
            config,
            secret_access_key: clean_secret,
            client,
        })
    }

    fn sign_request(
        &self,
        method: &Method,
        url: &Url,
        payload_hash: &str,
        extra_headers: &[(String, String)],
    ) -> anyhow::Result<HeaderMap> {
        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date_stamp = now.format("%Y%m%d").to_string();
        let effective_region = resolve_effective_s3_region(&self.config);
        let region = effective_region.as_str();

        let host_header = match url.port() {
            Some(port) => format!("{}:{}", url.host_str().unwrap_or_default(), port),
            None => url.host_str().unwrap_or_default().to_string(),
        };

        let mut canonical_headers_vec: Vec<(String, String)> = vec![
            ("host".to_string(), host_header.clone()),
            ("x-amz-content-sha256".to_string(), payload_hash.to_string()),
            ("x-amz-date".to_string(), amz_date.clone()),
        ];
        for (k, v) in extra_headers {
            canonical_headers_vec.push((k.to_lowercase(), v.trim().to_string()));
        }
        canonical_headers_vec.sort_by(|a, b| a.0.cmp(&b.0));

        let canonical_headers = canonical_headers_vec
            .iter()
            .map(|(k, v)| format!("{k}:{v}\n"))
            .collect::<String>();
        let signed_headers = canonical_headers_vec
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");

        let mut query_pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| {
                (
                    utf8_percent_encode(&k, S3_SEGMENT_ENCODE_SET).to_string(),
                    utf8_percent_encode(&v, S3_SEGMENT_ENCODE_SET).to_string(),
                )
            })
            .collect();
        query_pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let canonical_query = query_pairs
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");

        let canonical_uri = if url.path().is_empty() {
            "/"
        } else {
            url.path()
        };

        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method.as_str(),
            canonical_uri,
            canonical_query,
            canonical_headers,
            signed_headers,
            payload_hash
        );

        let credential_scope = format!("{date_stamp}/{region}/s3/aws4_request");
        let req_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
        let string_to_sign =
            format!("AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{req_hash}");

        let k_date = hmac_sha256(
            format!("AWS4{}", self.secret_access_key).as_bytes(),
            date_stamp.as_bytes(),
        );
        let k_region = hmac_sha256(&k_date, region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"s3");
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        let auth_header = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.config.access_key_id.trim(),
            credential_scope,
            signed_headers,
            signature
        );

        let mut headers = HeaderMap::new();
        headers.insert(HOST, HeaderValue::from_str(&host_header)?);
        headers.insert("x-amz-date", HeaderValue::from_str(&amz_date)?);
        headers.insert(
            "x-amz-content-sha256",
            HeaderValue::from_str(payload_hash)?,
        );
        headers.insert("authorization", HeaderValue::from_str(&auth_header)?);
        for (k, v) in extra_headers {
            headers.insert(
                HeaderName::from_bytes(k.as_bytes())?,
                HeaderValue::from_str(v)?,
            );
        }
        Ok(headers)
    }

    async fn delete_single_object(&self, path: &str, is_dir: bool) -> anyhow::Result<()> {
        let (bucket, mut key) =
            resolve_bucket_and_object_key(self.config.bucket.as_deref(), path)?;
        if is_dir && !key.ends_with('/') {
            key.push('/');
        }
        let url = build_s3_url(
            &self.config.endpoint,
            self.config.addressing_style,
            Some(&bucket),
            &key,
        )?;
        let empty_hash = hex::encode(Sha256::digest(b""));
        let headers = self.sign_request(&Method::DELETE, &url, &empty_hash, &[])?;
        let resp = self.client.delete(url).headers(headers).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "S3 DeleteObject failed ({status}): {}",
                summarize_s3_error_body(&body)
            ));
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl StorageBackend for S3Backend {
    fn protocol_name(&self) -> &'static str {
        "S3"
    }

    async fn connect_and_verify(&self) -> anyhow::Result<String> {
        let initial = normalize_remote_path(&self.config.initial_prefix);
        let _ = self.list_dir(&initial).await?;
        Ok(initial)
    }

    async fn list_dir(&self, path: &str) -> anyhow::Result<Vec<FileEntry>> {
        let ui_path = normalize_remote_path(path);
        let (bucket_opt, prefix) =
            resolve_bucket_and_prefix(self.config.bucket.as_deref(), &ui_path);

        match bucket_opt {
            None => {
                // ListBuckets at service root
                let url = build_s3_url(
                    &self.config.endpoint,
                    self.config.addressing_style,
                    None,
                    "",
                )?;
                let empty_hash = hex::encode(Sha256::digest(b""));
                let headers = self.sign_request(&Method::GET, &url, &empty_hash, &[])?;
                let resp = self
                    .client
                    .get(url)
                    .headers(headers)
                    .send()
                    .await
                    .context("S3 ListBuckets request failed")?;
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if !status.is_success() {
                    return Err(anyhow!(
                        "S3 ListBuckets failed ({status}): {}",
                        summarize_s3_error_body(&body)
                    ));
                }
                parse_list_buckets_xml(&body)
            }
            Some(bucket) => {
                let mut url = build_s3_url(
                    &self.config.endpoint,
                    self.config.addressing_style,
                    Some(&bucket),
                    "",
                )?;
                {
                    let mut qp = url.query_pairs_mut();
                    qp.append_pair("list-type", "2");
                    qp.append_pair("delimiter", "/");
                    if !prefix.is_empty() {
                        qp.append_pair("prefix", &prefix);
                    }
                }
                let empty_hash = hex::encode(Sha256::digest(b""));
                let headers = self.sign_request(&Method::GET, &url, &empty_hash, &[])?;
                let resp = self
                    .client
                    .get(url)
                    .headers(headers)
                    .send()
                    .await
                    .context("S3 ListObjectsV2 request failed")?;
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if !status.is_success() {
                    return Err(anyhow!(
                        "S3 ListObjectsV2 failed ({status}): {}",
                        summarize_s3_error_body(&body)
                    ));
                }
                parse_list_objects_xml(&body, &prefix, &ui_path)
            }
        }
    }

    async fn create_dir(&self, path: &str) -> anyhow::Result<()> {
        let (bucket, mut key) =
            resolve_bucket_and_object_key(self.config.bucket.as_deref(), path)?;
        if !key.ends_with('/') {
            key.push('/');
        }
        let url = build_s3_url(
            &self.config.endpoint,
            self.config.addressing_style,
            Some(&bucket),
            &key,
        )?;
        let empty_hash = hex::encode(Sha256::digest(b""));
        let headers = self.sign_request(&Method::PUT, &url, &empty_hash, &[])?;
        let resp = self
            .client
            .put(url)
            .headers(headers)
            .body(Vec::new())
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "S3 PutObject directory failed ({status}): {}",
                summarize_s3_error_body(&body)
            ));
        }
        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> anyhow::Result<()> {
        let (src_bucket, src_key) =
            resolve_bucket_and_object_key(self.config.bucket.as_deref(), from)?;
        let (dst_bucket, dst_key) =
            resolve_bucket_and_object_key(self.config.bucket.as_deref(), to)?;
        let url = build_s3_url(
            &self.config.endpoint,
            self.config.addressing_style,
            Some(&dst_bucket),
            &dst_key,
        )?;
        let copy_source = format!("/{}/{}", src_bucket, encode_s3_key_path(&src_key));
        let empty_hash = hex::encode(Sha256::digest(b""));
        let headers = self.sign_request(
            &Method::PUT,
            &url,
            &empty_hash,
            &[("x-amz-copy-source".to_string(), copy_source)],
        )?;
        let resp = self.client.put(url).headers(headers).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "S3 CopyObject failed ({status}): {}",
                summarize_s3_error_body(&body)
            ));
        }
        self.remove(from, false).await
    }

    async fn remove(&self, path: &str, is_dir: bool) -> anyhow::Result<()> {
        if is_dir {
            let mut stack = vec![(normalize_remote_path(path), false)];
            while let Some((cur_path, children_listed)) = stack.pop() {
                if !children_listed {
                    stack.push((cur_path.clone(), true));
                    let children = self.list_dir(&cur_path).await?;
                    for child in children {
                        if child.kind.is_dir_like() {
                            stack.push((child.path, false));
                        } else {
                            self.delete_single_object(&child.path, false).await?;
                        }
                    }
                } else {
                    self.delete_single_object(&cur_path, true).await?;
                }
            }
            Ok(())
        } else {
            self.delete_single_object(path, false).await
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
        let (bucket, key) =
            resolve_bucket_and_object_key(self.config.bucket.as_deref(), remote_path)?;
        let url = build_s3_url(
            &self.config.endpoint,
            self.config.addressing_style,
            Some(&bucket),
            &key,
        )?;
        let empty_hash = hex::encode(Sha256::digest(b""));
        let headers = self.sign_request(&Method::GET, &url, &empty_hash, &[])?;
        let resp = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(anyhow!("Transfer cancelled")),
            r = self.client.get(url).headers(headers).send() => r?,
        };
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "S3 GetObject failed ({status}): {}",
                summarize_s3_error_body(&body)
            ));
        }

        let mut stream = resp.bytes_stream();
        let mut total = 0u64;
        loop {
            let next_chunk = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(anyhow!("Transfer cancelled")),
                item = stream.next() => item,
            };
            let Some(chunk_res) = next_chunk else {
                break;
            };
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
        let (bucket, key) =
            resolve_bucket_and_object_key(self.config.bucket.as_deref(), remote_path)?;
        let url = build_s3_url(
            &self.config.endpoint,
            self.config.addressing_style,
            Some(&bucket),
            &key,
        )?;

        let extra_headers =
            build_put_object_acl_headers(crate::services::preferences::current_s3_upload_acl());
        let headers = self.sign_request(&Method::PUT, &url, "UNSIGNED-PAYLOAD", &extra_headers)?;
        let (mut tx, rx) = futures::channel::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(4);

        let mut req = self
            .client
            .put(url)
            .headers(headers)
            .body(reqwest::Body::wrap_stream(rx));
        if let Some(len) = total_size {
            req = req.header(reqwest::header::CONTENT_LENGTH, len);
        }

        let pump_cancel = cancel.clone();
        let pump_fut = async move {
            use futures::SinkExt;
            let mut buf = vec![0u8; 64 * 1024];
            let mut total = 0u64;
            loop {
                if pump_cancel.is_cancelled() {
                    return Err(anyhow!("Transfer cancelled"));
                }
                let n = reader.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                let chunk = buf[..n].to_vec();
                tx.send(Ok(chunk))
                    .await
                    .map_err(|_| anyhow!("S3 upload body stream closed early"))?;
                total += n as u64;
                progress(total);
            }
            tx.close().await.ok();
            Ok::<(), anyhow::Error>(())
        };

        let send_fut = async move {
            let resp = req.send().await?;
            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow!(
                    "S3 PutObject failed ({status}): {}",
                    summarize_s3_error_body(&body)
                ));
            }
            Ok::<(), anyhow::Error>(())
        };

        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(anyhow!("Transfer cancelled")),
            res = async { tokio::try_join!(pump_fut, send_fut) } => {
                res.map(|_| ())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::S3AddressingStyle;

    #[test]
    fn test_virtual_hosted_avoids_duplicate_bucket_prefix_and_encodes_cjk() {
        // User entered bucket-qualified OSS endpoint AND bucket name
        let url = build_s3_url(
            "https://my-bucket.oss-cn-hangzhou.aliyuncs.com",
            S3AddressingStyle::VirtualHosted,
            Some("my-bucket"),
            "设计稿 2026/v1.zip",
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://my-bucket.oss-cn-hangzhou.aliyuncs.com/%E8%AE%BE%E8%AE%A1%E7%A8%BF%202026/v1.zip"
        );

        // Standard regional endpoint with VirtualHosted
        let url2 = build_s3_url(
            "https://cos.ap-shanghai.myqcloud.com",
            S3AddressingStyle::VirtualHosted,
            Some("app-125000"),
            "releases/app.msi",
        )
        .unwrap();
        assert_eq!(
            url2.as_str(),
            "https://app-125000.cos.ap-shanghai.myqcloud.com/releases/app.msi"
        );

        // Path-style endpoint (MinIO / private S3)
        let url3 = build_s3_url(
            "http://127.0.0.1:9000",
            S3AddressingStyle::PathStyle,
            Some("dev-bucket"),
            "logs/today.txt",
        )
        .unwrap();
        assert_eq!(
            url3.as_str(),
            "http://127.0.0.1:9000/dev-bucket/logs/today.txt"
        );
    }

    #[test]
    fn test_single_bucket_vs_list_buckets_root_resolution() {
        // Configured single bucket: "/" maps straight into that bucket with empty prefix
        assert_eq!(
            resolve_bucket_and_prefix(Some("prod-oss"), "/"),
            (Some("prod-oss".to_string()), "".to_string())
        );
        assert_eq!(
            resolve_bucket_and_prefix(Some("prod-oss"), "/assets/img"),
            (Some("prod-oss".to_string()), "assets/img/".to_string())
        );

        // No configured bucket: "/" has None bucket (triggers ListBuckets), "/my-bkt/dir" extracts bucket "my-bkt"
        assert_eq!(
            resolve_bucket_and_prefix(None, "/"),
            (None, "".to_string())
        );
        assert_eq!(
            resolve_bucket_and_prefix(None, "/my-bkt/dir"),
            (Some("my-bkt".to_string()), "dir/".to_string())
        );
    }

    #[test]
    fn test_parse_list_objects_v2_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <ListBucketResult>
            <Prefix>releases/</Prefix>
            <CommonPrefixes><Prefix>releases/archive/</Prefix></CommonPrefixes>
            <Contents>
                <Key>releases/</Key>
                <Size>0</Size>
            </Contents>
            <Contents>
                <Key>releases/rustduck.exe</Key>
                <LastModified>2026-11-05T10:00:00.000Z</LastModified>
                <Size>1048576</Size>
            </Contents>
        </ListBucketResult>"#;

        let entries = parse_list_objects_xml(xml, "releases/", "/releases").unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .any(|e| e.name == "archive" && e.kind == FileKind::Directory));
        assert!(entries
            .iter()
            .any(|e| e.name == "rustduck.exe" && e.size == 1048576));
    }

    #[tokio::test]
    async fn test_s3_upload_stream_uses_unsigned_payload_and_aborts_on_cancel() {
        use crate::models::{S3AddressingStyle, S3Config, S3Preset};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (header_tx, header_rx) = tokio::sync::oneshot::channel::<String>();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut req_buf = vec![0u8; 4096];
                let n = socket.read(&mut req_buf).await.unwrap_or(0);
                let req_str = String::from_utf8_lossy(&req_buf[..n]).to_string();
                let _ = header_tx.send(req_str);
                // Hold connection open for 2 seconds before replying 200 OK
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        });

        let cfg = S3Config {
            preset: S3Preset::Minio,
            endpoint: format!("http://{addr}"),
            region: "us-east-1".to_string(),
            access_key_id: "minioadmin".to_string(),
            bucket: Some("test-bkt".to_string()),
            initial_prefix: "/".to_string(),
            addressing_style: S3AddressingStyle::PathStyle,
        };
        let backend = S3Backend::new(cfg, "miniosecret".to_string()).unwrap();

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            cancel_clone.cancel();
        });

        let started = std::time::Instant::now();
        let mut cursor = std::io::Cursor::new(vec![b'x'; 1024]);
        let res = backend
            .upload_stream(
                &mut cursor,
                Some(1024),
                "/demo.bin",
                Arc::new(|_| {}),
                cancel,
            )
            .await;

        assert!(res.is_err(), "upload_stream must abort when cancelled during in-flight PUT");
        assert!(
            started.elapsed() < std::time::Duration::from_millis(600),
            "upload_stream did not abort promptly on cancel"
        );
        let received_headers = header_rx.await.unwrap().to_lowercase();
        assert!(
            received_headers.contains("x-amz-content-sha256: unsigned-payload"),
            "expected UNSIGNED-PAYLOAD header for streaming upload, got: {received_headers}"
        );
    }

    #[tokio::test]
    async fn test_s3_remove_directory_recursively_deletes_children() {
        use crate::models::{S3AddressingStyle, S3Config, S3Preset};
        use std::sync::{Arc, Mutex};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let requests_clone = requests.clone();

        tokio::spawn(async move {
            for _ in 0..3 {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let mut buf = vec![0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let first_line = req.lines().next().unwrap_or("").to_string();
                    requests_clone.lock().unwrap().push(first_line.clone());

                    if first_line.starts_with("GET ") {
                        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
                        <ListBucketResult>
                            <Prefix>docs/</Prefix>
                            <Contents>
                                <Key>docs/report.pdf</Key>
                                <Size>128</Size>
                            </Contents>
                        </ListBucketResult>"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                            xml.len(),
                            xml
                        );
                        let _ = socket.write_all(resp.as_bytes()).await;
                    } else {
                        let _ = socket
                            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                            .await;
                    }
                }
            }
        });

        let cfg = S3Config {
            preset: S3Preset::Minio,
            endpoint: format!("http://{addr}"),
            region: "us-east-1".to_string(),
            access_key_id: "minioadmin".to_string(),
            bucket: Some("test-bkt".to_string()),
            initial_prefix: "/".to_string(),
            addressing_style: S3AddressingStyle::PathStyle,
        };
        let backend = S3Backend::new(cfg, "miniosecret".to_string()).unwrap();
        backend.remove("/docs", true).await.unwrap();

        let recorded = requests.lock().unwrap().clone();
        assert!(
            recorded.iter().any(|r| r.starts_with("DELETE /test-bkt/docs/report.pdf ")),
            "expected child object DELETE request, got: {recorded:?}"
        );
        assert!(
            recorded.iter().any(|r| r.starts_with("DELETE /test-bkt/docs/ ")),
            "expected directory marker DELETE request, got: {recorded:?}"
        );
    }

    #[test]
    fn test_infer_and_resolve_s3_region_from_cloud_endpoints() {
        use crate::models::{S3AddressingStyle, S3Config, S3Preset};

        assert_eq!(
            infer_s3_region_from_endpoint("https://cos.na-siliconvalley.myqcloud.com").as_deref(),
            Some("na-siliconvalley")
        );
        assert_eq!(
            infer_s3_region_from_endpoint("https://app-125000.cos.ap-shanghai.myqcloud.com")
                .as_deref(),
            Some("ap-shanghai")
        );
        assert_eq!(
            infer_s3_region_from_endpoint("https://oss-cn-shenzhen.aliyuncs.com").as_deref(),
            Some("cn-shenzhen")
        );
        assert_eq!(
            infer_s3_region_from_endpoint("https://my-bkt.oss-cn-beijing-internal.aliyuncs.com")
                .as_deref(),
            Some("cn-beijing")
        );
        assert_eq!(
            infer_s3_region_from_endpoint("https://s3.us-west-2.amazonaws.com").as_deref(),
            Some("us-west-2")
        );
        assert_eq!(
            infer_s3_region_from_endpoint("https://my-bkt.s3.ap-northeast-1.amazonaws.com")
                .as_deref(),
            Some("ap-northeast-1")
        );
        assert_eq!(infer_s3_region_from_endpoint("http://127.0.0.1:9000"), None);

        // Saved bookmark where endpoint is COS Silicon Valley while region stayed at default cn-hangzhou
        let cos_cfg = S3Config {
            preset: S3Preset::AlibabaOss,
            endpoint: "https://cos.na-siliconvalley.myqcloud.com".to_string(),
            region: "cn-hangzhou".to_string(),
            access_key_id: "AKID_DEMO".to_string(),
            bucket: None,
            initial_prefix: "/".to_string(),
            addressing_style: S3AddressingStyle::VirtualHosted,
        };
        assert_eq!(resolve_effective_s3_region(&cos_cfg), "na-siliconvalley");

        // MinIO / custom endpoint preserves explicit region or defaults to us-east-1
        let minio_cfg = S3Config {
            preset: S3Preset::Minio,
            endpoint: "http://127.0.0.1:9000".to_string(),
            region: "".to_string(),
            access_key_id: "minioadmin".to_string(),
            bucket: None,
            initial_prefix: "/".to_string(),
            addressing_style: S3AddressingStyle::PathStyle,
        };
        assert_eq!(resolve_effective_s3_region(&minio_cfg), "us-east-1");
    }

    #[test]
    fn test_s3_backend_rejects_empty_credentials_upfront() {
        use crate::models::{S3AddressingStyle, S3Config, S3Preset};

        let base = S3Config {
            preset: S3Preset::TencentCos,
            endpoint: "https://cos.na-siliconvalley.myqcloud.com".to_string(),
            region: "na-siliconvalley".to_string(),
            access_key_id: "AKID_DEMO".to_string(),
            bucket: None,
            initial_prefix: "/".to_string(),
            addressing_style: S3AddressingStyle::VirtualHosted,
        };

        assert!(S3Backend::new(base.clone(), "".to_string()).is_err());
        assert!(S3Backend::new(base.clone(), "   ".to_string()).is_err());

        let mut empty_ak = base.clone();
        empty_ak.access_key_id = "  ".to_string();
        assert!(S3Backend::new(empty_ak, "valid-secret".to_string()).is_err());

        let mut empty_ep = base;
        empty_ep.endpoint = "".to_string();
        assert!(S3Backend::new(empty_ep, "valid-secret".to_string()).is_err());
    }

    #[test]
    fn test_format_s3_error_extracts_xml_code_and_message() {
        let xml = "<?xml version='1.0' encoding='utf-8' ?>\n<Error>\n\t<Code>SignatureDoesNotMatch</Code>\n\t<Message>The Signature you specified is invalid.</Message>\n\t<Resource>/</Resource>\n</Error>";
        let summary = summarize_s3_error_body(xml);
        assert_eq!(
            summary,
            "SignatureDoesNotMatch: The Signature you specified is invalid."
        );
        assert!(!summary.contains('\n'));
    }

    #[tokio::test]
    async fn test_s3_upload_stream_includes_signed_x_amz_acl_header_when_configured() {
        use crate::models::{S3AddressingStyle, S3Config, S3Preset};
        use crate::services::preferences::{set_current_s3_upload_acl, S3UploadAcl};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        assert!(build_put_object_acl_headers(S3UploadAcl::BucketDefault).is_empty());
        assert_eq!(
            build_put_object_acl_headers(S3UploadAcl::PublicRead),
            vec![("x-amz-acl".to_string(), "public-read".to_string())]
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (header_tx, header_rx) = tokio::sync::oneshot::channel::<String>();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut req_buf = vec![0u8; 4096];
                let n = socket.read(&mut req_buf).await.unwrap_or(0);
                let req_str = String::from_utf8_lossy(&req_buf[..n]).to_string();
                let _ = header_tx.send(req_str);
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        });

        set_current_s3_upload_acl(S3UploadAcl::PublicRead);
        let cfg = S3Config {
            preset: S3Preset::Minio,
            endpoint: format!("http://{addr}"),
            region: "us-east-1".to_string(),
            access_key_id: "minioadmin".to_string(),
            bucket: Some("test-bkt".to_string()),
            initial_prefix: "/".to_string(),
            addressing_style: S3AddressingStyle::PathStyle,
        };
        let backend = S3Backend::new(cfg, "miniosecret".to_string()).unwrap();

        let mut cursor = std::io::Cursor::new(b"hello-acl".to_vec());
        let res = backend
            .upload_stream(
                &mut cursor,
                Some(9),
                "/acl-demo.txt",
                Arc::new(|_| {}),
                CancellationToken::new(),
            )
            .await;
        set_current_s3_upload_acl(S3UploadAcl::BucketDefault);

        assert!(res.is_ok(), "upload_stream failed: {res:?}");
        let received_headers = header_rx.await.unwrap().to_lowercase();
        assert!(
            received_headers.contains("x-amz-acl: public-read"),
            "expected x-amz-acl: public-read header, got: {received_headers}"
        );
        assert!(
            received_headers.contains("signedheaders=host;x-amz-acl;x-amz-content-sha256;x-amz-date"),
            "expected x-amz-acl in SignedHeaders, got: {received_headers}"
        );
    }
}
