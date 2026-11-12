pub mod fallback;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

use crate::models::{FileEntry, FileKind, format_bytes};
use crate::ui::file_pane::{SortColumn, format_modified_time, join_local_or_remote};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use std::path::Path;
use std::sync::Arc;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
use fallback::PlatformHost;
#[cfg(target_os = "macos")]
use macos::PlatformHost;
#[cfg(target_os = "windows")]
use windows::PlatformHost;

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

pub const MACOS_NS_NOT_FOUND: usize = isize::MAX as usize;
pub const MAX_SAFE_PANE_POINTS: f32 = 16_384.0;
pub const MAX_SAFE_PANE_DEVICE_PX: i32 = 65_536;

pub fn collect_paths_from_index_set<F>(
    rows: &[NativeRowViewModel],
    first_index: usize,
    mut next_index: F,
) -> Vec<String>
where
    F: FnMut(usize) -> usize,
{
    let mut out = Vec::new();
    let mut cur = first_index;
    let mut steps = 0usize;
    let max_steps = rows.len();
    while cur < rows.len() && cur != MACOS_NS_NOT_FOUND && cur != usize::MAX && steps < max_steps {
        if let Some(r) = rows.get(cur) {
            if !r.is_parent_row {
                out.push(r.path.clone());
            }
        }
        let next = next_index(cur);
        if next <= cur {
            break;
        }
        cur = next;
        steps += 1;
    }
    out
}

pub fn sanitize_pane_bounds_px(
    origin_x: f32,
    origin_y: f32,
    width: f32,
    height: f32,
    viewport_width: f32,
    viewport_height: f32,
    scale_factor: f32,
) -> Option<(i32, i32, i32, i32)> {
    if !origin_x.is_finite()
        || !origin_y.is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || !viewport_width.is_finite()
        || !viewport_height.is_finite()
        || !scale_factor.is_finite()
        || width <= 0.0
        || height <= 0.0
        || viewport_width <= 0.0
        || viewport_height <= 0.0
    {
        return None;
    }

    let scale = scale_factor.clamp(0.5, 8.0);
    let max_vp_w = viewport_width.min(MAX_SAFE_PANE_POINTS);
    let max_vp_h = viewport_height.min(MAX_SAFE_PANE_POINTS);

    let left = origin_x.clamp(0.0, max_vp_w);
    let top = origin_y.clamp(0.0, max_vp_h);
    let right = (origin_x + width).clamp(left, max_vp_w);
    let bottom = (origin_y + height).clamp(top, max_vp_h);

    let clamped_w = right - left;
    let clamped_h = bottom - top;
    if clamped_w < 1.0 || clamped_h < 1.0 {
        return None;
    }

    let x_px = (left * scale).round() as i32;
    let y_px = (top * scale).round() as i32;
    let w_px = ((clamped_w * scale).round() as i32).clamp(1, MAX_SAFE_PANE_DEVICE_PX);
    let h_px = ((clamped_h * scale).round() as i32).clamp(1, MAX_SAFE_PANE_DEVICE_PX);
    Some((x_px, y_px, w_px, h_px))
}

pub fn compute_pane_visibility(
    is_connected: bool,
    settings_open: bool,
    width: i32,
    height: i32,
) -> bool {
    is_connected
        && !settings_open
        && (1..=MAX_SAFE_PANE_DEVICE_PX).contains(&width)
        && (1..=MAX_SAFE_PANE_DEVICE_PX).contains(&height)
}

pub fn compute_appkit_frame(
    bounds_px: (i32, i32, i32, i32),
    scale_factor: f32,
    parent_height_pts: f64,
    is_flipped: bool,
) -> (f64, f64, f64, f64) {
    let scale = if scale_factor.is_finite() {
        scale_factor.clamp(0.5, 8.0) as f64
    } else {
        1.0
    };
    let max_pts = MAX_SAFE_PANE_POINTS as f64;
    let safe_parent_h = if parent_height_pts.is_finite() && parent_height_pts > 0.0 {
        parent_height_pts.min(max_pts)
    } else {
        max_pts
    };
    let x = ((bounds_px.0.max(0) as f64) / scale).clamp(0.0, max_pts);
    let top_y = ((bounds_px.1.max(0) as f64) / scale).clamp(0.0, safe_parent_h);
    let raw_w = ((bounds_px.2.clamp(1, MAX_SAFE_PANE_DEVICE_PX)) as f64) / scale;
    let raw_h = ((bounds_px.3.clamp(1, MAX_SAFE_PANE_DEVICE_PX)) as f64) / scale;
    let w = raw_w.clamp(1.0, (max_pts - x).max(1.0));
    let h = raw_h.clamp(1.0, (safe_parent_h - top_y).max(1.0));
    let y = if is_flipped {
        top_y
    } else {
        (safe_parent_h - (top_y + h)).max(0.0)
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

    pub(crate) fn close(&mut self) {
        self.events.clear();
        self.wakeup_senders.clear();
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

    pub fn teardown(&self) {
        self.inner.teardown();
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
            map_row_index_to_entry(1, true, &entries)
                .unwrap()
                .unwrap()
                .name,
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

    #[test]
    fn test_compute_pane_visibility_and_appkit_frame_reject_retina_infinity_overflow() {
        // f32::INFINITY as i32 saturates to i32::MAX on Rust; pane visibility must reject overflowed bounds
        assert!(
            !compute_pane_visibility(true, false, i32::MAX, 600),
            "overflowed i32::MAX width must never be treated as a valid visible pane"
        );
        assert!(
            !compute_pane_visibility(true, false, 800, i32::MAX),
            "overflowed i32::MAX height must never be treated as a valid visible pane"
        );

        // Even if huge pixel bounds reach compute_appkit_frame, dimensions must be clamped safely
        let (_x, _y, w, h) = compute_appkit_frame((0, 0, i32::MAX, i32::MAX), 2.0, 900.0, false);
        assert!(
            w <= 16_384.0 && h <= 900.0,
            "compute_appkit_frame must clamp overflowed dimensions instead of allocating a 1B-pt backing store, got ({w}, {h})"
        );
    }

    #[test]
    fn test_macos_avoids_nsuinteger_max_hang_and_metal_subview_conflict() {
        let macos_src = include_str!("macos.rs");
        assert!(
            !macos_src.contains("NSUInteger::MAX"),
            "NSIndexSet returns NSNotFound (isize::MAX as usize), never NSUInteger::MAX; comparing != NSUInteger::MAX hangs forever"
        );
        assert!(
            !macos_src.contains("msg_send![root_view, addSubview: sv]"),
            "NSScrollView must not be added directly inside CAMetalLayer-backed GPUIView"
        );
        assert!(
            macos_src.contains("makeViewWithIdentifier:"),
            "view_for_table_column must reuse NSTableCellView via makeViewWithIdentifier:owner:"
        );
        assert!(
            !macos_src.contains("iconForFile:"),
            "view_for_table_column must not call synchronous disk-hitting iconForFile: per row"
        );
        assert!(
            !macos_src.contains("arrayWithObjects:"),
            "macos.rs should use safe NSArray::from_retained_slice instead of raw arrayWithObjects: pointer arrays"
        );
        assert!(
            macos_src.contains("autoreleasepool"),
            "macos.rs must drain temporary Objective-C objects via autoreleasepool"
        );
        assert!(
            !macos_src.contains("static mut "),
            "macos.rs must avoid static mut for Rust edition 2024 compatibility"
        );

        let dual_pane_src = include_str!("../dual_pane_view.rs");
        assert!(
            !dual_pane_src.contains("resizable_panel().child("),
            "resizable_panel() defaults max_size to Pixels::MAX (f32::MAX), which overflows f32::MAX * 2.0 = INFINITY on Retina macOS"
        );
    }

    #[test]
    fn test_collect_paths_from_index_set_terminates_on_ns_not_found_and_non_monotonic_index() {
        let entries = sample_entries();
        let rows = build_native_row_models(&entries, true, "..");

        // Empty NSIndexSet returns NSNotFound = isize::MAX as usize (NOT usize::MAX!)
        let empty = collect_paths_from_index_set(&rows, MACOS_NS_NOT_FOUND, |_| MACOS_NS_NOT_FOUND);
        assert!(empty.is_empty());

        // Selecting row 0 (parent "..") and row 2 ("/var/report.pdf") then NSNotFound
        let selected = collect_paths_from_index_set(&rows, 0, |cur| match cur {
            0 => 2,
            _ => MACOS_NS_NOT_FOUND,
        });
        assert_eq!(selected, vec!["/var/report.pdf".to_string()]);

        // Defensive guard: even a broken non-monotonic callback (returning same index) must terminate
        let non_monotonic = collect_paths_from_index_set(&rows, 1, |cur| cur);
        assert_eq!(non_monotonic, vec!["/var/docs".to_string()]);
    }

    #[test]
    fn test_sanitize_pane_bounds_px_clamps_to_viewport_and_rejects_non_finite() {
        // Retina 2x normal pane inside 1280x800 pt viewport
        assert_eq!(
            sanitize_pane_bounds_px(10.0, 80.0, 620.0, 650.0, 1280.0, 800.0, 2.0),
            Some((20, 160, 1240, 1300))
        );

        // Overflowed f32::INFINITY / f32::MAX width from unbounded Taffy layout is clamped to viewport (or rejected if non-finite)
        assert_eq!(
            sanitize_pane_bounds_px(0.0, 80.0, f32::INFINITY, 650.0, 1280.0, 800.0, 2.0),
            None
        );
        assert_eq!(
            sanitize_pane_bounds_px(0.0, 80.0, f32::NAN, 650.0, 1280.0, 800.0, 2.0),
            None
        );
        assert_eq!(
            sanitize_pane_bounds_px(640.0, 80.0, 50_000.0, 650.0, 1280.0, 800.0, 2.0),
            Some((1280, 160, 1280, 1300))
        );
        assert_eq!(
            sanitize_pane_bounds_px(0.0, 80.0, 0.0, 650.0, 1280.0, 800.0, 2.0),
            None
        );
    }

    #[test]
    fn test_native_file_list_host_teardown_closes_wakeup_stream_idempotently() {
        let host = NativeFileListHost::new();
        let mut rx = host.subscribe_wakeup();
        host.push_event(NativePaneEvent::NavigateUp { is_local: true });

        host.teardown();
        host.teardown();

        assert!(host.drain_events().is_empty());
        // After teardown drops wakeup senders, draining any buffered wakeup reaches Disconnected EOF
        while rx.try_recv().is_ok() {}
        assert!(
            matches!(
                rx.try_recv(),
                Err(futures::channel::mpsc::TryRecvError::Closed)
            ),
            "teardown must close wakeup channels so background watcher exits immediately"
        );
    }
}
