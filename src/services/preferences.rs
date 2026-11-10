use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

static ACTIVE_S3_UPLOAD_ACL: AtomicU8 = AtomicU8::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum S3UploadAcl {
    #[default]
    BucketDefault,
    Private,
    PublicRead,
    PublicReadWrite,
    AuthenticatedRead,
    BucketOwnerRead,
    BucketOwnerFullControl,
}

impl S3UploadAcl {
    pub const ALL: [S3UploadAcl; 7] = [
        Self::BucketDefault,
        Self::Private,
        Self::PublicRead,
        Self::PublicReadWrite,
        Self::AuthenticatedRead,
        Self::BucketOwnerRead,
        Self::BucketOwnerFullControl,
    ];

    pub fn header_value(self) -> Option<&'static str> {
        match self {
            Self::BucketDefault => None,
            Self::Private => Some("private"),
            Self::PublicRead => Some("public-read"),
            Self::PublicReadWrite => Some("public-read-write"),
            Self::AuthenticatedRead => Some("authenticated-read"),
            Self::BucketOwnerRead => Some("bucket-owner-read"),
            Self::BucketOwnerFullControl => Some("bucket-owner-full-control"),
        }
    }
}

pub fn current_s3_upload_acl() -> S3UploadAcl {
    match ACTIVE_S3_UPLOAD_ACL.load(Ordering::Relaxed) {
        1 => S3UploadAcl::Private,
        2 => S3UploadAcl::PublicRead,
        3 => S3UploadAcl::PublicReadWrite,
        4 => S3UploadAcl::AuthenticatedRead,
        5 => S3UploadAcl::BucketOwnerRead,
        6 => S3UploadAcl::BucketOwnerFullControl,
        _ => S3UploadAcl::BucketDefault,
    }
}

pub fn set_current_s3_upload_acl(acl: S3UploadAcl) {
    let code = match acl {
        S3UploadAcl::BucketDefault => 0,
        S3UploadAcl::Private => 1,
        S3UploadAcl::PublicRead => 2,
        S3UploadAcl::PublicReadWrite => 3,
        S3UploadAcl::AuthenticatedRead => 4,
        S3UploadAcl::BucketOwnerRead => 5,
        S3UploadAcl::BucketOwnerFullControl => 6,
    };
    ACTIVE_S3_UPLOAD_ACL.store(code, Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LocalStartupDirMode {
    #[default]
    RememberLast,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppLanguage {
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en-US")]
    EnUs,
}

impl AppLanguage {
    /// Parses a locale tag (e.g. `"zh-CN"`, `"zh_Hans_CN.UTF-8"`, `"en-US"`) into `AppLanguage`.
    pub fn from_locale_tag(tag: &str) -> Self {
        let lower = tag.trim().to_ascii_lowercase();
        if lower.starts_with("zh") {
            Self::ZhCn
        } else {
            Self::EnUs
        }
    }

    /// Detects the operating system UI language on first launch.
    pub fn detect_system_language() -> Self {
        for var in ["RUSTDUCK_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(val) = std::env::var(var) {
                let trimmed = val.trim();
                if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("c") && !trimmed.eq_ignore_ascii_case("posix") {
                    return Self::from_locale_tag(trimmed);
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn GetUserDefaultUILanguage() -> u16;
                fn GetSystemDefaultUILanguage() -> u16;
                fn GetUserDefaultLCID() -> u32;
                fn GetSystemDefaultLCID() -> u32;
            }
            let ids = unsafe {
                [
                    GetUserDefaultUILanguage(),
                    GetSystemDefaultUILanguage(),
                    (GetUserDefaultLCID() & 0xFFFF) as u16,
                    (GetSystemDefaultLCID() & 0xFFFF) as u16,
                ]
            };
            // Primary language ID mask is 0x03FF; 0x04 is LANG_CHINESE
            if ids.into_iter().any(|id| (id & 0x03FF) == 0x04) {
                return Self::ZhCn;
            }
            return Self::EnUs;
        }

        #[cfg(not(target_os = "windows"))]
        {
            Self::EnUs
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::ZhCn => Self::EnUs,
            Self::EnUs => Self::ZhCn,
        }
    }

    pub fn gpui_locale(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::EnUs => "en",
        }
    }

    pub fn short_badge(self) -> &'static str {
        match self {
            Self::ZhCn => "中文",
            Self::EnUs => "EN",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::ZhCn => "简体中文",
            Self::EnUs => "English",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemePreference {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "light")]
    Light,
    #[serde(rename = "dark")]
    Dark,
}

impl ThemePreference {
    pub fn cycle_next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }

    pub fn resolve_is_dark(self, system_is_dark: bool) -> bool {
        match self {
            Self::System => system_is_dark,
            Self::Light => false,
            Self::Dark => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppPreferences {
    pub language: AppLanguage,
    pub theme: ThemePreference,
    #[serde(default)]
    pub last_update_check_unix: Option<i64>,
    #[serde(default)]
    pub s3_default_acl: S3UploadAcl,
    #[serde(default)]
    pub local_startup_mode: LocalStartupDirMode,
    #[serde(default)]
    pub custom_local_dir: String,
    #[serde(default)]
    pub last_local_dir: Option<String>,
}

impl AppPreferences {
    pub fn system_defaults() -> Self {
        Self {
            language: AppLanguage::detect_system_language(),
            theme: ThemePreference::System,
            last_update_check_unix: None,
            s3_default_acl: S3UploadAcl::BucketDefault,
            local_startup_mode: LocalStartupDirMode::RememberLast,
            custom_local_dir: crate::storage::local::LocalFileSystem::initial_local_dir(),
            last_local_dir: None,
        }
    }
}

pub struct PreferencesStore {
    path: PathBuf,
    prefs: AppPreferences,
}

impl PreferencesStore {
    pub fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rustduck")
            .join("preferences.json")
    }

    /// Loads preferences from disk if present; on first launch, initializes from system defaults
    /// (system language + system theme) and writes `preferences.json`.
    pub fn load_or_init(path: PathBuf) -> anyhow::Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read preferences file: {}", path.display()))?;
            if let Ok(mut prefs) = serde_json::from_str::<AppPreferences>(&content) {
                if prefs.custom_local_dir.trim().is_empty() {
                    prefs.custom_local_dir =
                        crate::storage::local::LocalFileSystem::initial_local_dir();
                }
                set_current_s3_upload_acl(prefs.s3_default_acl);
                return Ok(Self { path, prefs });
            }
        }

        let prefs = AppPreferences::system_defaults();
        set_current_s3_upload_acl(prefs.s3_default_acl);
        let store = Self { path, prefs };
        let _ = store.save();
        Ok(store)
    }

    pub fn preferences(&self) -> &AppPreferences {
        &self.prefs
    }

    pub fn set_language(&mut self, language: AppLanguage) -> anyhow::Result<()> {
        self.prefs.language = language;
        self.save()
    }

    pub fn set_theme(&mut self, theme: ThemePreference) -> anyhow::Result<()> {
        self.prefs.theme = theme;
        self.save()
    }

    pub fn should_run_daily_update_check(&self, now_unix: i64) -> bool {
        match self.prefs.last_update_check_unix {
            None => true,
            Some(last) => {
                now_unix.saturating_sub(last)
                    >= crate::services::update_checker::DAILY_UPDATE_CHECK_INTERVAL_SECS
            }
        }
    }

    pub fn set_last_update_check_unix(&mut self, timestamp_unix: i64) -> anyhow::Result<()> {
        self.prefs.last_update_check_unix = Some(timestamp_unix);
        self.save()
    }

    pub fn set_s3_default_acl(&mut self, acl: S3UploadAcl) -> anyhow::Result<()> {
        self.prefs.s3_default_acl = acl;
        set_current_s3_upload_acl(acl);
        self.save()
    }

    pub fn set_local_startup_mode(&mut self, mode: LocalStartupDirMode) -> anyhow::Result<()> {
        self.prefs.local_startup_mode = mode;
        self.save()
    }

    pub fn set_custom_local_dir(&mut self, dir: String) -> anyhow::Result<()> {
        self.prefs.custom_local_dir = dir.trim().to_string();
        self.save()
    }

    pub fn set_last_local_dir(&mut self, dir: String) -> anyhow::Result<()> {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            self.prefs.last_local_dir = Some(trimmed.to_string());
            self.save()?;
        }
        Ok(())
    }

    pub fn resolve_effective_local_dir(&self) -> String {
        let candidate = match self.prefs.local_startup_mode {
            LocalStartupDirMode::RememberLast => self.prefs.last_local_dir.as_deref(),
            LocalStartupDirMode::Custom => Some(self.prefs.custom_local_dir.as_str()),
        };
        resolve_valid_local_dir_or_fallback(candidate)
    }

    fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.prefs)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }
}

pub fn resolve_valid_local_dir_or_fallback(candidate: Option<&str>) -> String {
    if let Some(raw) = candidate {
        let trimmed = raw.trim();
        if !trimmed.is_empty()
            && std::fs::metadata(trimmed)
                .map(|m| m.is_dir())
                .unwrap_or(false)
        {
            return trimmed.to_string();
        }
    }
    crate::storage::local::LocalFileSystem::initial_local_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_tag_parsing_and_language_toggle() {
        assert_eq!(AppLanguage::from_locale_tag("zh-CN"), AppLanguage::ZhCn);
        assert_eq!(AppLanguage::from_locale_tag("zh_Hans_CN.UTF-8"), AppLanguage::ZhCn);
        assert_eq!(AppLanguage::from_locale_tag("ZH-TW"), AppLanguage::ZhCn);
        assert_eq!(AppLanguage::from_locale_tag("en-US"), AppLanguage::EnUs);
        assert_eq!(AppLanguage::from_locale_tag("en_GB.UTF-8"), AppLanguage::EnUs);
        assert_eq!(AppLanguage::from_locale_tag("ja-JP"), AppLanguage::EnUs);

        assert_eq!(AppLanguage::ZhCn.toggle(), AppLanguage::EnUs);
        assert_eq!(AppLanguage::EnUs.toggle(), AppLanguage::ZhCn);
        assert_eq!(AppLanguage::ZhCn.display_name(), "简体中文");
        assert_eq!(AppLanguage::EnUs.display_name(), "English");
    }

    #[test]
    fn test_theme_preference_cycling_and_resolution() {
        let t0 = ThemePreference::default();
        assert_eq!(t0, ThemePreference::System);
        assert!(t0.resolve_is_dark(true));
        assert!(!t0.resolve_is_dark(false));

        let t1 = t0.cycle_next();
        assert_eq!(t1, ThemePreference::Light);
        assert!(!t1.resolve_is_dark(true));
        assert!(!t1.resolve_is_dark(false));

        let t2 = t1.cycle_next();
        assert_eq!(t2, ThemePreference::Dark);
        assert!(t2.resolve_is_dark(true));
        assert!(t2.resolve_is_dark(false));

        assert_eq!(t2.cycle_next(), ThemePreference::System);
    }

    #[test]
    fn test_preferences_store_first_launch_and_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        assert!(!path.exists());

        let mut store = PreferencesStore::load_or_init(path.clone()).unwrap();
        assert!(path.exists());
        assert_eq!(store.preferences().theme, ThemePreference::System);

        store.set_language(AppLanguage::EnUs).unwrap();
        store.set_theme(ThemePreference::Light).unwrap();

        let reloaded = PreferencesStore::load_or_init(path).unwrap();
        assert_eq!(reloaded.preferences().language, AppLanguage::EnUs);
        assert_eq!(reloaded.preferences().theme, ThemePreference::Light);
    }

    #[test]
    fn test_daily_update_check_timestamp_persistence_and_legacy_json_compat() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        // Write legacy preferences.json without last_update_check_unix
        std::fs::write(&path, r#"{"language":"zh-CN","theme":"dark"}"#).unwrap();

        let mut store = PreferencesStore::load_or_init(path.clone()).unwrap();
        assert_eq!(store.preferences().last_update_check_unix, None);
        assert!(store.should_run_daily_update_check(1_700_000_000));

        store.set_last_update_check_unix(1_700_000_000).unwrap();
        assert_eq!(
            store.preferences().last_update_check_unix,
            Some(1_700_000_000)
        );
        assert!(!store.should_run_daily_update_check(1_700_000_000 + 3_600));
        assert!(!store.should_run_daily_update_check(1_700_000_000 + 86_399));
        assert!(store.should_run_daily_update_check(1_700_000_000 + 86_400));

        let reloaded = PreferencesStore::load_or_init(path).unwrap();
        assert_eq!(
            reloaded.preferences().last_update_check_unix,
            Some(1_700_000_000)
        );
    }

    #[test]
    fn test_s3_upload_acl_and_local_startup_dir_with_deleted_folder_fallback() {
        assert_eq!(S3UploadAcl::BucketDefault.header_value(), None);
        assert_eq!(S3UploadAcl::Private.header_value(), Some("private"));
        assert_eq!(S3UploadAcl::PublicRead.header_value(), Some("public-read"));
        assert_eq!(
            S3UploadAcl::PublicReadWrite.header_value(),
            Some("public-read-write")
        );
        assert_eq!(
            S3UploadAcl::AuthenticatedRead.header_value(),
            Some("authenticated-read")
        );
        assert_eq!(
            S3UploadAcl::BucketOwnerRead.header_value(),
            Some("bucket-owner-read")
        );
        assert_eq!(
            S3UploadAcl::BucketOwnerFullControl.header_value(),
            Some("bucket-owner-full-control")
        );

        let tmp = tempfile::tempdir().unwrap();
        let pref_path = tmp.path().join("preferences.json");
        let existing_dir = tmp.path().join("workspace_dir");
        std::fs::create_dir_all(&existing_dir).unwrap();
        let existing_dir_str = existing_dir.to_string_lossy().to_string();

        let mut store = PreferencesStore::load_or_init(pref_path.clone()).unwrap();
        assert_eq!(store.preferences().s3_default_acl, S3UploadAcl::BucketDefault);
        assert_eq!(
            store.preferences().local_startup_mode,
            LocalStartupDirMode::RememberLast
        );

        // RememberLast mode uses last_local_dir when it exists
        store.set_last_local_dir(existing_dir_str.clone()).unwrap();
        assert_eq!(store.resolve_effective_local_dir(), existing_dir_str);

        // If last_local_dir is deleted, it falls back safely to an existing directory
        std::fs::remove_dir_all(&existing_dir).unwrap();
        let fallback_resolved = store.resolve_effective_local_dir();
        assert_ne!(fallback_resolved, existing_dir_str);
        assert!(std::fs::metadata(&fallback_resolved)
            .map(|m| m.is_dir())
            .unwrap_or(false));

        // Custom mode uses custom_local_dir when valid, and falls back when deleted/missing
        let custom_dir = tmp.path().join("custom_downloads");
        std::fs::create_dir_all(&custom_dir).unwrap();
        let custom_dir_str = custom_dir.to_string_lossy().to_string();
        store
            .set_local_startup_mode(LocalStartupDirMode::Custom)
            .unwrap();
        store.set_custom_local_dir(custom_dir_str.clone()).unwrap();
        store.set_s3_default_acl(S3UploadAcl::PublicRead).unwrap();
        assert_eq!(store.resolve_effective_local_dir(), custom_dir_str);

        std::fs::remove_dir_all(&custom_dir).unwrap();
        let custom_fallback = store.resolve_effective_local_dir();
        assert_ne!(custom_fallback, custom_dir_str);
        assert!(std::fs::metadata(&custom_fallback)
            .map(|m| m.is_dir())
            .unwrap_or(false));

        let reloaded = PreferencesStore::load_or_init(pref_path).unwrap();
        assert_eq!(reloaded.preferences().s3_default_acl, S3UploadAcl::PublicRead);
        assert_eq!(
            reloaded.preferences().local_startup_mode,
            LocalStartupDirMode::Custom
        );
    }
}
