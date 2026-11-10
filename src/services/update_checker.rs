use chrono::Utc;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const GITHUB_REPO_OWNER: &str = "CyberTianzun";
pub const GITHUB_REPO_NAME: &str = "Rustduck";
pub const GITHUB_RELEASES_PAGE_URL: &str = "https://github.com/CyberTianzun/Rustduck/releases";
pub const GITHUB_LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/CyberTianzun/Rustduck/releases/latest";
pub const GITHUB_LATEST_RELEASE_REDIRECT_URL: &str =
    "https://github.com/CyberTianzun/Rustduck/releases/latest";
pub const DAILY_UPDATE_CHECK_INTERVAL_SECS: i64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub version: String,
    pub tag_name: String,
    pub html_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateCheckState {
    #[default]
    Idle,
    Checking,
    UpToDate {
        checked_at_unix: i64,
    },
    UpdateAvailable(ReleaseInfo),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub prerelease: Option<String>,
}

impl SemVer {
    pub fn to_version_string(&self) -> String {
        match &self.prerelease {
            Some(pre) if !pre.is_empty() => {
                format!("{}.{}.{}-{pre}", self.major, self.minor, self.patch)
            }
            _ => format!("{}.{}.{}", self.major, self.minor, self.patch),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GithubReleasePayload {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

pub fn parse_semver_tag(tag: &str) -> Option<SemVer> {
    let trimmed = tag.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_release = if trimmed.len() >= 8 && trimmed[..8].eq_ignore_ascii_case("release-") {
        &trimmed[8..]
    } else {
        trimmed
    };

    let without_v = without_release
        .strip_prefix('v')
        .or_else(|| without_release.strip_prefix('V'))
        .unwrap_or(without_release);

    let without_build = without_v.split('+').next().unwrap_or(without_v);
    let (core, prerelease) = match without_build.split_once('-') {
        Some((c, pre)) => {
            if pre.trim().is_empty() {
                return None;
            }
            (c, Some(pre.trim().to_string()))
        }
        None => (without_build, None),
    };

    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }

    Some(SemVer {
        major,
        minor,
        patch,
        prerelease,
    })
}

pub fn is_remote_version_newer(current_version: &str, remote_tag: &str) -> bool {
    let Some(cur) = parse_semver_tag(current_version) else {
        return false;
    };
    let Some(rem) = parse_semver_tag(remote_tag) else {
        return false;
    };

    let cur_tuple = (cur.major, cur.minor, cur.patch);
    let rem_tuple = (rem.major, rem.minor, rem.patch);
    if rem_tuple != cur_tuple {
        return rem_tuple > cur_tuple;
    }

    match (&cur.prerelease, &rem.prerelease) {
        (None, None) => false,
        (None, Some(_)) => false,
        (Some(_), None) => true,
        (Some(cur_pre), Some(rem_pre)) => rem_pre > cur_pre,
    }
}

pub fn parse_github_release_json(json_str: &str) -> anyhow::Result<Option<ReleaseInfo>> {
    let payload: GithubReleasePayload = serde_json::from_str(json_str)?;
    if payload.draft || payload.prerelease {
        return Ok(None);
    }

    let Some(semver) = parse_semver_tag(&payload.tag_name) else {
        return Ok(None);
    };
    if semver.prerelease.is_some() {
        return Ok(None);
    }

    let html_url = if payload.html_url.trim().is_empty() {
        format!("{GITHUB_RELEASES_PAGE_URL}/tag/{}", payload.tag_name.trim())
    } else {
        payload.html_url.trim().to_string()
    };

    Ok(Some(ReleaseInfo {
        version: semver.to_version_string(),
        tag_name: payload.tag_name.trim().to_string(),
        html_url,
    }))
}

pub fn extract_release_from_redirect_url(final_url: &str) -> Option<ReleaseInfo> {
    let trimmed = final_url.trim();
    let (_, after_marker) = trimmed.split_once("/releases/tag/")?;
    let raw_tag = after_marker
        .split(['?', '#'])
        .next()
        .unwrap_or(after_marker)
        .trim_matches('/');
    if raw_tag.is_empty() {
        return None;
    }

    let semver = parse_semver_tag(raw_tag)?;
    if semver.prerelease.is_some() {
        return None;
    }

    Some(ReleaseInfo {
        version: semver.to_version_string(),
        tag_name: raw_tag.to_string(),
        html_url: trimmed.to_string(),
    })
}

pub fn evaluate_latest_release(
    current_version: &str,
    latest: Option<ReleaseInfo>,
    checked_at_unix: i64,
) -> UpdateCheckState {
    if let Some(info) = latest {
        if is_remote_version_newer(current_version, &info.tag_name) {
            return UpdateCheckState::UpdateAvailable(info);
        }
    }
    UpdateCheckState::UpToDate { checked_at_unix }
}

pub async fn check_github_latest_release(current_version: &str) -> anyhow::Result<UpdateCheckState> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(format!("Rustduck/{current_version}"))
        .build()?;

    let resp = client
        .get(GITHUB_LATEST_RELEASE_API_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;

    let now_unix = Utc::now().timestamp();
    let status = resp.status();

    if status.is_success() {
        let body = resp.text().await?;
        let latest = parse_github_release_json(&body)?;
        return Ok(evaluate_latest_release(current_version, latest, now_unix));
    }

    if status == StatusCode::NOT_FOUND {
        return Ok(UpdateCheckState::UpToDate {
            checked_at_unix: now_unix,
        });
    }

    if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
        let redirect_resp = client.get(GITHUB_LATEST_RELEASE_REDIRECT_URL).send().await?;
        if redirect_resp.status().is_success() || redirect_resp.status() == StatusCode::NOT_FOUND {
            let final_url = redirect_resp.url().as_str();
            let latest = extract_release_from_redirect_url(final_url);
            return Ok(evaluate_latest_release(current_version, latest, now_unix));
        }
    }

    anyhow::bail!("GitHub Releases HTTP {status}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_semver_tag_supports_release_prefix_v_prefix_and_prerelease() {
        assert_eq!(
            parse_semver_tag("release-v0.2.1"),
            Some(SemVer {
                major: 0,
                minor: 2,
                patch: 1,
                prerelease: None,
            })
        );
        assert_eq!(
            parse_semver_tag("v1.0.0"),
            Some(SemVer {
                major: 1,
                minor: 0,
                patch: 0,
                prerelease: None,
            })
        );
        assert_eq!(
            parse_semver_tag("0.1.0"),
            Some(SemVer {
                major: 0,
                minor: 1,
                patch: 0,
                prerelease: None,
            })
        );
        assert_eq!(
            parse_semver_tag("release-v1.2.3-rc.1+build42"),
            Some(SemVer {
                major: 1,
                minor: 2,
                patch: 3,
                prerelease: Some("rc.1".to_string()),
            })
        );
        assert_eq!(parse_semver_tag("invalid-tag"), None);
        assert_eq!(parse_semver_tag("v1.2"), None);
    }

    #[test]
    fn test_is_remote_version_newer_compares_semver_and_prerelease_accurately() {
        assert!(is_remote_version_newer("0.1.0", "release-v0.1.1"));
        assert!(is_remote_version_newer("0.1.0", "v0.2.0"));
        assert!(is_remote_version_newer("0.1.9", "1.0.0"));
        assert!(!is_remote_version_newer("0.1.0", "release-v0.1.0"));
        assert!(!is_remote_version_newer("0.2.0", "v0.1.9"));
        // Stable is newer than same-version prerelease, but older than next-version stable
        assert!(!is_remote_version_newer("0.2.0", "release-v0.2.0-rc.1"));
        assert!(is_remote_version_newer("0.2.0-rc.1", "release-v0.2.0"));
    }

    #[test]
    fn test_parse_github_release_json_handles_stable_prerelease_and_draft() {
        let stable_json = r#"{
            "tag_name": "release-v0.2.0",
            "html_url": "https://github.com/CyberTianzun/Rustduck/releases/tag/release-v0.2.0",
            "draft": false,
            "prerelease": false
        }"#;
        let info = parse_github_release_json(stable_json)
            .unwrap()
            .expect("should return ReleaseInfo for stable release");
        assert_eq!(info.version, "0.2.0");
        assert_eq!(info.tag_name, "release-v0.2.0");
        assert_eq!(
            info.html_url,
            "https://github.com/CyberTianzun/Rustduck/releases/tag/release-v0.2.0"
        );

        let prerelease_json = r#"{
            "tag_name": "release-v0.3.0-rc.1",
            "html_url": "https://github.com/CyberTianzun/Rustduck/releases/tag/release-v0.3.0-rc.1",
            "draft": false,
            "prerelease": true
        }"#;
        assert_eq!(parse_github_release_json(prerelease_json).unwrap(), None);

        let draft_json = r#"{
            "tag_name": "v0.3.0",
            "html_url": "https://github.com/CyberTianzun/Rustduck/releases/tag/v0.3.0",
            "draft": true,
            "prerelease": false
        }"#;
        assert_eq!(parse_github_release_json(draft_json).unwrap(), None);
    }

    #[test]
    fn test_extract_release_from_redirect_url_and_evaluate_state() {
        let redirected =
            "https://github.com/CyberTianzun/Rustduck/releases/tag/release-v0.2.5";
        let info = extract_release_from_redirect_url(redirected).expect("valid tag redirect");
        assert_eq!(info.version, "0.2.5");
        assert_eq!(info.tag_name, "release-v0.2.5");
        assert_eq!(info.html_url, redirected);

        assert_eq!(
            extract_release_from_redirect_url("https://github.com/CyberTianzun/Rustduck/releases"),
            None
        );

        let state_newer = evaluate_latest_release("0.1.0", Some(info.clone()), 1700000000);
        assert_eq!(state_newer, UpdateCheckState::UpdateAvailable(info.clone()));

        let state_same = evaluate_latest_release("0.2.5", Some(info), 1700000000);
        assert_eq!(
            state_same,
            UpdateCheckState::UpToDate {
                checked_at_unix: 1700000000
            }
        );

        let state_none = evaluate_latest_release("0.1.0", None, 1700000000);
        assert_eq!(
            state_none,
            UpdateCheckState::UpToDate {
                checked_at_unix: 1700000000
            }
        );
    }
}
