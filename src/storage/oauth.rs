use anyhow::{anyhow, Context};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::RngCore;
use reqwest::{Client, Proxy, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

pub fn resolve_oauth_credential_from_sources(
    configured: Option<&str>,
    runtime_env: Option<&str>,
    compile_time_env: Option<&str>,
) -> Option<String> {
    for candidate in [configured, runtime_env, compile_time_env] {
        if let Some(val) = candidate.map(str::trim).filter(|s| !s.is_empty()) {
            return Some(val.to_string());
        }
    }
    None
}

pub fn resolve_oauth_credential(
    configured: Option<&str>,
    env_var_name: &str,
    compile_time_env: Option<&str>,
) -> Option<String> {
    let runtime_val = std::env::var(env_var_name).ok();
    resolve_oauth_credential_from_sources(configured, runtime_val.as_deref(), compile_time_env)
}

pub fn normalize_proxy_url(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("socks5://")
        || lower.starts_with("socks5h://")
        || lower.starts_with("socks4://")
        || lower.starts_with("socks4a://")
    {
        Some(trimmed.to_string())
    } else {
        Some(format!("http://{trimmed}"))
    }
}

pub fn build_http_client(proxy_url: Option<&str>) -> anyhow::Result<Client> {
    let mut builder = Client::builder().connect_timeout(Duration::from_secs(20));
    if let Some(norm_proxy) = normalize_proxy_url(proxy_url) {
        let proxy = Proxy::all(&norm_proxy)
            .with_context(|| format!("Invalid proxy URL '{norm_proxy}'"))?;
        builder = builder.proxy(proxy);
    }
    builder.build().context("Failed to build HTTP client")
}

pub fn generate_pkce_pair() -> (String, String) {
    let mut random_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut random_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(random_bytes);
    let challenge_hash = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(challenge_hash);
    (verifier, challenge)
}

pub fn generate_oauth_state() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn build_authorize_url(
    auth_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    state: &str,
    code_challenge: &str,
    extra_params: &[(&str, &str)],
) -> anyhow::Result<String> {
    let mut url =
        Url::parse(auth_endpoint).with_context(|| format!("Invalid auth URL: {auth_endpoint}"))?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("client_id", client_id);
        pairs.append_pair("redirect_uri", redirect_uri);
        pairs.append_pair("response_type", "code");
        pairs.append_pair("scope", scope);
        pairs.append_pair("state", state);
        pairs.append_pair("code_challenge", code_challenge);
        pairs.append_pair("code_challenge_method", "S256");
        for (k, v) in extra_params {
            pairs.append_pair(k, v);
        }
    }
    Ok(url.to_string())
}

pub fn parse_oauth_callback_request(raw_http_request: &str) -> anyhow::Result<(String, Option<String>)> {
    let first_line = raw_http_request
        .lines()
        .next()
        .ok_or_else(|| anyhow!("Empty HTTP request on OAuth callback"))?;
    let mut parts = first_line.split_whitespace();
    let _method = parts.next().unwrap_or("GET");
    let path_and_query = parts
        .next()
        .ok_or_else(|| anyhow!("Malformed HTTP request line on OAuth callback"))?;

    let dummy_url = format!("http://localhost{path_and_query}");
    let parsed = Url::parse(&dummy_url)
        .with_context(|| format!("Invalid callback URL path: {path_and_query}"))?;

    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut error_desc = None;

    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.to_string()),
            "state" => state = Some(v.to_string()),
            "error" => error = Some(v.to_string()),
            "error_description" => error_desc = Some(v.to_string()),
            _ => {}
        }
    }

    if let Some(err) = error {
        let detail = error_desc.unwrap_or_default();
        if detail.is_empty() {
            return Err(anyhow!("OAuth authorization denied: {err}"));
        } else {
            return Err(anyhow!("OAuth authorization denied: {err} ({detail})"));
        }
    }

    let auth_code = code.ok_or_else(|| anyhow!("OAuth callback missing 'code' query parameter"))?;
    Ok((auth_code, state))
}

async fn handle_loopback_stream(
    mut stream: TcpStream,
    expected_state: &str,
) -> anyhow::Result<String> {
    let mut buf = vec![0u8; 8192];
    let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buf))
        .await
        .context("Timed out reading OAuth callback request")?
        .context("Failed to read OAuth callback socket")?;
    let req_str = String::from_utf8_lossy(&buf[..n]);

    match parse_oauth_callback_request(&req_str) {
        Ok((code, state_opt)) => {
            if let Some(st) = state_opt {
                if st != expected_state {
                    let body = "<html><body style='font-family:sans-serif;text-align:center;padding:40px;'><h2>OAuth State Mismatch</h2><p>Please return to Rustduck and try again.</p></body></html>";
                    let resp = format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.flush().await;
                    return Err(anyhow!("OAuth callback state mismatch"));
                }
            }
            let body = "<!DOCTYPE html><html><head><meta charset='utf-8'><title>Rustduck OAuth</title></head><body style='background:#111318;color:#f1f5f9;font-family:system-ui,-apple-system,sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;'><div style='background:#1f242d;border:1px solid #343b4a;border-radius:12px;padding:32px 48px;text-align:center;box-shadow:0 10px 30px rgba(0,0,0,0.4);'><h2 style='color:#10b981;margin-top:0;'>&#10004; 授权成功 / Authorization Complete</h2><p style='color:#cbd5e1;font-size:14px;'>已完成账号授权，您现在可以关闭此浏览器标签页并返回 <b>Rustduck</b>。</p><p style='color:#64748b;font-size:12px;margin-bottom:0;'>You may now close this window and return to Rustduck.</p></div></body></html>";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes()).await;
            let _ = stream.flush().await;
            Ok(code)
        }
        Err(e) => {
            let body = format!(
                "<html><body style='font-family:sans-serif;text-align:center;padding:40px;'><h2>Authorization Failed</h2><p>{e}</p></body></html>"
            );
            let resp = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes()).await;
            let _ = stream.flush().await;
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn authorize_via_browser_with_launcher<F>(
    client: &Client,
    auth_endpoint: &str,
    token_endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    scope: &str,
    extra_auth_params: &[(&str, &str)],
    cancel: CancellationToken,
    launch_browser: F,
) -> anyhow::Result<OAuthTokenResponse>
where
    F: FnOnce(&str) -> anyhow::Result<()>,
{
    if cancel.is_cancelled() {
        return Err(anyhow!("OAuth browser authorization cancelled"));
    }

    let v4_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("Failed to bind local OAuth callback listener")?;
    let port = v4_listener.local_addr()?.port();

    let redirect_uri = format!("http://127.0.0.1:{port}");
    let (verifier, challenge) = generate_pkce_pair();
    let state = generate_oauth_state();

    let auth_url = build_authorize_url(
        auth_endpoint,
        client_id,
        &redirect_uri,
        scope,
        &state,
        &challenge,
        extra_auth_params,
    )?;

    launch_browser(&auth_url)?;

    let wait_callback = async {
        loop {
            let stream = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(anyhow!("OAuth browser authorization cancelled"));
                }
                res = v4_listener.accept() => res?.0,
            };
            let handle_res = tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(anyhow!("OAuth browser authorization cancelled"));
                }
                r = handle_loopback_stream(stream, &state) => r,
            };
            match handle_res {
                Ok(code) => return Ok::<String, anyhow::Error>(code),
                Err(e) => {
                    // Ignore favicon.ico or unrelated requests without 'code' or 'error'
                    let msg = e.to_string();
                    if msg.contains("OAuth authorization denied")
                        || msg.contains("state mismatch")
                    {
                        return Err(e);
                    }
                }
            }
        }
    };

    let code = tokio::time::timeout(Duration::from_secs(180), wait_callback)
        .await
        .context("Timed out waiting for browser OAuth callback (180s)")??;

    tokio::select! {
        _ = cancel.cancelled() => Err(anyhow!("OAuth browser authorization cancelled")),
        res = exchange_code_for_token(
            client,
            token_endpoint,
            client_id,
            client_secret,
            &code,
            &redirect_uri,
            &verifier,
        ) => res,
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn authorize_via_browser_loopback(
    client: &Client,
    auth_endpoint: &str,
    token_endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    scope: &str,
    extra_auth_params: &[(&str, &str)],
    cancel: CancellationToken,
) -> anyhow::Result<OAuthTokenResponse> {
    authorize_via_browser_with_launcher(
        client,
        auth_endpoint,
        token_endpoint,
        client_id,
        client_secret,
        scope,
        extra_auth_params,
        cancel,
        |url| {
            open::that_detached(url)
                .with_context(|| format!("Failed to launch browser for OAuth URL: {url}"))
        },
    )
    .await
}

pub async fn exchange_code_for_token(
    client: &Client,
    token_endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> anyhow::Result<OAuthTokenResponse> {
    let mut params = vec![
        ("client_id", client_id),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
        ("code_verifier", code_verifier),
    ];
    if let Some(sec) = client_secret.map(str::trim).filter(|s| !s.is_empty()) {
        params.push(("client_secret", sec));
    }

    let resp = client
        .post(token_endpoint)
        .form(&params)
        .send()
        .await
        .with_context(|| format!("Failed to contact OAuth token endpoint {token_endpoint}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "OAuth token exchange failed (HTTP {}): {}",
            status.as_u16(),
            body
        ));
    }

    serde_json::from_str::<OAuthTokenResponse>(&body)
        .with_context(|| format!("Invalid OAuth token response JSON: {body}"))
}

pub async fn refresh_access_token(
    client: &Client,
    token_endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
) -> anyhow::Result<OAuthTokenResponse> {
    let mut params = vec![
        ("client_id", client_id),
        ("refresh_token", refresh_token.trim()),
        ("grant_type", "refresh_token"),
    ];
    if let Some(sec) = client_secret.map(str::trim).filter(|s| !s.is_empty()) {
        params.push(("client_secret", sec));
    }

    let resp = client
        .post(token_endpoint)
        .form(&params)
        .send()
        .await
        .with_context(|| format!("Failed to refresh OAuth token at {token_endpoint}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "OAuth token refresh failed (HTTP {}): {}",
            status.as_u16(),
            body
        ));
    }

    serde_json::from_str::<OAuthTokenResponse>(&body)
        .with_context(|| format!("Invalid OAuth token refresh JSON: {body}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_proxy_url_and_build_client() {
        assert_eq!(normalize_proxy_url(None), None);
        assert_eq!(normalize_proxy_url(Some("   ")), None);
        assert_eq!(
            normalize_proxy_url(Some("127.0.0.1:7890")),
            Some("http://127.0.0.1:7890".to_string())
        );
        assert_eq!(
            normalize_proxy_url(Some("socks5://127.0.0.1:1080")),
            Some("socks5://127.0.0.1:1080".to_string())
        );

        assert!(build_http_client(Some("http://127.0.0.1:7890")).is_ok());
        assert!(build_http_client(Some("socks5://127.0.0.1:1080")).is_ok());
    }

    #[test]
    fn test_pkce_and_oauth_callback_parsing() {
        let (verifier, challenge) = generate_pkce_pair();
        assert!(verifier.len() >= 43);
        assert!(!challenge.is_empty());
        assert_ne!(verifier, challenge);

        let req = "GET /?code=4/0AfJohXl_test_code&state=xyz123 HTTP/1.1\r\nHost: localhost:54321\r\n\r\n";
        let (code, state) = parse_oauth_callback_request(req).unwrap();
        assert_eq!(code, "4/0AfJohXl_test_code");
        assert_eq!(state.as_deref(), Some("xyz123"));

        let err_req = "GET /?error=access_denied&error_description=User+cancelled HTTP/1.1\r\n\r\n";
        let err = parse_oauth_callback_request(err_req).unwrap_err();
        assert!(err.to_string().contains("access_denied"));
    }

    #[test]
    fn test_resolve_oauth_credential_priority_and_empty_handling() {
        assert_eq!(
            resolve_oauth_credential_from_sources(None, None, None),
            None
        );
        assert_eq!(
            resolve_oauth_credential_from_sources(Some("   "), Some(""), Some("  ")),
            None
        );
        assert_eq!(
            resolve_oauth_credential_from_sources(
                Some("  user-cid  "),
                Some("env-cid"),
                Some("build-cid")
            ),
            Some("user-cid".to_string())
        );
        assert_eq!(
            resolve_oauth_credential_from_sources(
                Some(""),
                Some("  env-cid  "),
                Some("build-cid")
            ),
            Some("env-cid".to_string())
        );
        assert_eq!(
            resolve_oauth_credential_from_sources(None, None, Some("  build-cid ")),
            Some("build-cid".to_string())
        );
    }

    #[tokio::test]
    async fn test_loopback_oauth_flow_with_mock_token_server() {
        // Start a mock OAuth token server
        let token_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let token_port = token_listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = token_listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let json = r#"{"access_token":"mock_access_123","refresh_token":"mock_refresh_456","expires_in":3600}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    json.len(),
                    json
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            }
        });

        let client = build_http_client(None).unwrap();
        let token_endpoint = format!("http://127.0.0.1:{token_port}/token");
        let cancel = tokio_util::sync::CancellationToken::new();

        let token_res = authorize_via_browser_with_launcher(
            &client,
            "https://accounts.example.com/authorize",
            &token_endpoint,
            "test-client-id",
            Some("test-secret"),
            "drive",
            &[("access_type", "offline")],
            cancel,
            |auth_url| {
                let parsed = Url::parse(auth_url).unwrap();
                let mut redirect_uri = String::new();
                let mut state = String::new();
                for (k, v) in parsed.query_pairs() {
                    if k == "redirect_uri" {
                        redirect_uri = v.to_string();
                    } else if k == "state" {
                        state = v.to_string();
                    }
                }
                assert!(
                    redirect_uri.starts_with("http://127.0.0.1:"),
                    "Loopback redirect_uri should bind explicit IPv4 127.0.0.1, got: {redirect_uri}"
                );
                tokio::spawn(async move {
                    let callback_url = format!("{redirect_uri}/?code=MOCK_AUTH_CODE&state={state}");
                    let _ = Client::new().get(&callback_url).send().await;
                });
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(token_res.access_token, "mock_access_123");
        assert_eq!(token_res.refresh_token.as_deref(), Some("mock_refresh_456"));
    }

    #[tokio::test]
    async fn test_loopback_oauth_flow_cancels_immediately_without_browser_callback() {
        let client = build_http_client(None).unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        let cancel_trigger = cancel.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            cancel_trigger.cancel();
        });

        let started = std::time::Instant::now();
        let err = authorize_via_browser_with_launcher(
            &client,
            "https://accounts.example.com/authorize",
            "http://127.0.0.1:9/token",
            "test-client-id",
            Some("test-secret"),
            "drive",
            &[],
            cancel,
            |_auth_url| {
                // Simulate browser displaying "Access blocked: This app's request is invalid"
                // and never redirecting back to 127.0.0.1.
                Ok(())
            },
        )
        .await
        .unwrap_err();

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "Cancelled OAuth flow must abort immediately, took {:?}",
            started.elapsed()
        );
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("cancel") || msg.contains("取消"),
            "Unexpected cancel error: {err}"
        );
    }
}
