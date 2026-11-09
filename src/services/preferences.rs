use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
}

impl AppPreferences {
    pub fn system_defaults() -> Self {
        Self {
            language: AppLanguage::detect_system_language(),
            theme: ThemePreference::System,
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
            if let Ok(prefs) = serde_json::from_str::<AppPreferences>(&content) {
                return Ok(Self { path, prefs });
            }
        }

        let prefs = AppPreferences::system_defaults();
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

    fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.prefs)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }
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
}
