pub mod dual_pane_view;
pub mod file_pane;
pub mod new_tab_view;
pub mod settings_modal;
pub mod transfer_drawer;
pub mod workspace;

use crate::models::{FileEntry, FileKind, ProtocolKind};
use crate::services::preferences::ThemePreference;
use gpui::{rgb, App, AssetSource, Hsla, SharedString, Window, WindowAppearance};
use gpui_component::theme::{Theme, ThemeMode};
use gpui_kit_assets::IconName;
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

const BRAND_LOGO_PNG: &[u8] = include_bytes!("../../assets/icons/brand_logo.png");

/// Combined asset source providing all `gpui_kit_assets::AllAssets` icons plus Rustduck's
/// custom transparent Rusty Duck brand icon (`"icons/brand_logo.png"`).
pub struct RustduckAssets;

impl AssetSource for RustduckAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path == "icons/brand_logo.png" {
            return Ok(Some(Cow::Borrowed(BRAND_LOGO_PNG)));
        }
        gpui_kit_assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut items = gpui_kit_assets::AllAssets.list(path)?;
        if path.is_empty() || path == "icons" {
            items.push("icons/brand_logo.png".into());
        }
        Ok(items)
    }
}

static DARK_MODE_ACTIVE: AtomicBool = AtomicBool::new(true);

pub fn is_dark_theme() -> bool {
    DARK_MODE_ACTIVE.load(Ordering::Relaxed)
}

pub fn set_dark_theme(is_dark: bool) {
    DARK_MODE_ACTIVE.store(is_dark, Ordering::Relaxed);
}

pub fn apply_theme_preference(
    pref: ThemePreference,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    let sys_dark = window
        .as_ref()
        .map(|w| {
            matches!(
                w.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            )
        })
        .unwrap_or_else(|| {
            matches!(
                cx.window_appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            )
        });

    let is_dark = pref.resolve_is_dark(sys_dark);
    set_dark_theme(is_dark);

    let mode = if is_dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, window, cx);
    Theme::sync_scrollbar_appearance(cx);
}

/// Panic Transmit 5-inspired studio background (Dark Carbon / Crisp Light)
pub fn bg_canvas() -> Hsla {
    if is_dark_theme() {
        rgb(0x111318).into()
    } else {
        rgb(0xf8fafc).into()
    }
}

/// Slightly lighter/darker alternating table row background
pub fn bg_row_alt() -> Hsla {
    if is_dark_theme() {
        rgb(0x15181f).into()
    } else {
        rgb(0xf1f5f9).into()
    }
}

/// Hover row highlight
pub fn bg_row_hover() -> Hsla {
    if is_dark_theme() {
        rgb(0x1e232d).into()
    } else {
        rgb(0xe2e8f0).into()
    }
}

/// Selected row fill (Transmit cobalt blue tint)
pub fn bg_row_selected() -> Hsla {
    if is_dark_theme() {
        rgb(0x172b4d).into()
    } else {
        rgb(0xdbeafe).into()
    }
}

/// Secondary chrome / sidebar / header surface
pub fn bg_panel() -> Hsla {
    if is_dark_theme() {
        rgb(0x171a21).into()
    } else {
        rgb(0xeef2f6).into()
    }
}

/// Elevated card / toolbar / inline bar surface
pub fn bg_elevated() -> Hsla {
    if is_dark_theme() {
        rgb(0x1f242d).into()
    } else {
        rgb(0xffffff).into()
    }
}

/// 1px subtle hairline divider
pub fn border_subtle() -> Hsla {
    if is_dark_theme() {
        rgb(0x262b36).into()
    } else {
        rgb(0xd8e0ea).into()
    }
}

/// Emphasized card/control border
pub fn border_strong() -> Hsla {
    if is_dark_theme() {
        rgb(0x343b4a).into()
    } else {
        rgb(0xb8c4d4).into()
    }
}

/// Active selection border
pub fn border_selected() -> Hsla {
    if is_dark_theme() {
        rgb(0x3b82f6).into()
    } else {
        rgb(0x2563eb).into()
    }
}

pub fn text_primary() -> Hsla {
    if is_dark_theme() {
        rgb(0xf1f5f9).into()
    } else {
        rgb(0x0f172a).into()
    }
}

pub fn text_secondary() -> Hsla {
    if is_dark_theme() {
        rgb(0xcbd5e1).into()
    } else {
        rgb(0x334155).into()
    }
}

pub fn text_muted() -> Hsla {
    if is_dark_theme() {
        rgb(0x64748b).into()
    } else {
        rgb(0x64748b).into()
    }
}

pub fn accent_blue() -> Hsla {
    if is_dark_theme() {
        rgb(0x38bdf8).into()
    } else {
        rgb(0x0284c7).into()
    }
}

pub fn accent_amber() -> Hsla {
    if is_dark_theme() {
        rgb(0xf59e0b).into()
    } else {
        rgb(0xd97706).into()
    }
}

pub fn accent_green() -> Hsla {
    if is_dark_theme() {
        rgb(0x10b981).into()
    } else {
        rgb(0x059669).into()
    }
}

pub fn accent_red() -> Hsla {
    if is_dark_theme() {
        rgb(0xef4444).into()
    } else {
        rgb(0xdc2626).into()
    }
}

pub fn accent_purple() -> Hsla {
    if is_dark_theme() {
        rgb(0xa78bfa).into()
    } else {
        rgb(0x7c3aed).into()
    }
}

/// Maps a `FileEntry` to a crisp Lucide vector icon and semantic Transmit color.
pub fn entry_icon_and_color(entry: &FileEntry) -> (IconName, Hsla) {
    match entry.kind {
        FileKind::Bucket => (IconName::Database, accent_amber()),
        FileKind::Directory => (IconName::Folder, accent_blue()),
        FileKind::File => {
            let lower = entry.name.to_lowercase();
            let ext = lower.rsplit('.').next().unwrap_or("");
            match ext {
                "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" | "xz" | "bz2" | "zst" => {
                    (IconName::FileArchive, accent_purple())
                }
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" => {
                    let pink = if is_dark_theme() {
                        rgb(0xf472b6).into()
                    } else {
                        rgb(0xdb2777).into()
                    };
                    (IconName::FileImage, pink)
                }
                "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "c" | "cpp" | "h" | "java"
                | "json" | "toml" | "yaml" | "yml" | "xml" | "html" | "css" | "sh" | "ps1"
                | "sql" => (IconName::FileCode, accent_green()),
                "md" | "txt" | "log" | "csv" | "pdf" | "doc" | "docx" => {
                    (IconName::FileText, text_secondary())
                }
                _ => (IconName::File, text_muted()),
            }
        }
    }
}

/// Maps a `ProtocolKind` to its signature icon and accent color.
pub fn protocol_icon_and_color(protocol: ProtocolKind) -> (IconName, Hsla) {
    match protocol {
        ProtocolKind::Sftp => (IconName::Terminal, accent_green()),
        ProtocolKind::S3 => (IconName::Cloud, accent_amber()),
        ProtocolKind::Smb => (IconName::HardDrive, accent_purple()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_light_and_dark_theme_palettes_switch_cleanly() {
        set_dark_theme(true);
        let dark_canvas = bg_canvas();
        let dark_text = text_primary();

        set_dark_theme(false);
        let light_canvas = bg_canvas();
        let light_text = text_primary();

        assert_ne!(dark_canvas, light_canvas);
        assert_ne!(dark_text, light_text);
        assert!(light_canvas.l > dark_canvas.l);
        assert!(dark_text.l > light_text.l);
        set_dark_theme(true);
    }

    #[test]
    fn test_rustduck_assets_loads_brand_logo_png() {
        let bytes = RustduckAssets
            .load("icons/brand_logo.png")
            .unwrap()
            .expect("brand_logo.png should exist");
        assert!(bytes.len() > 100);
        // PNG magic header
        assert_eq!(&bytes[0..4], &[0x89, b'P', b'N', b'G']);
    }
}
