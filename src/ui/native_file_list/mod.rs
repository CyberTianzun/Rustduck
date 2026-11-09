pub mod fallback;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

use crate::models::{format_bytes, FileEntry, FileKind};
use crate::ui::file_pane::{format_modified_time, join_local_or_remote, SortColumn};
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use std::path::Path;
use std::sync::Arc;

#[cfg(target_os = "macos")]
use macos::PlatformHost;
#[cfg(target_os = "windows")]
use windows::PlatformHost;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
use fallback::PlatformHost;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeContextMenuAction {
    TransferSelected,
    OpenSelected,
    RenameSelected,
    NewFolder,
    DeleteSelected,
    CopySelectedPath,
    Refresh,
    RevealInShell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativePaneEvent {
    SelectionChanged {
        is_local: bool,
        selected_paths: Vec<String>,
    },
    ActivateEntry {
        is_local: bool,
        entry: FileEntry,
    },
    NavigateUp {
        is_local: bool,
    },
    SortChanged {
        is_local: bool,
        column: SortColumn,
    },
    InlineRenameCommitted {
        is_local: bool,
        original_path: String,
        new_name: String,
    },
    ContextMenuCommand {
        is_local: bool,
        action: NativeContextMenuAction,
    },
    FilesDropped {
        target_is_local: bool,
        source_paths: Vec<String>,
        target_dir_override: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRowViewModel {
    pub name: String,
    pub path: String,
    pub modified_text: String,
    pub size_text: String,
    pub is_dir: bool,
    pub is_bucket: bool,
    pub is_parent_row: bool,
    pub extension: String,
}

#[derive(Debug, Clone)]
pub struct NativePaneSnapshot {
    pub tab_idx: usize,
    pub is_local: bool,
    pub current_path: String,
    pub bounds_px: (i32, i32, i32, i32),
    pub scale_factor: f32,
    pub visible: bool,
    pub is_dark: bool,
    pub sort_col: SortColumn,
    pub sort_ascending: bool,
    pub has_parent: bool,
    pub entries_revision: u64,
    pub rows: Arc<Vec<NativeRowViewModel>>,
    pub entries: Arc<Vec<FileEntry>>,
    pub selected_paths: Vec<String>,
}

pub fn build_native_row_models(
    entries: &[FileEntry],
    has_parent: bool,
    parent_label: &str,
) -> Vec<NativeRowViewModel> {
    let mut rows = Vec::with_capacity(entries.len() + usize::from(has_parent));
    if has_parent {
        rows.push(NativeRowViewModel {
            name: parent_label.to_string(),
            path: "..".to_string(),
            modified_text: String::new(),
            size_text: String::new(),
            is_dir: true,
            is_bucket: false,
            is_parent_row: true,
            extension: String::new(),
        });
    }

    for entry in entries {
        let is_dir = entry.kind.is_dir_like();
        let is_bucket = matches!(entry.kind, FileKind::Bucket);
        let size_text = if is_dir {
            "--".to_string()
        } else {
            format_bytes(entry.size)
        };
        let modified_text = format_modified_time(entry.modified_unix);
        let extension = if is_dir {
            String::new()
        } else {
            Path::new(&entry.name)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
        };

        rows.push(NativeRowViewModel {
            name: entry.name.clone(),
            path: entry.path.clone(),
            modified_text,
            size_text,
            is_dir,
            is_bucket,
            is_parent_row: false,
            extension,
        });
    }

    rows
}

pub fn map_row_index_to_entry(
    row_idx: usize,
    has_parent: bool,
    entries: &[FileEntry],
) -> Option<Result<&FileEntry, ()>> {
    if has_parent {
        if row_idx == 0 {
            Some(Err(()))
        } else {
            entries.get(row_idx - 1).map(Ok)
        }
    } else {
        entries.get(row_idx).map(Ok)
    }
}

pub fn resolve_drop_destination_dir(
    current_dir: &str,
    hovered_entry: Option<&FileEntry>,
    is_local: bool,
) -> String {
    if let Some(entry) = hovered_entry {
        if entry.kind.is_dir_like() {
            if !entry.path.trim().is_empty() {
                return entry.path.clone();
            }
            return join_local_or_remote(current_dir, &entry.name, is_local);
        }
    }
    current_dir.to_string()
}

pub fn should_commit_inline_rename(original_name: &str, edited_text: &str) -> Option<String> {
    let trimmed = edited_text.trim();
    if trimmed.is_empty() || trimmed == original_name {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn resolve_inline_rename_entry<'a>(
    tracked_path: Option<&str>,
    fallback_row: Option<usize>,
    has_parent: bool,
    entries: &'a [FileEntry],
) -> Option<&'a FileEntry> {
    if let Some(target_path) = tracked_path {
        if !target_path.is_empty() {
            return entries.iter().find(|e| e.path == target_path);
        }
    }
    let row_idx = fallback_row?;
    match map_row_index_to_entry(row_idx, has_parent, entries) {
        Some(Ok(entry)) => Some(entry),
        _ => None,
    }
}

pub fn compute_pane_visibility(
    is_connected: bool,
    settings_open: bool,
    width: i32,
    height: i32,
) -> bool {
    is_connected && !settings_open && width > 0 && height > 0
}

pub fn compute_appkit_frame(
    bounds_px: (i32, i32, i32, i32),
    scale_factor: f32,
    parent_height_pts: f64,
    is_flipped: bool,
) -> (f64, f64, f64, f64) {
    let scale = (scale_factor.max(0.5)) as f64;
    let x = (bounds_px.0 as f64) / scale;
    let top_y = (bounds_px.1 as f64) / scale;
    let w = ((bounds_px.2.max(1)) as f64) / scale;
    let h = ((bounds_px.3.max(1)) as f64) / scale;
    let y = if is_flipped {
        top_y
    } else {
        (parent_height_pts - (top_y + h)).max(0.0)
    };
    (x, y, w, h)
}

#[derive(Default)]
pub(crate) struct NativeEventSink {
    events: Vec<NativePaneEvent>,
    wakeup_senders: Vec<UnboundedSender<()>>,
}

impl NativeEventSink {
    pub(crate) fn push(&mut self, event: NativePaneEvent) {
        self.events.push(event);
        self.wakeup_senders
            .retain(|tx| tx.unbounded_send(()).is_ok());
    }

    pub(crate) fn drain(&mut self) -> Vec<NativePaneEvent> {
        std::mem::take(&mut self.events)
    }

    pub(crate) fn subscribe(&mut self) -> UnboundedReceiver<()> {
        let (tx, rx) = unbounded();
        self.wakeup_senders.push(tx);
        rx
    }
}

#[derive(Default)]
pub struct NativeFileListHost {
    inner: PlatformHost,
}

impl NativeFileListHost {
    pub fn new() -> Self {
        Self {
            inner: PlatformHost::new(),
        }
    }

    pub fn sync_pane(&self, window: &gpui::Window, snapshot: &NativePaneSnapshot) {
        self.inner.sync_pane(window, snapshot);
    }

    pub fn hide_all(&self) {
        self.inner.hide_all();
    }

    pub fn drain_events(&self) -> Vec<NativePaneEvent> {
        self.inner.drain_events()
    }

    pub fn subscribe_wakeup(&self) -> UnboundedReceiver<()> {
        self.inner.subscribe_wakeup()
    }

    pub fn push_event(&self, event: NativePaneEvent) {
        self.inner.push_event(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FileEntry, FileKind};

    fn sample_entries() -> Vec<FileEntry> {
        vec![
            FileEntry {
                name: "docs".into(),
                path: "/var/docs".into(),
                kind: FileKind::Directory,
                size: 0,
                modified_unix: Some(1762336800),
                permissions: None,
            },
            FileEntry {
                name: "report.pdf".into(),
                path: "/var/report.pdf".into(),
                kind: FileKind::File,
                size: 2048,
                modified_unix: Some(1762336800),
                permissions: None,
            },
        ]
    }

    #[test]
    fn test_build_native_row_models_and_row_mapping_with_parent() {
        let entries = sample_entries();
        let rows = build_native_row_models(&entries, true, ".. (上级目录)");
        assert_eq!(rows.len(), 3);
        assert!(rows[0].is_parent_row);
        assert_eq!(rows[1].name, "docs");
        assert_eq!(rows[2].extension, "pdf");

        assert_eq!(map_row_index_to_entry(0, true, &entries), Some(Err(())));
        assert_eq!(
            map_row_index_to_entry(1, true, &entries).unwrap().unwrap().name,
            "docs"
        );
        assert_eq!(map_row_index_to_entry(99, true, &entries), None);
    }

    #[test]
    fn test_resolve_drop_destination_dir_prefers_hovered_directory() {
        let entries = sample_entries();
        assert_eq!(
            resolve_drop_destination_dir("/var", Some(&entries[0]), false),
            "/var/docs"
        );
        assert_eq!(
            resolve_drop_destination_dir("/var", Some(&entries[1]), false),
            "/var"
        );
        assert_eq!(resolve_drop_destination_dir("/var", None, false), "/var");
    }

    #[test]
    fn test_should_commit_inline_rename_ignores_empty_and_unchanged() {
        assert_eq!(should_commit_inline_rename("a.txt", "   "), None);
        assert_eq!(should_commit_inline_rename("a.txt", "a.txt"), None);
        assert_eq!(
            should_commit_inline_rename("a.txt", " b.txt "),
            Some("b.txt".to_string())
        );
    }

    #[test]
    fn test_compute_pane_visibility_hides_when_modal_or_disconnected() {
        assert!(compute_pane_visibility(true, false, 400, 300));
        assert!(!compute_pane_visibility(true, true, 400, 300));
        assert!(!compute_pane_visibility(false, false, 400, 300));
        assert!(!compute_pane_visibility(true, false, 0, 300));
    }

    #[test]
    fn test_compute_appkit_frame_converts_retina_pixels_and_flips_y() {
        let frame = compute_appkit_frame((20, 200, 800, 600), 2.0, 500.0, false);
        assert_eq!(frame, (10.0, 100.0, 400.0, 300.0));

        let flipped = compute_appkit_frame((20, 160, 800, 600), 2.0, 500.0, true);
        assert_eq!(flipped, (10.0, 80.0, 400.0, 300.0));
    }

    #[test]
    fn test_native_file_list_host_wakeup_fires_when_event_pushed() {
        let host = NativeFileListHost::new();
        let mut rx = host.subscribe_wakeup();
        assert!(rx.try_recv().is_err());

        host.push_event(NativePaneEvent::NavigateUp { is_local: true });
        assert_eq!(rx.try_recv().unwrap(), ());
        assert_eq!(
            host.drain_events(),
            vec![NativePaneEvent::NavigateUp { is_local: true }]
        );
    }

    #[test]
    fn test_resolve_inline_rename_entry_prefers_tracked_path_over_shifted_row() {
        let shifted_entries = vec![
            FileEntry {
                name: "00_new_folder".into(),
                path: "/var/00_new_folder".into(),
                kind: FileKind::Directory,
                size: 0,
                modified_unix: None,
                permissions: None,
            },
            FileEntry {
                name: "docs".into(),
                path: "/var/docs".into(),
                kind: FileKind::Directory,
                size: 0,
                modified_unix: None,
                permissions: None,
            },
            FileEntry {
                name: "report.pdf".into(),
                path: "/var/report.pdf".into(),
                kind: FileKind::File,
                size: 2048,
                modified_unix: None,
                permissions: None,
            },
        ];

        // Row 1 used to be "/var/docs", now "/var/00_new_folder" is at row 1 and "/var/docs" is at row 2.
        let resolved =
            resolve_inline_rename_entry(Some("/var/docs"), Some(1), true, &shifted_entries);
        assert_eq!(resolved.map(|e| e.path.as_str()), Some("/var/docs"));

        // If the tracked path was deleted mid-edit, abort rather than renaming whatever is now at row 1.
        let deleted =
            resolve_inline_rename_entry(Some("/var/deleted.txt"), Some(1), true, &shifted_entries);
        assert_eq!(deleted, None);

        // Without a tracked path hint, fall back to live row index mapping.
        let by_row = resolve_inline_rename_entry(None, Some(3), true, &shifted_entries);
        assert_eq!(by_row.map(|e| e.path.as_str()), Some("/var/report.pdf"));
    }

    #[test]
    fn test_macos_source_balances_retains_and_uses_weak_delegate_state() {
        let macos_src = include_str!("macos.rs");
        let delegate_block = macos_src
            .split("struct DelegateContext {")
            .nth(1)
            .and_then(|s| s.split('}').next())
            .expect("DelegateContext struct must exist in macos.rs");
        assert!(
            delegate_block.contains("state: Weak<Mutex<HostInner>>"),
            "DelegateContext must hold Weak<Mutex<HostInner>> to avoid Arc reference cycle"
        );
        assert!(
            !delegate_block.contains("Arc<Mutex<HostInner>>"),
            "DelegateContext must not hold strong Arc<Mutex<HostInner>>"
        );
        assert!(
            macos_src.contains("msg_send![tf, autorelease]"),
            "NSTextField subview must be autoreleased after addSubview"
        );
        assert!(
            macos_src.contains("msg_send![iv, autorelease]"),
            "NSImageView subview must be autoreleased after addSubview"
        );
        assert!(
            macos_src.contains("msg_send![item, autorelease]"),
            "NSMenuItem must be autoreleased after addItem"
        );
        assert!(
            macos_src.contains("rowForView:"),
            "macOS inline rename must resolve live row via [tableView rowForView:] instead of stale [tf tag]"
        );
    }
}
