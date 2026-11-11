use crate::i18n::{i18n, I18n};
use crate::models::{FileEntry, FileKind};
use chrono::{TimeZone, Utc};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Size,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneInlineAction {
    NewFolder,
    Rename {
        original_name: String,
        original_path: String,
    },
    ConfirmDelete {
        entry: FileEntry,
    },
    ConfirmBatchDelete {
        entries: Vec<FileEntry>,
    },
}

pub fn filter_and_sort_entries(
    entries: &[FileEntry],
    search_query: &str,
    show_hidden: bool,
    sort_col: SortColumn,
    ascending: bool,
) -> Vec<FileEntry> {
    let q = search_query.trim().to_lowercase();
    let mut filtered: Vec<FileEntry> = entries
        .iter()
        .filter(|e| {
            if !show_hidden && e.name.starts_with('.') {
                return false;
            }
            if !q.is_empty() && !e.name.to_lowercase().contains(&q) {
                return false;
            }
            true
        })
        .cloned()
        .collect();

    filtered.sort_by(|a, b| {
        let a_dir = a.kind.is_dir_like();
        let b_dir = b.kind.is_dir_like();
        if a_dir != b_dir {
            return if a_dir {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }

        let cmp = match sort_col {
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::Size => a.size.cmp(&b.size),
            SortColumn::Modified => a
                .modified_unix
                .unwrap_or(0)
                .cmp(&b.modified_unix.unwrap_or(0)),
        };

        if ascending {
            cmp
        } else {
            cmp.reverse()
        }
    });

    filtered
}

pub fn parent_local_or_remote(path: &str, is_local: bool) -> Option<String> {
    if is_local {
        let trimmed = path.trim();
        if trimmed.is_empty() || trimmed == "/" {
            return None;
        }
        let p = std::path::Path::new(trimmed);
        match p.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                let s = parent.to_string_lossy().to_string();
                if s == trimmed {
                    Some("/".to_string())
                } else {
                    Some(s)
                }
            }
            _ => Some("/".to_string()),
        }
    } else {
        crate::models::parent_remote_path(path)
    }
}

pub fn join_local_or_remote(base: &str, name: &str, is_local: bool) -> String {
    if is_local {
        if base == "/" {
            name.to_string()
        } else {
            std::path::Path::new(base)
                .join(name)
                .to_string_lossy()
                .to_string()
        }
    } else {
        crate::models::join_remote_path(base, name)
    }
}

pub fn resolve_rename_target_path(
    current_entry_path: &str,
    new_name: &str,
    is_local: bool,
) -> anyhow::Result<String> {
    resolve_rename_target_path_with_i18n(current_entry_path, new_name, is_local, i18n())
}

pub fn resolve_rename_target_path_with_i18n(
    current_entry_path: &str,
    new_name: &str,
    is_local: bool,
    t: I18n,
) -> anyhow::Result<String> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return Err(anyhow::anyhow!(t.err_rename_empty()));
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(anyhow::anyhow!(t.err_rename_invalid_sep()));
    }
    let parent =
        parent_local_or_remote(current_entry_path, is_local).unwrap_or_else(|| "/".to_string());
    Ok(join_local_or_remote(&parent, trimmed, is_local))
}

/// Splits a local or remote path into clickable `(label, target_path)` breadcrumb segments.
pub fn split_breadcrumb_segments(path: &str, is_local: bool) -> Vec<(String, String)> {
    split_breadcrumb_segments_with_i18n(path, is_local, i18n())
}

pub fn split_breadcrumb_segments_with_i18n(
    path: &str,
    is_local: bool,
    t: I18n,
) -> Vec<(String, String)> {
    let trimmed = path.trim();
    if is_local {
        let mut crumbs = vec![(t.breadcrumb_local_drives_root().to_string(), "/".to_string())];
        if trimmed.is_empty() || trimmed == "/" || trimmed == "\\" {
            return crumbs;
        }
        let normalized = trimmed.replace('/', r"\");
        let parts: Vec<&str> = normalized.split('\\').filter(|s| !s.is_empty()).collect();
        let mut accum = String::new();
        for (idx, part) in parts.into_iter().enumerate() {
            if idx == 0 && part.ends_with(':') {
                accum = format!(r"{part}\");
                crumbs.push((part.to_string(), accum.clone()));
            } else {
                if accum.ends_with('\\') {
                    accum.push_str(part);
                } else if accum.is_empty() {
                    accum = part.to_string();
                } else {
                    accum = format!(r"{accum}\{part}");
                }
                crumbs.push((part.to_string(), accum.clone()));
            }
        }
        crumbs
    } else {
        let mut crumbs = vec![(t.breadcrumb_remote_root().to_string(), "/".to_string())];
        let norm = crate::models::normalize_remote_path(trimmed);
        if norm == "/" {
            return crumbs;
        }
        let mut accum = String::new();
        for part in norm.split('/').filter(|s| !s.is_empty()) {
            accum = format!("{accum}/{part}");
            crumbs.push((part.to_string(), accum.clone()));
        }
        crumbs
    }
}

/// Formats an optional Unix timestamp into `YYYY-MM-DD HH:mm` (UTC).
pub fn format_modified_time(unix_secs: Option<i64>) -> String {
    let Some(secs) = unix_secs else {
        return "--".to_string();
    };
    match Utc.timestamp_opt(secs, 0).single() {
        Some(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        None => "--".to_string(),
    }
}

/// Returns `(dir_count, file_count, total_file_bytes)` for the footer status bar.
pub fn summarize_pane_entries(entries: &[FileEntry]) -> (usize, usize, u64) {
    let mut dirs = 0usize;
    let mut files = 0usize;
    let mut bytes = 0u64;
    for e in entries {
        if e.kind.is_dir_like() {
            dirs += 1;
        } else if matches!(e.kind, FileKind::File) {
            files += 1;
            bytes = bytes.saturating_add(e.size);
        }
    }
    (dirs, files, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FileEntry, FileKind};

    #[test]
    fn test_file_pane_filter_hidden_and_search_and_sort_dirs_first() {
        let entries = vec![
            FileEntry {
                name: "z_file.txt".into(),
                path: "/z_file.txt".into(),
                kind: FileKind::File,
                size: 100,
                modified_unix: None,
                permissions: None,
            },
            FileEntry {
                name: ".env".into(),
                path: "/.env".into(),
                kind: FileKind::File,
                size: 50,
                modified_unix: None,
                permissions: None,
            },
            FileEntry {
                name: "assets".into(),
                path: "/assets".into(),
                kind: FileKind::Directory,
                size: 0,
                modified_unix: None,
                permissions: None,
            },
        ];

        let visible = filter_and_sort_entries(&entries, "", false, SortColumn::Name, true);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].name, "assets");
        assert_eq!(visible[1].name, "z_file.txt");

        let with_hidden = filter_and_sort_entries(&entries, ".env", true, SortColumn::Name, true);
        assert_eq!(with_hidden.len(), 1);
        assert_eq!(with_hidden[0].name, ".env");
    }

    #[test]
    fn test_resolve_rename_target_path_validates_and_joins_parent() {
        let zh = I18n::new(crate::services::preferences::AppLanguage::ZhCn);
        let en = I18n::new(crate::services::preferences::AppLanguage::EnUs);

        let remote_renamed =
            resolve_rename_target_path("/bucket/docs/old.txt", "new.txt", false).unwrap();
        assert_eq!(remote_renamed, "/bucket/docs/new.txt");

        let zh_empty_err =
            resolve_rename_target_path_with_i18n("/bucket/docs/old.txt", "   ", false, zh)
                .unwrap_err()
                .to_string();
        assert_eq!(zh_empty_err, "新名称不能为空");

        let en_empty_err =
            resolve_rename_target_path_with_i18n("/bucket/docs/old.txt", "   ", false, en)
                .unwrap_err()
                .to_string();
        assert_eq!(en_empty_err, "New name cannot be empty");

        let en_sep_err =
            resolve_rename_target_path_with_i18n("/bucket/docs/old.txt", "sub/new.txt", false, en)
                .unwrap_err()
                .to_string();
        assert_eq!(en_sep_err, "New name cannot contain path separators");
    }

    #[test]
    fn test_split_breadcrumb_segments_local_and_remote_bilingual() {
        let zh = I18n::new(crate::services::preferences::AppLanguage::ZhCn);
        let en = I18n::new(crate::services::preferences::AppLanguage::EnUs);

        let remote_crumbs_zh =
            split_breadcrumb_segments_with_i18n("/prod-bucket/releases/v1", false, zh);
        assert_eq!(
            remote_crumbs_zh,
            vec![
                ("根目录".to_string(), "/".to_string()),
                ("prod-bucket".to_string(), "/prod-bucket".to_string()),
                ("releases".to_string(), "/prod-bucket/releases".to_string()),
                ("v1".to_string(), "/prod-bucket/releases/v1".to_string()),
            ]
        );

        let remote_crumbs_en =
            split_breadcrumb_segments_with_i18n("/prod-bucket/releases/v1", false, en);
        assert_eq!(
            remote_crumbs_en,
            vec![
                ("Root".to_string(), "/".to_string()),
                ("prod-bucket".to_string(), "/prod-bucket".to_string()),
                ("releases".to_string(), "/prod-bucket/releases".to_string()),
                ("v1".to_string(), "/prod-bucket/releases/v1".to_string()),
            ]
        );

        let win_crumbs_zh = split_breadcrumb_segments_with_i18n(r"C:\Users\Admin", true, zh);
        assert_eq!(
            win_crumbs_zh,
            vec![
                ("磁盘".to_string(), "/".to_string()),
                ("C:".to_string(), r"C:\".to_string()),
                ("Users".to_string(), r"C:\Users".to_string()),
                ("Admin".to_string(), r"C:\Users\Admin".to_string()),
            ]
        );

        let win_crumbs_en = split_breadcrumb_segments_with_i18n(r"C:\Users\Admin", true, en);
        assert_eq!(
            win_crumbs_en,
            vec![
                ("Drives".to_string(), "/".to_string()),
                ("C:".to_string(), r"C:\".to_string()),
                ("Users".to_string(), r"C:\Users".to_string()),
                ("Admin".to_string(), r"C:\Users\Admin".to_string()),
            ]
        );
    }

    #[test]
    fn test_format_modified_time_and_directory_summary() {
        assert_eq!(format_modified_time(None), "--");
        assert_eq!(format_modified_time(Some(1762336800)), "2025-11-05 10:00");
    }
}
