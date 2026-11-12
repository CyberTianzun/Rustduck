use super::{
    NativeContextMenuAction, NativeEventSink, NativePaneEvent, NativePaneSnapshot,
    NativeRowViewModel, map_row_index_to_entry, resolve_drop_destination_dir,
    resolve_inline_rename_entry, should_commit_inline_rename,
};
use crate::i18n::i18n;
use crate::models::FileEntry;
use crate::ui::file_pane::SortColumn;
use futures::channel::mpsc::UnboundedReceiver;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null;
#[cfg(test)]
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, CreateFontIndirectW, CreateSolidBrush, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX,
    DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, FillRect, HFONT, HGDIOBJ,
    InvalidateRect, ScreenToClient, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize, RevokeDragDrop};
use windows::Win32::UI::Controls::{
    CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDRF_DODEFAULT, CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYPOSTPAINT,
    CDRF_SKIPDEFAULT, HDF_LEFT, HDF_RIGHT, HDF_SORTDOWN, HDF_SORTUP, HDF_STRING, HDI_FORMAT,
    HDITEMW, HDM_GETITEMW, HDM_SETITEMW, HEADER_CONTROL_FORMAT_FLAGS, ICC_LISTVIEW_CLASSES,
    INITCOMMONCONTROLSEX, InitCommonControlsEx, LVCF_FMT, LVCF_TEXT, LVCF_WIDTH, LVCFMT_LEFT,
    LVCFMT_RIGHT, LVCOLUMNW, LVHITTESTINFO, LVIF_IMAGE, LVIF_STATE, LVIF_TEXT, LVIS_SELECTED,
    LVITEMW, LVM_EDITLABELW, LVM_ENSUREVISIBLE, LVM_GETHEADER, LVM_GETNEXTITEM, LVM_HITTEST,
    LVM_INSERTCOLUMNW, LVM_REDRAWITEMS, LVM_SETBKCOLOR, LVM_SETCOLUMNW, LVM_SETCOLUMNWIDTH,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETIMAGELIST, LVM_SETITEMCOUNT, LVM_SETITEMSTATE,
    LVM_SETTEXTBKCOLOR, LVM_SETTEXTCOLOR, LVN_BEGINDRAG, LVN_BEGINLABELEDITW, LVN_COLUMNCLICK,
    LVN_ENDLABELEDITW, LVN_GETDISPINFOW, LVN_ITEMCHANGED, LVN_KEYDOWN, LVN_ODSTATECHANGED,
    LVNI_FOCUSED, LVNI_SELECTED, LVS_EDITLABELS, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT,
    LVS_EX_HEADERDRAGDROP, LVS_OWNERDATA, LVS_REPORT, LVS_SHAREIMAGELISTS, LVS_SHOWSELALWAYS,
    LVSICF_NOINVALIDATEALL, LVSICF_NOSCROLL, LVSIL_SMALL, NM_CUSTOMDRAW, NM_DBLCLK, NM_RCLICK,
    NMCUSTOMDRAW, NMHDR, NMITEMACTIVATE, NMLISTVIEW, NMLVDISPINFOW, NMLVKEYDOWN, SetWindowTheme,
    WC_LISTVIEWW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_A, VK_BACK, VK_CONTROL, VK_DELETE, VK_F2,
    VK_RETURN,
};
#[cfg(test)]
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW, DragQueryPoint, HDROP,
    RemoveWindowSubclass, SHFILEINFOW, SHGFI_SMALLICON, SHGFI_SYSICONINDEX,
    SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW, SetWindowSubclass,
};
#[cfg(test)]
use windows::Win32::UI::Shell::{
    ILFindLastID, ILFree, IShellFolder, SHBindToObject, SHBindToParent, SHParseDisplayName,
};
#[cfg(test)]
use windows::Win32::UI::WindowsAndMessaging::WS_POPUP;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DestroyMenu, DestroyWindow, GWL_STYLE,
    GetClientRect, GetCursorPos, GetWindowLongW, HMENU, IDC_ARROW, IDC_HAND, LoadCursorW,
    MF_GRAYED, MF_SEPARATOR, MF_STRING, NONCLIENTMETRICSW, PostMessageW, SPI_GETNONCLIENTMETRICS,
    SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetCursor,
    SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow, SystemParametersInfoW,
    TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_CAPTURECHANGED, WM_CONTEXTMENU, WM_DROPFILES, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_NOTIFY, WM_NULL, WM_RBUTTONDOWN, WM_SETFONT, WS_CHILD,
    WS_CLIPCHILDREN, WS_TABSTOP,
};
use windows::core::{PCWSTR, PWSTR, w};

const PARENT_SUBCLASS_ID: usize = 0x5244_0001;
const CHILD_SUBCLASS_ID: usize = 0x5244_0002;
const LEFT_LISTVIEW_ID: usize = 1001;
const RIGHT_LISTVIEW_ID: usize = 1002;
const WM_APP_SHOW_CONTEXT_MENU: u32 = WM_APP + 0x42;

const CMD_TRANSFER: u32 = 30_001;
const CMD_OPEN: u32 = 30_002;
const CMD_RENAME: u32 = 30_003;
const CMD_NEW_FOLDER: u32 = 30_004;
const CMD_DELETE: u32 = 30_005;
const CMD_COPY_PATH: u32 = 30_006;
const CMD_REFRESH: u32 = 30_007;
const CMD_REVEAL_IN_SHELL: u32 = 30_008;

#[inline]
unsafe fn send_msg(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> LRESULT {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))) }
}

pub fn can_use_shell_context_menu(current_path: &str, selected_paths: &[String]) -> bool {
    let trimmed = current_path.trim();
    if trimmed.is_empty() || trimmed == "/" || trimmed == "\\" {
        return false;
    }
    !selected_paths.is_empty()
}

pub fn resolve_drag_source_paths(
    selected_paths: &[String],
    dragged_row: i32,
    has_parent: bool,
    entries: &[FileEntry],
) -> Vec<String> {
    if dragged_row >= 0 {
        match map_row_index_to_entry(dragged_row as usize, has_parent, entries) {
            Some(Ok(entry)) => {
                if matches!(entry.kind, crate::models::FileKind::Bucket) {
                    return Vec::new();
                }
                if selected_paths.iter().any(|p| p == &entry.path) {
                    return selected_paths.to_vec();
                }
                return vec![entry.path.clone()];
            }
            _ => return Vec::new(),
        }
    }
    selected_paths.to_vec()
}

pub struct ShellIconCache {
    sys_image_list: isize,
    folder_icon: i32,
    drive_icon: i32,
    default_file_icon: i32,
    by_ext: HashMap<String, i32>,
}

impl ShellIconCache {
    pub fn new() -> Self {
        let mut sfi = SHFILEINFOW::default();
        let flags = SHGFI_SYSICONINDEX | SHGFI_SMALLICON | SHGFI_USEFILEATTRIBUTES;
        let sys_list = unsafe {
            SHGetFileInfoW(
                w!("C:\\"),
                FILE_ATTRIBUTE_DIRECTORY,
                Some(&mut sfi),
                size_of::<SHFILEINFOW>() as u32,
                flags,
            )
        };
        let drive_icon = sfi.iIcon;

        let mut dir_sfi = SHFILEINFOW::default();
        unsafe {
            SHGetFileInfoW(
                w!("folder"),
                FILE_ATTRIBUTE_DIRECTORY,
                Some(&mut dir_sfi),
                size_of::<SHFILEINFOW>() as u32,
                flags,
            );
        }
        let folder_icon = dir_sfi.iIcon;

        let mut file_sfi = SHFILEINFOW::default();
        unsafe {
            SHGetFileInfoW(
                w!("file.bin"),
                FILE_ATTRIBUTE_NORMAL,
                Some(&mut file_sfi),
                size_of::<SHFILEINFOW>() as u32,
                flags,
            );
        }
        let default_file_icon = file_sfi.iIcon;

        Self {
            sys_image_list: sys_list as isize,
            folder_icon,
            drive_icon,
            default_file_icon,
            by_ext: HashMap::new(),
        }
    }

    pub fn icon_index_for(
        &mut self,
        is_dir: bool,
        is_drive_or_bucket: bool,
        is_parent_row: bool,
        ext: &str,
    ) -> i32 {
        if is_parent_row || (is_dir && !is_drive_or_bucket) {
            return self.folder_icon;
        }
        if is_drive_or_bucket {
            return self.drive_icon;
        }
        let key = ext.trim().to_ascii_lowercase();
        if key.is_empty() {
            return self.default_file_icon;
        }
        if let Some(&cached) = self.by_ext.get(&key) {
            return cached;
        }

        let dummy_name: Vec<u16> = format!("dummy.{key}\0").encode_utf16().collect();
        let mut sfi = SHFILEINFOW::default();
        let res = unsafe {
            SHGetFileInfoW(
                PCWSTR(dummy_name.as_ptr()),
                FILE_ATTRIBUTE_NORMAL,
                Some(&mut sfi),
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_SYSICONINDEX | SHGFI_SMALLICON | SHGFI_USEFILEATTRIBUTES,
            )
        };
        let idx = if res != 0 {
            sfi.iIcon
        } else {
            self.default_file_icon
        };
        self.by_ext.insert(key, idx);
        idx
    }
}

struct PaneControlState {
    hwnd: isize,
    visible: bool,
    bounds_px: (i32, i32, i32, i32),
    is_dark: Option<bool>,
    tab_idx: usize,
    entries_revision: u64,
    current_path: String,
    has_parent: bool,
    sort_col: SortColumn,
    sort_ascending: bool,
    header_labels: (String, String, String),
    rows: Arc<Vec<NativeRowViewModel>>,
    entries: Arc<Vec<FileEntry>>,
    selected_paths: Vec<String>,
    editing_original_path: Option<String>,
}

impl PaneControlState {
    fn new() -> Self {
        Self {
            hwnd: 0,
            visible: false,
            bounds_px: (0, 0, 0, 0),
            is_dark: None,
            tab_idx: usize::MAX,
            entries_revision: u64::MAX,
            current_path: String::new(),
            has_parent: false,
            sort_col: SortColumn::Name,
            sort_ascending: true,
            header_labels: (String::new(), String::new(), String::new()),
            rows: Arc::new(Vec::new()),
            entries: Arc::new(Vec::new()),
            selected_paths: Vec::new(),
            editing_original_path: None,
        }
    }

    fn take_hide_hwnd(&mut self) -> Option<HWND> {
        if self.visible && self.hwnd != 0 {
            self.visible = false;
            Some(HWND(self.hwnd as *mut c_void))
        } else {
            None
        }
    }
}

struct ActivePaneDrag {
    from_local: bool,
    source_paths: Vec<String>,
}

struct HostInner {
    parent_hwnd: isize,
    subclass_ctx_ptr: usize,
    ui_font: isize,
    icon_cache: ShellIconCache,
    left: PaneControlState,
    right: PaneControlState,
    events: NativeEventSink,
    active_pane_drag: Option<ActivePaneDrag>,
}

pub struct PlatformHost {
    state: Arc<Mutex<HostInner>>,
    suppress_selection_events: Arc<AtomicBool>,
    in_context_menu: Arc<AtomicBool>,
    ole_initialized: bool,
}

impl Default for PlatformHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PlatformHost {
    fn drop(&mut self) {
        if self.ole_initialized {
            unsafe {
                OleUninitialize();
            }
        }
    }
}

impl PlatformHost {
    pub fn new() -> Self {
        let ole_initialized = unsafe {
            let ole_ok = OleInitialize(None).is_ok();
            let icc = INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_LISTVIEW_CLASSES,
            };
            let _ = InitCommonControlsEx(&icc);
            ole_ok
        };

        let inner = HostInner {
            parent_hwnd: 0,
            subclass_ctx_ptr: 0,
            ui_font: create_message_font(),
            icon_cache: ShellIconCache::new(),
            left: PaneControlState::new(),
            right: PaneControlState::new(),
            events: NativeEventSink::default(),
            active_pane_drag: None,
        };

        Self {
            state: Arc::new(Mutex::new(inner)),
            suppress_selection_events: Arc::new(AtomicBool::new(false)),
            in_context_menu: Arc::new(AtomicBool::new(false)),
            ole_initialized,
        }
    }

    pub fn sync_pane(&self, window: &gpui::Window, snapshot: &NativePaneSnapshot) {
        let Ok(raw_handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::Win32(win32_handle) = raw_handle.as_raw() else {
            return;
        };
        let parent_hwnd_val = win32_handle.hwnd.get();
        if parent_hwnd_val == 0 {
            return;
        }
        self.sync_pane_for_parent_hwnd(parent_hwnd_val, snapshot);
    }

    pub(crate) fn sync_pane_for_parent_hwnd(
        &self,
        parent_hwnd_val: isize,
        snapshot: &NativePaneSnapshot,
    ) {
        if self.in_context_menu.load(Ordering::SeqCst) {
            return;
        }
        self.ensure_controls_created(parent_hwnd_val);

        let mut guard = self.state.lock().unwrap();
        let sys_img_list = guard.icon_cache.sys_image_list;
        for row in snapshot.rows.iter() {
            let is_drive_or_bucket = row.is_bucket || (row.is_dir && row.name.ends_with(':'));
            let _ = guard.icon_cache.icon_index_for(
                row.is_dir,
                is_drive_or_bucket,
                row.is_parent_row,
                &row.extension,
            );
        }

        let pane = if snapshot.is_local {
            &mut guard.left
        } else {
            &mut guard.right
        };
        let hwnd = HWND(pane.hwnd as *mut c_void);
        if hwnd.0.is_null() {
            return;
        }

        let need_imagelist = if pane.is_dark.is_none() && sys_img_list != 0 {
            Some(sys_img_list)
        } else {
            None
        };

        let bounds_changed = pane.bounds_px != snapshot.bounds_px;
        let vis_changed = pane.visible != snapshot.visible;
        pane.bounds_px = snapshot.bounds_px;
        pane.visible = snapshot.visible;

        if !snapshot.visible {
            drop(guard);
            if vis_changed {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    if parent_hwnd_val != 0 {
                        let _ = SetFocus(Some(HWND(parent_hwnd_val as *mut c_void)));
                    }
                }
            }
            return;
        }

        let need_theme = if pane.is_dark != Some(snapshot.is_dark) {
            pane.is_dark = Some(snapshot.is_dark);
            Some(snapshot.is_dark)
        } else {
            None
        };

        let t = i18n();
        let desired_labels = (
            t.col_name().to_string(),
            t.col_modified().to_string(),
            t.col_size().to_string(),
        );
        let sort_changed =
            pane.sort_col != snapshot.sort_col || pane.sort_ascending != snapshot.sort_ascending;
        let need_headers = if pane.header_labels != desired_labels || sort_changed {
            pane.header_labels = desired_labels.clone();
            pane.sort_col = snapshot.sort_col;
            pane.sort_ascending = snapshot.sort_ascending;
            Some((desired_labels, snapshot.sort_col, snapshot.sort_ascending))
        } else {
            None
        };

        let data_changed = pane.tab_idx != snapshot.tab_idx
            || pane.entries_revision != snapshot.entries_revision
            || pane.rows.len() != snapshot.rows.len();
        pane.tab_idx = snapshot.tab_idx;
        pane.entries_revision = snapshot.entries_revision;
        pane.current_path = snapshot.current_path.clone();
        pane.has_parent = snapshot.has_parent;
        pane.rows = Arc::clone(&snapshot.rows);
        pane.entries = Arc::clone(&snapshot.entries);

        let item_count = data_changed.then_some(pane.rows.len());
        let need_sel_sync = if data_changed || pane.selected_paths != snapshot.selected_paths {
            pane.selected_paths = snapshot.selected_paths.clone();
            Some(Arc::clone(&pane.rows))
        } else {
            None
        };
        let (x, y, w, h) = snapshot.bounds_px;
        drop(guard);

        if let Some(img_list) = need_imagelist {
            unsafe {
                send_msg(hwnd, LVM_SETIMAGELIST, LVSIL_SMALL as usize, img_list);
            }
        }
        if bounds_changed {
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    x,
                    y,
                    w.max(1),
                    h.max(1),
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            resize_columns_to_width(hwnd, w);
        }
        if vis_changed {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        }
        if let Some(is_dark) = need_theme {
            apply_listview_theme(hwnd, is_dark);
        }
        if let Some((labels, sort_col, sort_asc)) = need_headers {
            update_column_headers_and_sort(hwnd, &labels, sort_col, sort_asc);
        }
        if let Some(count) = item_count {
            unsafe {
                send_msg(
                    hwnd,
                    LVM_SETITEMCOUNT,
                    count,
                    (LVSICF_NOINVALIDATEALL | LVSICF_NOSCROLL) as isize,
                );
                if count > 0 {
                    send_msg(hwnd, LVM_REDRAWITEMS, 0, (count - 1) as isize);
                }
            }
        }
        if let Some(rows_clone) = need_sel_sync {
            let sel_set: HashSet<&str> =
                snapshot.selected_paths.iter().map(|s| s.as_str()).collect();
            self.suppress_selection_events.store(true, Ordering::SeqCst);
            sync_listview_selection(hwnd, &rows_clone, &sel_set);
            self.suppress_selection_events
                .store(false, Ordering::SeqCst);
        }
    }

    pub fn hide_all(&self) {
        let (left_hwnd, right_hwnd, parent_hwnd_val) = {
            let mut guard = self.state.lock().unwrap();
            (
                guard.left.take_hide_hwnd(),
                guard.right.take_hide_hwnd(),
                guard.parent_hwnd,
            )
        };
        let any_hidden = left_hwnd.is_some() || right_hwnd.is_some();
        unsafe {
            for hwnd in [left_hwnd, right_hwnd].into_iter().flatten() {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            if any_hidden && parent_hwnd_val != 0 {
                let _ = SetFocus(Some(HWND(parent_hwnd_val as *mut c_void)));
            }
        }
    }

    pub fn teardown(&self) {
        let (left_hwnd, right_hwnd, parent_hwnd, subclass_ctx_ptr) = {
            let mut guard = self.state.lock().unwrap();
            let tup = (
                guard.left.hwnd,
                guard.right.hwnd,
                guard.parent_hwnd,
                guard.subclass_ctx_ptr,
            );
            guard.left.hwnd = 0;
            guard.right.hwnd = 0;
            guard.left.visible = false;
            guard.right.visible = false;
            guard.parent_hwnd = 0;
            guard.subclass_ctx_ptr = 0;
            guard.events.close();
            tup
        };
        unsafe {
            teardown_win32_controls(left_hwnd, right_hwnd, parent_hwnd, subclass_ctx_ptr);
        }
    }

    pub fn drain_events(&self) -> Vec<NativePaneEvent> {
        let mut guard = self.state.lock().unwrap();
        guard.events.drain()
    }

    pub fn subscribe_wakeup(&self) -> UnboundedReceiver<()> {
        let mut guard = self.state.lock().unwrap();
        guard.events.subscribe()
    }

    pub fn push_event(&self, event: NativePaneEvent) {
        let mut guard = self.state.lock().unwrap();
        guard.events.push(event);
    }

    fn ensure_controls_created(&self, parent_hwnd_val: isize) {
        let (old_left, old_right, old_parent, old_ctx, font_val) = {
            let mut guard = self.state.lock().unwrap();
            if guard.parent_hwnd == parent_hwnd_val && guard.left.hwnd != 0 && guard.right.hwnd != 0
            {
                return;
            }
            let tup = (
                guard.left.hwnd,
                guard.right.hwnd,
                guard.parent_hwnd,
                guard.subclass_ctx_ptr,
                guard.ui_font,
            );
            guard.left.hwnd = 0;
            guard.right.hwnd = 0;
            guard.parent_hwnd = 0;
            guard.subclass_ctx_ptr = 0;
            tup
        };

        unsafe {
            teardown_win32_controls(old_left, old_right, old_parent, old_ctx);
        }

        let parent_hwnd = HWND(parent_hwnd_val as *mut c_void);
        unsafe {
            let style = GetWindowLongW(parent_hwnd, GWL_STYLE) as u32;
            if (style & WS_CLIPCHILDREN.0) == 0 {
                SetWindowLongW(parent_hwnd, GWL_STYLE, (style | WS_CLIPCHILDREN.0) as i32);
            }
            // Revoke GPUI's default OLE IDropTarget on parent_hwnd so WM_DROPFILES (DragAcceptFiles)
            // reaches the native child ListViews when dragging from Windows Explorer.
            let _ = RevokeDragDrop(parent_hwnd);
        }

        let ctx_ptr = Box::into_raw(Box::new(SubclassContext {
            state: Arc::downgrade(&self.state),
            suppress_selection: Arc::clone(&self.suppress_selection_events),
            in_context_menu: Arc::clone(&self.in_context_menu),
        }));

        unsafe {
            let _ = SetWindowSubclass(
                parent_hwnd,
                Some(parent_subclass_proc),
                PARENT_SUBCLASS_ID,
                ctx_ptr as usize,
            );
        }

        let font = HFONT(font_val as *mut c_void);
        let left_hwnd = create_single_listview(parent_hwnd, LEFT_LISTVIEW_ID, font, ctx_ptr);
        let right_hwnd = create_single_listview(parent_hwnd, RIGHT_LISTVIEW_ID, font, ctx_ptr);

        let mut guard = self.state.lock().unwrap();
        guard.parent_hwnd = parent_hwnd_val;
        guard.subclass_ctx_ptr = ctx_ptr as usize;
        guard.left.hwnd = left_hwnd.0 as isize;
        guard.right.hwnd = right_hwnd.0 as isize;
    }
}

unsafe fn teardown_win32_controls(
    left_hwnd_val: isize,
    right_hwnd_val: isize,
    parent_hwnd_val: isize,
    subclass_ctx_ptr: usize,
) {
    unsafe {
        for child_val in [left_hwnd_val, right_hwnd_val] {
            if child_val != 0 {
                let child = HWND(child_val as *mut c_void);
                let _ = RemoveWindowSubclass(child, Some(child_subclass_proc), CHILD_SUBCLASS_ID);
                let _ = DestroyWindow(child);
            }
        }
        if parent_hwnd_val != 0 {
            let parent = HWND(parent_hwnd_val as *mut c_void);
            let _ = RemoveWindowSubclass(parent, Some(parent_subclass_proc), PARENT_SUBCLASS_ID);
        }
        if subclass_ctx_ptr != 0 {
            let _ = Box::from_raw(subclass_ctx_ptr as *mut SubclassContext);
        }
    }
}

impl Drop for HostInner {
    fn drop(&mut self) {
        unsafe {
            teardown_win32_controls(
                self.left.hwnd,
                self.right.hwnd,
                self.parent_hwnd,
                self.subclass_ctx_ptr,
            );
            self.left.hwnd = 0;
            self.right.hwnd = 0;
            self.parent_hwnd = 0;
            self.subclass_ctx_ptr = 0;
            if self.ui_font != 0 {
                let _ = DeleteObject(HGDIOBJ(self.ui_font as *mut c_void));
                self.ui_font = 0;
            }
        }
    }
}

struct SubclassContext {
    state: Weak<Mutex<HostInner>>,
    suppress_selection: Arc<AtomicBool>,
    in_context_menu: Arc<AtomicBool>,
}

fn create_message_font() -> isize {
    unsafe {
        let mut ncm = NONCLIENTMETRICSW {
            cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        if SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            ncm.cbSize,
            Some(&mut ncm as *mut _ as *mut c_void),
            Default::default(),
        )
        .is_ok()
        {
            let hfont = CreateFontIndirectW(&ncm.lfMessageFont);
            hfont.0 as isize
        } else {
            0
        }
    }
}

fn create_single_listview(
    parent_hwnd: HWND,
    control_id: usize,
    font: HFONT,
    ctx_ptr: *mut SubclassContext,
) -> HWND {
    let style = WINDOW_STYLE(
        WS_CHILD.0
            | WS_TABSTOP.0
            | LVS_REPORT
            | LVS_OWNERDATA
            | LVS_SHAREIMAGELISTS
            | LVS_SHOWSELALWAYS
            | LVS_EDITLABELS,
    );
    let hinstance = unsafe { GetModuleHandleW(None).unwrap_or_default() };
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            WC_LISTVIEWW,
            w!(""),
            style,
            0,
            0,
            100,
            100,
            Some(parent_hwnd),
            Some(HMENU(control_id as *mut c_void)),
            Some(hinstance.into()),
            None,
        )
        .unwrap_or_default()
    };
    if hwnd.0.is_null() {
        return hwnd;
    }

    unsafe {
        if !font.0.is_null() {
            send_msg(hwnd, WM_SETFONT, font.0 as usize, 1);
        }
        let ex_style = LVS_EX_DOUBLEBUFFER | LVS_EX_FULLROWSELECT | LVS_EX_HEADERDRAGDROP;
        send_msg(
            hwnd,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            ex_style as usize,
            ex_style as isize,
        );

        insert_initial_columns(hwnd);
        DragAcceptFiles(hwnd, true);
        let _ = SetWindowSubclass(
            hwnd,
            Some(child_subclass_proc),
            CHILD_SUBCLASS_ID,
            ctx_ptr as usize,
        );
    }

    hwnd
}

pub const MODIFIED_COLUMN_WIDTH_PX: i32 = 175;
pub const SIZE_COLUMN_WIDTH_PX: i32 = 100;

fn insert_initial_columns(hwnd: HWND) {
    let t = i18n();
    let cols = [
        (t.col_name(), 260, LVCFMT_LEFT),
        (t.col_modified(), MODIFIED_COLUMN_WIDTH_PX, LVCFMT_LEFT),
        (t.col_size(), SIZE_COLUMN_WIDTH_PX, LVCFMT_RIGHT),
    ];
    for (idx, (label, width, fmt)) in cols.into_iter().enumerate() {
        let mut wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
        let col = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH | LVCF_FMT,
            fmt,
            cx: width,
            pszText: PWSTR(wide.as_mut_ptr()),
            ..Default::default()
        };
        unsafe {
            send_msg(hwnd, LVM_INSERTCOLUMNW, idx, &col as *const _ as isize);
        }
    }
}

fn resize_columns_to_width(hwnd: HWND, total_width: i32) {
    let mod_w = MODIFIED_COLUMN_WIDTH_PX;
    let size_w = SIZE_COLUMN_WIDTH_PX;
    let scrollbar_pad = 22;
    let name_w = (total_width - mod_w - size_w - scrollbar_pad).max(120);
    unsafe {
        send_msg(hwnd, LVM_SETCOLUMNWIDTH, 0, name_w as isize);
        send_msg(hwnd, LVM_SETCOLUMNWIDTH, 1, mod_w as isize);
        send_msg(hwnd, LVM_SETCOLUMNWIDTH, 2, size_w as isize);
    }
}

unsafe fn apply_uxtheme_dark_mode(hwnd: HWND, header_hwnd: HWND, is_dark: bool) {
    unsafe {
        let hmod = GetModuleHandleW(w!("uxtheme.dll"))
            .or_else(|_| LoadLibraryW(w!("uxtheme.dll")))
            .unwrap_or_default();
        if hmod.is_invalid() {
            return;
        }

        // Ordinal 135: SetPreferredAppMode(mode: i32) -> i32 (2 = ForceDark, 3 = ForceLight)
        if let Some(proc) = GetProcAddress(hmod, windows::core::PCSTR(135usize as *const u8)) {
            let set_app_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(proc);
            let _ = set_app_mode(if is_dark { 2 } else { 3 });
        }
        // Ordinal 133: AllowDarkModeForWindow(hwnd: HWND, allow: bool) -> bool
        if let Some(proc) = GetProcAddress(hmod, windows::core::PCSTR(133usize as *const u8)) {
            let allow_dark: unsafe extern "system" fn(HWND, bool) -> bool =
                std::mem::transmute(proc);
            let _ = allow_dark(hwnd, is_dark);
            if !header_hwnd.0.is_null() {
                let _ = allow_dark(header_hwnd, is_dark);
            }
        }
        // Ordinal 136: FlushMenuThemes()
        if let Some(proc) = GetProcAddress(hmod, windows::core::PCSTR(136usize as *const u8)) {
            let flush_menus: unsafe extern "system" fn() = std::mem::transmute(proc);
            flush_menus();
        }
    }
}

fn apply_listview_theme(hwnd: HWND, is_dark: bool) {
    let (bg, fg) = if is_dark {
        (
            rgb_colorref(0x11, 0x13, 0x18),
            rgb_colorref(0xf1, 0xf5, 0xf9),
        )
    } else {
        (
            rgb_colorref(0xf8, 0xfa, 0xfc),
            rgb_colorref(0x0f, 0x17, 0x2a),
        )
    };

    unsafe {
        let header_hwnd = HWND(send_msg(hwnd, LVM_GETHEADER, 0, 0).0 as *mut c_void);
        apply_uxtheme_dark_mode(hwnd, header_hwnd, is_dark);

        let theme_name = if is_dark {
            w!("DarkMode_Explorer")
        } else {
            w!("Explorer")
        };
        let _ = SetWindowTheme(hwnd, theme_name, None);
        if !header_hwnd.0.is_null() {
            let header_theme = if is_dark {
                w!("DarkMode_ItemsView")
            } else {
                w!("ItemsView")
            };
            let _ = SetWindowTheme(header_hwnd, header_theme, None);
        }
        send_msg(hwnd, LVM_SETBKCOLOR, 0, bg.0 as isize);
        send_msg(hwnd, LVM_SETTEXTBKCOLOR, 0, bg.0 as isize);
        send_msg(hwnd, LVM_SETTEXTCOLOR, 0, fg.0 as isize);

        if !header_hwnd.0.is_null() {
            let _ = InvalidateRect(Some(header_hwnd), None, true);
        }
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

fn rgb_colorref(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

fn update_column_headers_and_sort(
    hwnd: HWND,
    labels: &(String, String, String),
    sort_col: SortColumn,
    ascending: bool,
) {
    let texts = [&labels.0, &labels.1, &labels.2];
    for (idx, text) in texts.iter().enumerate() {
        let mut wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let col = LVCOLUMNW {
            mask: LVCF_TEXT,
            pszText: PWSTR(wide.as_mut_ptr()),
            ..Default::default()
        };
        unsafe {
            send_msg(hwnd, LVM_SETCOLUMNW, idx, &col as *const _ as isize);
        }
    }

    unsafe {
        let header = HWND(send_msg(hwnd, LVM_GETHEADER, 0, 0).0 as *mut c_void);
        if header.0.is_null() {
            return;
        }
        let active_idx = match sort_col {
            SortColumn::Name => 0,
            SortColumn::Modified => 1,
            SortColumn::Size => 2,
        };
        for idx in 0..3 {
            let mut item = HDITEMW {
                mask: HDI_FORMAT,
                ..Default::default()
            };
            let _ = send_msg(header, HDM_GETITEMW, idx, &mut item as *mut _ as isize);
            let mut fmt_bits = item.fmt.0 & !(HDF_SORTUP.0 | HDF_SORTDOWN.0);
            if idx == 2 {
                fmt_bits |= HDF_RIGHT.0 | HDF_STRING.0;
            } else {
                fmt_bits |= HDF_LEFT.0 | HDF_STRING.0;
            }
            if idx == active_idx {
                fmt_bits |= if ascending {
                    HDF_SORTUP.0
                } else {
                    HDF_SORTDOWN.0
                };
            }
            item.fmt = HEADER_CONTROL_FORMAT_FLAGS(fmt_bits);
            let _ = send_msg(header, HDM_SETITEMW, idx, &item as *const _ as isize);
        }
        let _ = InvalidateRect(Some(header), None, true);
    }
}

fn sync_listview_selection(hwnd: HWND, rows: &[NativeRowViewModel], selected: &HashSet<&str>) {
    let mut first_selected: Option<usize> = None;
    for (idx, row) in rows.iter().enumerate() {
        let want_sel = !row.is_parent_row && selected.contains(row.path.as_str());
        if want_sel && first_selected.is_none() {
            first_selected = Some(idx);
        }
        let state = if want_sel {
            LVIS_SELECTED
        } else {
            Default::default()
        };
        let lvi = LVITEMW {
            stateMask: LVIS_SELECTED,
            state,
            ..Default::default()
        };
        unsafe {
            send_msg(hwnd, LVM_SETITEMSTATE, idx, &lvi as *const _ as isize);
        }
    }
    if let Some(first_idx) = first_selected {
        unsafe {
            send_msg(hwnd, LVM_ENSUREVISIBLE, first_idx, 0);
        }
    }
}

fn collect_selected_paths_from_hwnd(hwnd: HWND, rows: &[NativeRowViewModel]) -> Vec<String> {
    let mut out = Vec::new();
    let mut idx: isize = -1;
    loop {
        idx = unsafe { send_msg(hwnd, LVM_GETNEXTITEM, idx as usize, LVNI_SELECTED as isize).0 };
        if idx < 0 {
            break;
        }
        if let Some(row) = rows.get(idx as usize) {
            if !row.is_parent_row {
                out.push(row.path.clone());
            }
        }
    }
    out
}

unsafe extern "system" fn parent_subclass_proc(
    hwnd: HWND,
    umsg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uidsubclass: usize,
    dwrefdata: usize,
) -> LRESULT {
    unsafe {
        if matches!(umsg, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) {
            let _ = SetFocus(Some(hwnd));
        }
        if umsg == WM_NOTIFY && lparam.0 != 0 && dwrefdata != 0 {
            let ctx = &*(dwrefdata as *const SubclassContext);
            if let Some(state) = ctx.state.upgrade() {
                let hdr = &*(lparam.0 as *const NMHDR);
                let from_val = hdr.hwndFrom.0 as isize;

                let pane_side = {
                    let guard = state.lock().unwrap();
                    if from_val != 0 && from_val == guard.left.hwnd {
                        Some(true)
                    } else if from_val != 0 && from_val == guard.right.hwnd {
                        Some(false)
                    } else {
                        None
                    }
                };

                if let Some(is_local) = pane_side {
                    if let Some(res) = handle_listview_notify(
                        ctx,
                        &state,
                        hdr.hwndFrom,
                        is_local,
                        hdr.code,
                        lparam,
                    ) {
                        return res;
                    }
                }
            }
        }

        DefSubclassProc(hwnd, umsg, wparam, lparam)
    }
}

unsafe fn handle_listview_notify(
    ctx: &SubclassContext,
    state: &Arc<Mutex<HostInner>>,
    lv_hwnd: HWND,
    is_local: bool,
    code: u32,
    lparam: LPARAM,
) -> Option<LRESULT> {
    unsafe {
        match code {
            LVN_GETDISPINFOW => {
                let di = &mut *(lparam.0 as *mut NMLVDISPINFOW);
                let row_idx = di.item.iItem as usize;
                let sub_item = di.item.iSubItem;

                let mut guard = state.lock().unwrap();
                let row_opt = if is_local {
                    guard.left.rows.get(row_idx).cloned()
                } else {
                    guard.right.rows.get(row_idx).cloned()
                };

                if let Some(row) = row_opt {
                    if (di.item.mask & LVIF_TEXT) != Default::default()
                        && !di.item.pszText.is_null()
                        && di.item.cchTextMax > 0
                    {
                        let text = match sub_item {
                            0 => row.name.as_str(),
                            1 => row.modified_text.as_str(),
                            2 => row.size_text.as_str(),
                            _ => "",
                        };
                        let max_chars = (di.item.cchTextMax as usize).saturating_sub(1);
                        let mut count = 0usize;
                        let dst = di.item.pszText.0;
                        for unit in text.encode_utf16().take(max_chars) {
                            *dst.add(count) = unit;
                            count += 1;
                        }
                        *dst.add(count) = 0;
                    }

                    if sub_item == 0 && (di.item.mask & LVIF_IMAGE) != Default::default() {
                        let is_drive_or_bucket =
                            row.is_bucket || (row.is_dir && row.name.ends_with(':'));
                        di.item.iImage = guard.icon_cache.icon_index_for(
                            row.is_dir,
                            is_drive_or_bucket,
                            row.is_parent_row,
                            &row.extension,
                        );
                    }
                }
                Some(LRESULT(0))
            }

            LVN_ITEMCHANGED | LVN_ODSTATECHANGED => {
                if ctx.suppress_selection.load(Ordering::SeqCst) {
                    return Some(LRESULT(0));
                }
                if code == LVN_ITEMCHANGED {
                    let nmlv = &*(lparam.0 as *const NMLISTVIEW);
                    if (nmlv.uChanged & LVIF_STATE) == Default::default() {
                        return Some(LRESULT(0));
                    }
                }
                let rows = {
                    let guard = state.lock().unwrap();
                    if is_local {
                        Arc::clone(&guard.left.rows)
                    } else {
                        Arc::clone(&guard.right.rows)
                    }
                };
                let selected = collect_selected_paths_from_hwnd(lv_hwnd, &rows);
                let mut guard = state.lock().unwrap();
                let pane = if is_local {
                    &mut guard.left
                } else {
                    &mut guard.right
                };
                if pane.selected_paths != selected {
                    pane.selected_paths = selected.clone();
                    guard.events.push(NativePaneEvent::SelectionChanged {
                        is_local,
                        selected_paths: selected,
                    });
                }
                Some(LRESULT(0))
            }

            NM_DBLCLK => {
                let nmia = &*(lparam.0 as *const NMITEMACTIVATE);
                if nmia.iItem >= 0 {
                    let mut guard = state.lock().unwrap();
                    let (has_parent, entries) = if is_local {
                        (guard.left.has_parent, Arc::clone(&guard.left.entries))
                    } else {
                        (guard.right.has_parent, Arc::clone(&guard.right.entries))
                    };
                    match map_row_index_to_entry(nmia.iItem as usize, has_parent, &entries) {
                        Some(Err(())) => {
                            guard.events.push(NativePaneEvent::NavigateUp { is_local })
                        }
                        Some(Ok(entry)) => guard.events.push(NativePaneEvent::ActivateEntry {
                            is_local,
                            entry: entry.clone(),
                        }),
                        None => {}
                    }
                }
                Some(LRESULT(0))
            }

            LVN_COLUMNCLICK => {
                let nmlv = &*(lparam.0 as *const NMLISTVIEW);
                let col = match nmlv.iSubItem {
                    0 => Some(SortColumn::Name),
                    1 => Some(SortColumn::Modified),
                    2 => Some(SortColumn::Size),
                    _ => None,
                };
                if let Some(column) = col {
                    let mut guard = state.lock().unwrap();
                    guard
                        .events
                        .push(NativePaneEvent::SortChanged { is_local, column });
                }
                Some(LRESULT(0))
            }

            LVN_BEGINLABELEDITW => {
                let di = &*(lparam.0 as *const NMLVDISPINFOW);
                if di.item.iItem < 0 {
                    return Some(LRESULT(1));
                }
                let mut guard = state.lock().unwrap();
                let pane = if is_local {
                    &mut guard.left
                } else {
                    &mut guard.right
                };
                match map_row_index_to_entry(di.item.iItem as usize, pane.has_parent, &pane.entries)
                {
                    Some(Ok(entry)) if !matches!(entry.kind, crate::models::FileKind::Bucket) => {
                        pane.editing_original_path = Some(entry.path.clone());
                        Some(LRESULT(0))
                    }
                    _ => {
                        pane.editing_original_path = None;
                        Some(LRESULT(1))
                    }
                }
            }

            LVN_ENDLABELEDITW => {
                let di = &*(lparam.0 as *const NMLVDISPINFOW);
                let mut guard = state.lock().unwrap();
                let (tracked_path, has_parent, entries) = if is_local {
                    (
                        guard.left.editing_original_path.take(),
                        guard.left.has_parent,
                        Arc::clone(&guard.left.entries),
                    )
                } else {
                    (
                        guard.right.editing_original_path.take(),
                        guard.right.has_parent,
                        Arc::clone(&guard.right.entries),
                    )
                };
                if !di.item.pszText.is_null() {
                    let edited = di.item.pszText.to_string().unwrap_or_default();
                    let fallback_row = (di.item.iItem >= 0).then_some(di.item.iItem as usize);
                    if let Some(entry) = resolve_inline_rename_entry(
                        tracked_path.as_deref(),
                        fallback_row,
                        has_parent,
                        &entries,
                    ) {
                        if let Some(new_name) = should_commit_inline_rename(&entry.name, &edited) {
                            guard.events.push(NativePaneEvent::InlineRenameCommitted {
                                is_local,
                                original_path: entry.path.clone(),
                                new_name,
                            });
                            return Some(LRESULT(1));
                        }
                    }
                }
                Some(LRESULT(0))
            }

            LVN_KEYDOWN => {
                let kd = &*(lparam.0 as *const NMLVKEYDOWN);
                let vk = kd.wVKey;
                let ctrl_down = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;

                if ctrl_down && vk == VK_A.0 {
                    let lvi = LVITEMW {
                        stateMask: LVIS_SELECTED,
                        state: LVIS_SELECTED,
                        ..Default::default()
                    };
                    send_msg(
                        lv_hwnd,
                        LVM_SETITEMSTATE,
                        usize::MAX,
                        &lvi as *const _ as isize,
                    );
                    return Some(LRESULT(0));
                }

                if vk == VK_F2.0 {
                    let focused =
                        send_msg(lv_hwnd, LVM_GETNEXTITEM, usize::MAX, LVNI_FOCUSED as isize).0;
                    if focused >= 0 {
                        let _ = SetFocus(Some(lv_hwnd));
                        send_msg(lv_hwnd, LVM_EDITLABELW, focused as usize, 0);
                    }
                } else if vk == VK_BACK.0 {
                    let mut guard = state.lock().unwrap();
                    guard.events.push(NativePaneEvent::NavigateUp { is_local });
                } else if vk == VK_DELETE.0 {
                    let mut guard = state.lock().unwrap();
                    guard.events.push(NativePaneEvent::ContextMenuCommand {
                        is_local,
                        action: NativeContextMenuAction::DeleteSelected,
                    });
                } else if vk == VK_RETURN.0 {
                    let focused =
                        send_msg(lv_hwnd, LVM_GETNEXTITEM, usize::MAX, LVNI_FOCUSED as isize).0;
                    if focused >= 0 {
                        let mut guard = state.lock().unwrap();
                        let (has_parent, entries) = if is_local {
                            (guard.left.has_parent, Arc::clone(&guard.left.entries))
                        } else {
                            (guard.right.has_parent, Arc::clone(&guard.right.entries))
                        };
                        match map_row_index_to_entry(focused as usize, has_parent, &entries) {
                            Some(Err(())) => {
                                guard.events.push(NativePaneEvent::NavigateUp { is_local })
                            }
                            Some(Ok(entry)) => guard.events.push(NativePaneEvent::ActivateEntry {
                                is_local,
                                entry: entry.clone(),
                            }),
                            None => {}
                        }
                    }
                }
                Some(LRESULT(0))
            }

            NM_RCLICK => {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let packed_pt =
                    ((pt.y as i16 as u16 as usize) << 16) | (pt.x as i16 as u16 as usize);
                let _ = PostMessageW(
                    Some(lv_hwnd),
                    WM_APP_SHOW_CONTEXT_MENU,
                    WPARAM(usize::from(is_local)),
                    LPARAM(packed_pt as isize),
                );
                Some(LRESULT(1))
            }

            LVN_BEGINDRAG => {
                let nmlv = &*(lparam.0 as *const NMLISTVIEW);
                start_pane_drag(ctx, lv_hwnd, is_local, nmlv.iItem);
                Some(LRESULT(0))
            }

            _ => None,
        }
    }
}

unsafe extern "system" fn child_subclass_proc(
    hwnd: HWND,
    umsg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uidsubclass: usize,
    dwrefdata: usize,
) -> LRESULT {
    unsafe {
        if dwrefdata != 0 {
            let ctx = &*(dwrefdata as *const SubclassContext);
            if let Some(state) = ctx.state.upgrade() {
                let is_local = {
                    let guard = state.lock().unwrap();
                    hwnd.0 as isize == guard.left.hwnd
                };

                match umsg {
                    WM_NOTIFY if lparam.0 != 0 => {
                        let hdr = &*(lparam.0 as *const NMHDR);
                        if hdr.code == NM_CUSTOMDRAW {
                            let header_hwnd =
                                HWND(send_msg(hwnd, LVM_GETHEADER, 0, 0).0 as *mut c_void);
                            if !header_hwnd.0.is_null() && hdr.hwndFrom == header_hwnd {
                                return handle_header_custom_draw(
                                    &state,
                                    is_local,
                                    header_hwnd,
                                    lparam,
                                );
                            }
                        }
                    }
                    WM_DROPFILES => {
                        let hdrop = HDROP(wparam.0 as *mut c_void);
                        handle_wm_dropfiles(&state, hwnd, is_local, hdrop);
                        return LRESULT(0);
                    }
                    WM_CONTEXTMENU => {
                        let x = (lparam.0 & 0xFFFF) as i16 as i32;
                        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                        if x == -1 && y == -1 {
                            let mut pt = POINT::default();
                            let _ = GetCursorPos(&mut pt);
                            let packed_pt = ((pt.y as i16 as u16 as usize) << 16)
                                | (pt.x as i16 as u16 as usize);
                            let _ = PostMessageW(
                                Some(hwnd),
                                WM_APP_SHOW_CONTEXT_MENU,
                                WPARAM(usize::from(is_local)),
                                LPARAM(packed_pt as isize),
                            );
                            return LRESULT(0);
                        }
                    }
                    WM_APP_SHOW_CONTEXT_MENU => {
                        if ctx.in_context_menu.swap(true, Ordering::SeqCst) {
                            return LRESULT(0);
                        }
                        let _ = ReleaseCapture();
                        let x = (lparam.0 & 0xFFFF) as i16 as i32;
                        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                        let menu_is_local = wparam.0 != 0;
                        show_pane_context_menu(ctx, hwnd, menu_is_local, POINT { x, y });
                        ctx.in_context_menu.store(false, Ordering::SeqCst);
                        return LRESULT(0);
                    }
                    WM_MOUSEMOVE => {
                        let has_drag = state.lock().unwrap().active_pane_drag.is_some();
                        if has_drag {
                            if let Ok(cursor) = LoadCursorW(None, IDC_HAND) {
                                SetCursor(Some(cursor));
                            }
                            return LRESULT(0);
                        }
                    }
                    WM_LBUTTONUP => {
                        let drag_opt = state.lock().unwrap().active_pane_drag.take();
                        if let Some(drag) = drag_opt {
                            let _ = ReleaseCapture();
                            if let Ok(cursor) = LoadCursorW(None, IDC_ARROW) {
                                SetCursor(Some(cursor));
                            }
                            let mut drop_screen_pt = POINT {
                                x: (lparam.0 & 0xFFFF) as i16 as i32,
                                y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
                            };
                            let _ = ClientToScreen(hwnd, &mut drop_screen_pt);
                            finish_pane_drag(
                                &state,
                                drag.from_local,
                                drag.source_paths,
                                drop_screen_pt,
                            );
                            return LRESULT(0);
                        }
                    }
                    WM_CAPTURECHANGED => {
                        state.lock().unwrap().active_pane_drag = None;
                    }
                    _ => {}
                }
            }
        }

        DefSubclassProc(hwnd, umsg, wparam, lparam)
    }
}

unsafe fn handle_header_custom_draw(
    state: &Arc<Mutex<HostInner>>,
    is_local: bool,
    header_hwnd: HWND,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        let nmcd = &mut *(lparam.0 as *mut NMCUSTOMDRAW);
        let stage = nmcd.dwDrawStage;

        let (ui_font, is_dark, labels, sort_col, sort_asc) = {
            let guard = state.lock().unwrap();
            let pane = if is_local { &guard.left } else { &guard.right };
            (
                guard.ui_font,
                pane.is_dark.unwrap_or(true),
                pane.header_labels.clone(),
                pane.sort_col,
                pane.sort_ascending,
            )
        };

        let (bg_color, border_color) = if is_dark {
            (
                rgb_colorref(0x16, 0x19, 0x22),
                rgb_colorref(0x26, 0x2b, 0x38),
            )
        } else {
            (
                rgb_colorref(0xf1, 0xf5, 0xf9),
                rgb_colorref(0xe2, 0xe8, 0xf0),
            )
        };

        if stage == CDDS_PREPAINT {
            let mut client_rc = RECT::default();
            if GetClientRect(header_hwnd, &mut client_rc).is_ok() {
                let bg_brush = CreateSolidBrush(bg_color);
                let _ = FillRect(nmcd.hdc, &client_rc, bg_brush);
                let _ = DeleteObject(HGDIOBJ(bg_brush.0));

                let border_rc = RECT {
                    left: client_rc.left,
                    top: (client_rc.bottom - 1).max(client_rc.top),
                    right: client_rc.right,
                    bottom: client_rc.bottom,
                };
                let border_brush = CreateSolidBrush(border_color);
                let _ = FillRect(nmcd.hdc, &border_rc, border_brush);
                let _ = DeleteObject(HGDIOBJ(border_brush.0));
            }
            return LRESULT((CDRF_NOTIFYITEMDRAW | CDRF_NOTIFYPOSTPAINT) as isize);
        }

        if stage == CDDS_ITEMPREPAINT {
            let col_idx = nmcd.dwItemSpec;
            let rc = nmcd.rc;

            let bg_brush = CreateSolidBrush(bg_color);
            let _ = FillRect(nmcd.hdc, &rc, bg_brush);
            let _ = DeleteObject(HGDIOBJ(bg_brush.0));

            let border_brush = CreateSolidBrush(border_color);
            let bottom_rc = RECT {
                left: rc.left,
                top: (rc.bottom - 1).max(rc.top),
                right: rc.right,
                bottom: rc.bottom,
            };
            let _ = FillRect(nmcd.hdc, &bottom_rc, border_brush);

            let right_sep_rc = RECT {
                left: (rc.right - 1).max(rc.left),
                top: rc.top + 4,
                right: rc.right,
                bottom: (rc.bottom - 4).max(rc.top + 4),
            };
            let _ = FillRect(nmcd.hdc, &right_sep_rc, border_brush);
            let _ = DeleteObject(HGDIOBJ(border_brush.0));

            let active_idx = match sort_col {
                SortColumn::Name => 0,
                SortColumn::Modified => 1,
                SortColumn::Size => 2,
            };
            let base_label = match col_idx {
                0 => labels.0.as_str(),
                1 => labels.1.as_str(),
                2 => labels.2.as_str(),
                _ => "",
            };
            let full_text = if col_idx == active_idx {
                let arrow = if sort_asc { " ▲" } else { " ▼" };
                format!("{base_label}{arrow}")
            } else {
                base_label.to_string()
            };

            let fg_color = if is_dark {
                if col_idx == active_idx {
                    rgb_colorref(0xf8, 0xfa, 0xfc)
                } else {
                    rgb_colorref(0xe2, 0xe8, 0xf0)
                }
            } else if col_idx == active_idx {
                rgb_colorref(0x0f, 0x17, 0x2a)
            } else {
                rgb_colorref(0x33, 0x41, 0x55)
            };

            let prev_font = if ui_font != 0 {
                Some(SelectObject(nmcd.hdc, HGDIOBJ(ui_font as *mut c_void)))
            } else {
                None
            };
            let _ = SetBkMode(nmcd.hdc, TRANSPARENT);
            let _ = SetTextColor(nmcd.hdc, fg_color);

            let mut wide: Vec<u16> = full_text.encode_utf16().collect();
            let mut text_rc = RECT {
                left: rc.left + 8,
                top: rc.top,
                right: (rc.right - 8).max(rc.left + 8),
                bottom: (rc.bottom - 1).max(rc.top),
            };
            let align = if col_idx == 2 { DT_RIGHT } else { DT_LEFT };
            let _ = DrawTextW(
                nmcd.hdc,
                &mut wide,
                &mut text_rc,
                align | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
            );

            if let Some(old) = prev_font {
                let _ = SelectObject(nmcd.hdc, old);
            }

            return LRESULT(CDRF_SKIPDEFAULT as isize);
        }

        LRESULT(CDRF_DODEFAULT as isize)
    }
}

unsafe fn handle_wm_dropfiles(
    state: &Arc<Mutex<HostInner>>,
    lv_hwnd: HWND,
    target_is_local: bool,
    hdrop: HDROP,
) {
    unsafe {
        let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
        let mut dropped_paths = Vec::new();
        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            if len > 0 {
                let mut buf = vec![0u16; len + 1];
                let written = DragQueryFileW(hdrop, i, Some(&mut buf)) as usize;
                let path = String::from_utf16_lossy(&buf[..written]);
                if !path.is_empty() {
                    dropped_paths.push(path);
                }
            }
        }

        let mut pt = POINT::default();
        let _ = DragQueryPoint(hdrop, &mut pt);
        DragFinish(hdrop);

        if dropped_paths.is_empty() {
            return;
        }

        let mut hti = LVHITTESTINFO {
            pt,
            ..Default::default()
        };
        let hit_row = send_msg(lv_hwnd, LVM_HITTEST, 0, &mut hti as *mut _ as isize).0;

        let mut guard = state.lock().unwrap();
        let pane = if target_is_local {
            &guard.left
        } else {
            &guard.right
        };
        let hovered_entry = if hit_row >= 0 {
            match map_row_index_to_entry(hit_row as usize, pane.has_parent, &pane.entries) {
                Some(Ok(e)) => Some(e),
                _ => None,
            }
        } else {
            None
        };
        let dest_dir =
            resolve_drop_destination_dir(&pane.current_path, hovered_entry, target_is_local);

        guard.events.push(NativePaneEvent::FilesDropped {
            target_is_local,
            source_paths: dropped_paths,
            target_dir_override: Some(dest_dir),
        });
    }
}

unsafe fn start_pane_drag(ctx: &SubclassContext, lv_hwnd: HWND, is_local: bool, dragged_row: i32) {
    unsafe {
        let Some(state) = ctx.state.upgrade() else {
            return;
        };
        let (current_path, rows, has_parent, entries) = {
            let guard = state.lock().unwrap();
            let pane = if is_local { &guard.left } else { &guard.right };
            (
                pane.current_path.clone(),
                Arc::clone(&pane.rows),
                pane.has_parent,
                Arc::clone(&pane.entries),
            )
        };

        if is_local && current_path.trim() == "/" {
            return;
        }

        let selected_paths = collect_selected_paths_from_hwnd(lv_hwnd, &rows);
        let source_paths =
            resolve_drag_source_paths(&selected_paths, dragged_row, has_parent, &entries);
        if source_paths.is_empty() {
            return;
        }

        state.lock().unwrap().active_pane_drag = Some(ActivePaneDrag {
            from_local: is_local,
            source_paths,
        });
        SetCapture(lv_hwnd);
        if let Ok(cursor) = LoadCursorW(None, IDC_HAND) {
            SetCursor(Some(cursor));
        }
    }
}

#[cfg(test)]
struct PidlListGuard(Vec<*mut ITEMIDLIST>);

#[cfg(test)]
impl PidlListGuard {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn push(&mut self, pidl: *mut ITEMIDLIST) {
        if !pidl.is_null() {
            self.0.push(pidl);
        }
    }
}

#[cfg(test)]
impl Drop for PidlListGuard {
    fn drop(&mut self) {
        unsafe {
            for pidl in self.0.drain(..) {
                if !pidl.is_null() {
                    ILFree(Some(pidl as *const ITEMIDLIST));
                }
            }
        }
    }
}

unsafe fn finish_pane_drag(
    state: &Arc<Mutex<HostInner>>,
    from_local: bool,
    source_paths: Vec<String>,
    screen_pt: POINT,
) {
    unsafe {
        let target_is_local = !from_local;
        let (target_hwnd, target_visible) = {
            let guard = state.lock().unwrap();
            let pane = if target_is_local {
                &guard.left
            } else {
                &guard.right
            };
            (HWND(pane.hwnd as *mut c_void), pane.visible)
        };
        if target_hwnd.0.is_null() || !target_visible {
            return;
        }

        let mut client_pt = screen_pt;
        let _ = ScreenToClient(target_hwnd, &mut client_pt);
        let mut client_rc = RECT::default();
        if GetClientRect(target_hwnd, &mut client_rc).is_err() {
            return;
        }
        if client_pt.x < client_rc.left
            || client_pt.x >= client_rc.right
            || client_pt.y < client_rc.top
            || client_pt.y >= client_rc.bottom
        {
            return;
        }

        let mut hti = LVHITTESTINFO {
            pt: client_pt,
            ..Default::default()
        };
        let hit_row = send_msg(target_hwnd, LVM_HITTEST, 0, &mut hti as *mut _ as isize).0;

        let mut guard = state.lock().unwrap();
        let pane = if target_is_local {
            &guard.left
        } else {
            &guard.right
        };
        let hovered_entry = if hit_row >= 0 {
            match map_row_index_to_entry(hit_row as usize, pane.has_parent, &pane.entries) {
                Some(Ok(e)) => Some(e),
                _ => None,
            }
        } else {
            None
        };
        let dest_dir =
            resolve_drop_destination_dir(&pane.current_path, hovered_entry, target_is_local);
        if target_is_local && dest_dir.trim() == "/" {
            return;
        }

        guard.events.push(NativePaneEvent::FilesDropped {
            target_is_local,
            source_paths,
            target_dir_override: Some(dest_dir),
        });
    }
}

unsafe fn show_pane_context_menu(
    ctx: &SubclassContext,
    lv_hwnd: HWND,
    is_local: bool,
    screen_pt: POINT,
) {
    unsafe {
        let Some(state) = ctx.state.upgrade() else {
            return;
        };
        let (parent_hwnd_val, current_path, rows, entries) = {
            let guard = state.lock().unwrap();
            let pane = if is_local { &guard.left } else { &guard.right };
            (
                guard.parent_hwnd,
                pane.current_path.clone(),
                Arc::clone(&pane.rows),
                Arc::clone(&pane.entries),
            )
        };
        let selected_paths = collect_selected_paths_from_hwnd(lv_hwnd, &rows);
        let single_is_bucket = selected_paths.len() == 1
            && entries.iter().any(|e| {
                e.path == selected_paths[0] && matches!(e.kind, crate::models::FileKind::Bucket)
            });
        let can_reveal = is_local && can_use_shell_context_menu(&current_path, &selected_paths);
        let owner_hwnd = if parent_hwnd_val != 0 {
            HWND(parent_hwnd_val as *mut c_void)
        } else {
            lv_hwnd
        };

        show_standard_native_hmenu(
            ctx,
            lv_hwnd,
            owner_hwnd,
            is_local,
            !selected_paths.is_empty(),
            selected_paths.len() == 1 && !single_is_bucket,
            !single_is_bucket,
            can_reveal,
            screen_pt,
        );
    }
}

#[cfg(test)]
unsafe fn resolve_shell_menu_pidls(
    parent_dir: &str,
    selected_paths: &[String],
) -> anyhow::Result<(IShellFolder, Vec<*const ITEMIDLIST>, PidlListGuard)> {
    unsafe {
        if selected_paths.is_empty() {
            return Err(anyhow::anyhow!("empty selected_paths"));
        }

        let mut pidl_guard = PidlListGuard::new();
        let mut child_pidls: Vec<*const ITEMIDLIST> = Vec::new();

        if selected_paths.len() == 1 {
            let wpath: Vec<u16> = selected_paths[0]
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let mut pidl: *mut ITEMIDLIST = null_mut();
            SHParseDisplayName(PCWSTR(wpath.as_ptr()), None, &mut pidl, 0, None)?;
            pidl_guard.push(pidl);

            let mut child_last: *mut ITEMIDLIST = null_mut();
            let folder: IShellFolder = SHBindToParent(pidl, Some(&mut child_last))?;
            child_pidls.push(child_last as *const ITEMIDLIST);
            Ok((folder, child_pidls, pidl_guard))
        } else {
            let wparent: Vec<u16> = parent_dir
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let mut parent_pidl: *mut ITEMIDLIST = null_mut();
            SHParseDisplayName(PCWSTR(wparent.as_ptr()), None, &mut parent_pidl, 0, None)?;
            pidl_guard.push(parent_pidl);

            let folder: IShellFolder = SHBindToObject(None, parent_pidl, None)?;
            for path in selected_paths {
                let wpath: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
                let mut pidl: *mut ITEMIDLIST = null_mut();
                SHParseDisplayName(PCWSTR(wpath.as_ptr()), None, &mut pidl, 0, None)?;
                if pidl.is_null() {
                    return Err(anyhow::anyhow!("null pidl for {path}"));
                }
                pidl_guard.push(pidl);
                child_pidls.push(ILFindLastID(pidl) as *const ITEMIDLIST);
            }
            Ok((folder, child_pidls, pidl_guard))
        }
    }
}

unsafe fn show_standard_native_hmenu(
    ctx: &SubclassContext,
    lv_hwnd: HWND,
    owner_hwnd: HWND,
    is_local: bool,
    has_selection: bool,
    can_rename: bool,
    can_transfer: bool,
    can_reveal: bool,
    screen_pt: POINT,
) {
    unsafe {
        let Ok(hmenu) = CreatePopupMenu() else {
            return;
        };
        let t = i18n();
        let transfer_label = if is_local {
            t.ctx_upload_to_remote()
        } else {
            t.ctx_download_to_local()
        };

        append_menu_str(
            hmenu,
            CMD_TRANSFER,
            transfer_label,
            has_selection && can_transfer,
        );
        append_menu_str(hmenu, CMD_OPEN, t.ctx_open_or_enter(), has_selection);
        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR(null()));
        append_menu_str(hmenu, CMD_NEW_FOLDER, t.ctx_new_folder(), true);
        append_menu_str(hmenu, CMD_RENAME, t.ctx_rename(), can_rename);
        append_menu_str(hmenu, CMD_DELETE, t.ctx_delete(), has_selection);
        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR(null()));
        append_menu_str(hmenu, CMD_COPY_PATH, t.ctx_copy_path(), has_selection);
        if is_local {
            append_menu_str(
                hmenu,
                CMD_REVEAL_IN_SHELL,
                t.ctx_reveal_in_shell(),
                can_reveal,
            );
        }
        append_menu_str(hmenu, CMD_REFRESH, t.ctx_refresh(), true);

        let _ = SetForegroundWindow(owner_hwnd);
        let cmd = TrackPopupMenuEx(
            hmenu,
            (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN).0,
            screen_pt.x,
            screen_pt.y,
            owner_hwnd,
            None,
        )
        .0 as u32;
        let _ = PostMessageW(Some(owner_hwnd), WM_NULL, WPARAM(0), LPARAM(0));

        let _ = DestroyMenu(hmenu);
        if cmd != 0 {
            dispatch_custom_menu_cmd(ctx, lv_hwnd, is_local, cmd);
        }
    }
}

unsafe fn append_menu_str(hmenu: HMENU, id: u32, label: &str, enabled: bool) {
    unsafe {
        let wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
        let flags = if enabled {
            MF_STRING
        } else {
            MF_STRING | MF_GRAYED
        };
        let _ = AppendMenuW(hmenu, flags, id as usize, PCWSTR(wide.as_ptr()));
    }
}

unsafe fn dispatch_custom_menu_cmd(ctx: &SubclassContext, lv_hwnd: HWND, is_local: bool, cmd: u32) {
    unsafe {
        if cmd == CMD_RENAME {
            let sel = send_msg(lv_hwnd, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize).0;
            if sel >= 0 {
                let _ = SetFocus(Some(lv_hwnd));
                send_msg(lv_hwnd, LVM_EDITLABELW, sel as usize, 0);
                return;
            }
        }

        let action = match cmd {
            CMD_TRANSFER => Some(NativeContextMenuAction::TransferSelected),
            CMD_OPEN => Some(NativeContextMenuAction::OpenSelected),
            CMD_NEW_FOLDER => Some(NativeContextMenuAction::NewFolder),
            CMD_DELETE => Some(NativeContextMenuAction::DeleteSelected),
            CMD_COPY_PATH => Some(NativeContextMenuAction::CopySelectedPath),
            CMD_REVEAL_IN_SHELL => Some(NativeContextMenuAction::RevealInShell),
            CMD_REFRESH => Some(NativeContextMenuAction::Refresh),
            _ => None,
        };

        if let (Some(act), Some(state)) = (action, ctx.state.upgrade()) {
            state
                .lock()
                .unwrap()
                .events
                .push(NativePaneEvent::ContextMenuCommand {
                    is_local,
                    action: act,
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_use_shell_context_menu_guards_virtual_drive_root() {
        assert!(!can_use_shell_context_menu("/", &[]));
        assert!(!can_use_shell_context_menu("/", &[r"C:\".to_string()]));
        assert!(!can_use_shell_context_menu(r"C:\Users", &[]));
        assert!(can_use_shell_context_menu(
            r"C:\Users",
            &[r"C:\Users\Admin".to_string()]
        ));
    }

    #[test]
    fn test_system_icon_cache_resolves_valid_indices_for_dir_and_files() {
        let mut cache = ShellIconCache::new();
        let dir_icon = cache.icon_index_for(true, false, true, "");
        let txt_icon = cache.icon_index_for(false, false, false, "txt");
        let exe_icon = cache.icon_index_for(false, false, false, "exe");
        assert!(dir_icon >= 0);
        assert!(txt_icon >= 0);
        assert!(exe_icon >= 0);
    }

    #[test]
    fn test_ole_clipboard_initialized_for_drag_drop_on_fresh_thread() {
        let ok = std::thread::spawn(|| {
            let _host = PlatformHost::new();
            unsafe { windows::Win32::System::Ole::OleGetClipboard().is_ok() }
        })
        .join()
        .unwrap();
        assert!(ok, "PlatformHost::new must initialize OLE on the UI thread");
    }

    #[test]
    fn test_platform_host_teardown_releases_weak_state_and_syncs_unlocked() {
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap_or_default();
            let dummy_parent = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("rustduck_test_parent"),
                WS_POPUP,
                0,
                0,
                400,
                300,
                None,
                None,
                Some(hinstance.into()),
                None,
            )
            .expect("dummy parent HWND");

            let weak = {
                let host = PlatformHost::new();
                let weak = Arc::downgrade(&host.state);
                let snap = NativePaneSnapshot {
                    tab_idx: 0,
                    is_local: true,
                    current_path: r"C:\".to_string(),
                    bounds_px: (0, 0, 360, 240),
                    scale_factor: 1.0,
                    visible: true,
                    is_dark: true,
                    sort_col: SortColumn::Name,
                    sort_ascending: true,
                    has_parent: false,
                    entries_revision: 1,
                    rows: Arc::new(Vec::new()),
                    entries: Arc::new(Vec::new()),
                    selected_paths: Vec::new(),
                };
                host.sync_pane_for_parent_hwnd(dummy_parent.0 as isize, &snap);
                assert!(weak.upgrade().is_some());
                weak
            };

            assert!(
                weak.upgrade().is_none(),
                "Dropping PlatformHost must release HostInner without SubclassContext Arc cycles"
            );
            let _ = DestroyWindow(dummy_parent);
        }
    }

    #[test]
    fn test_pidl_guard_frees_allocated_pidls_when_bind_or_parse_fails_early() {
        let tmp = std::env::temp_dir();
        let valid_path = tmp.to_string_lossy().to_string();
        let invalid_path = r"Z:\__rustduck_nonexistent_drive_and_file_99999__".to_string();

        let _host = PlatformHost::new();
        let res =
            unsafe { resolve_shell_menu_pidls(&valid_path, &[valid_path.clone(), invalid_path]) };
        assert!(res.is_err());

        let single_ok =
            unsafe { resolve_shell_menu_pidls(&valid_path, std::slice::from_ref(&valid_path)) };
        assert!(single_ok.is_ok());
    }

    #[test]
    fn test_header_custom_draw_renders_light_text_in_dark_mode_on_light_os() {
        use windows::Win32::Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
            DIB_RGB_COLORS, DeleteDC, SelectObject,
        };
        use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindow};

        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap_or_default();
            let dummy_parent = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("rustduck_test_header_theme"),
                WS_POPUP,
                0,
                0,
                500,
                300,
                None,
                None,
                Some(hinstance.into()),
                None,
            )
            .expect("dummy parent HWND");
            let _ = ShowWindow(dummy_parent, SW_SHOWNOACTIVATE);

            let host = PlatformHost::new();
            let mut snap = NativePaneSnapshot {
                tab_idx: 0,
                is_local: true,
                current_path: r"C:\".to_string(),
                bounds_px: (0, 0, 480, 260),
                scale_factor: 1.0,
                visible: true,
                is_dark: true,
                sort_col: SortColumn::Name,
                sort_ascending: true,
                has_parent: false,
                entries_revision: 1,
                rows: Arc::new(Vec::new()),
                entries: Arc::new(Vec::new()),
                selected_paths: Vec::new(),
            };
            host.sync_pane_for_parent_hwnd(dummy_parent.0 as isize, &snap);

            let lv_hwnd = HWND(host.state.lock().unwrap().left.hwnd as *mut c_void);
            let header_hwnd = HWND(send_msg(lv_hwnd, LVM_GETHEADER, 0, 0).0 as *mut c_void);
            assert!(!header_hwnd.0.is_null());

            let w = 240i32;
            let h = 26i32;
            let hdc_screen = windows::Win32::Graphics::Gdi::GetDC(Some(header_hwnd));
            let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = null_mut();
            let hbm = CreateDIBSection(Some(hdc_mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
                .expect("CreateDIBSection");
            let old_obj = SelectObject(hdc_mem, HGDIOBJ(hbm.0));

            // Render dark-mode header via WM_PRINTCLIENT (PRF_CLIENT | PRF_ERASEBKGND = 6)
            const WM_PRINTCLIENT: u32 = 0x0318;
            let _ = send_msg(header_hwnd, WM_PRINTCLIENT, hdc_mem.0 as usize, 6);

            let slice = std::slice::from_raw_parts(bits as *const u32, (w * h) as usize);
            let mut dark_bg_pixels = 0usize;
            let mut bright_text_pixels = 0usize;
            for y in 2..(h - 2) {
                for x in 4..(w - 4) {
                    let px = slice[(y * w + x) as usize] & 0x00FF_FFFF;
                    let r = ((px >> 16) & 0xFF) as u8;
                    let g = ((px >> 8) & 0xFF) as u8;
                    let b = (px & 0xFF) as u8;
                    if r == 0x16 && g == 0x19 && b == 0x22 {
                        dark_bg_pixels += 1;
                    }
                    if r > 180 && g > 180 && b > 180 {
                        bright_text_pixels += 1;
                    }
                }
            }

            // Now switch to Light Mode and verify light background + dark text pixels
            snap.is_dark = false;
            host.sync_pane_for_parent_hwnd(dummy_parent.0 as isize, &snap);
            let _ = send_msg(header_hwnd, WM_PRINTCLIENT, hdc_mem.0 as usize, 6);
            let mut light_bg_pixels = 0usize;
            let mut dark_text_pixels = 0usize;
            for y in 2..(h - 2) {
                for x in 4..(w - 4) {
                    let px = slice[(y * w + x) as usize] & 0x00FF_FFFF;
                    let r = ((px >> 16) & 0xFF) as u8;
                    let g = ((px >> 8) & 0xFF) as u8;
                    let b = (px & 0xFF) as u8;
                    if r == 0xf1 && g == 0xf5 && b == 0xf9 {
                        light_bg_pixels += 1;
                    }
                    if r < 60 && g < 60 && b < 60 {
                        dark_text_pixels += 1;
                    }
                }
            }

            let _ = SelectObject(hdc_mem, old_obj);
            let _ = DeleteObject(HGDIOBJ(hbm.0));
            let _ = DeleteDC(hdc_mem);
            windows::Win32::Graphics::Gdi::ReleaseDC(Some(header_hwnd), hdc_screen);
            let _ = DestroyWindow(dummy_parent);

            assert!(
                dark_bg_pixels > 100 && bright_text_pixels > 10,
                "Expected dark header bg (#161922, got {dark_bg_pixels}) and bright text pixels (got {bright_text_pixels})"
            );
            assert!(
                light_bg_pixels > 100 && dark_text_pixels > 10,
                "Expected light header bg (#f1f5f9, got {light_bg_pixels}) and dark text pixels (got {dark_text_pixels})"
            );
        }
    }

    #[test]
    fn test_nm_rclick_defers_context_menu_and_returns_nonzero_without_blocking() {
        use windows::Win32::UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW};

        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap_or_default();
            let dummy_parent = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("rustduck_test_rclick"),
                WS_POPUP,
                0,
                0,
                400,
                300,
                None,
                None,
                Some(hinstance.into()),
                None,
            )
            .expect("dummy parent HWND");

            let host = PlatformHost::new();
            let snap = NativePaneSnapshot {
                tab_idx: 0,
                is_local: true,
                current_path: r"C:\".to_string(),
                bounds_px: (0, 0, 360, 240),
                scale_factor: 1.0,
                visible: true,
                is_dark: true,
                sort_col: SortColumn::Name,
                sort_ascending: true,
                has_parent: false,
                entries_revision: 1,
                rows: Arc::new(Vec::new()),
                entries: Arc::new(Vec::new()),
                selected_paths: Vec::new(),
            };
            host.sync_pane_for_parent_hwnd(dummy_parent.0 as isize, &snap);
            let lv_hwnd = HWND(host.state.lock().unwrap().left.hwnd as *mut c_void);

            let nmia = NMITEMACTIVATE {
                hdr: NMHDR {
                    hwndFrom: lv_hwnd,
                    idFrom: LEFT_LISTVIEW_ID,
                    code: NM_RCLICK,
                },
                iItem: -1,
                iSubItem: 0,
                ..Default::default()
            };
            let res = send_msg(
                dummy_parent,
                WM_NOTIFY,
                LEFT_LISTVIEW_ID,
                &nmia as *const _ as isize,
            );
            let mut msg = MSG::default();
            let queued = PeekMessageW(
                &mut msg,
                Some(lv_hwnd),
                WM_APP_SHOW_CONTEXT_MENU,
                WM_APP_SHOW_CONTEXT_MENU,
                PM_REMOVE,
            )
            .as_bool();
            let _ = DestroyWindow(dummy_parent);

            assert_eq!(
                res.0, 1,
                "NM_RCLICK must return nonzero (1) immediately and defer popup menu via PostMessageW"
            );
            assert!(
                queued,
                "NM_RCLICK must post WM_APP_SHOW_CONTEXT_MENU to lv_hwnd so WM_RBUTTONDOWN unwinds first"
            );
        }
    }

    #[test]
    fn test_resolve_drag_source_paths_handles_unselected_row_multi_select_and_parent_row() {
        use crate::models::FileKind;

        let entries = vec![
            FileEntry {
                name: "a.txt".into(),
                path: r"C:\work\a.txt".into(),
                kind: FileKind::File,
                size: 10,
                modified_unix: None,
                permissions: None,
            },
            FileEntry {
                name: "b.txt".into(),
                path: r"C:\work\b.txt".into(),
                kind: FileKind::File,
                size: 20,
                modified_unix: None,
                permissions: None,
            },
            FileEntry {
                name: "my-bucket".into(),
                path: "/my-bucket".into(),
                kind: FileKind::Bucket,
                size: 0,
                modified_unix: None,
                permissions: None,
            },
        ];

        // Row 0 is ".." when has_parent == true -> must not drag anything even if a.txt is selected
        assert!(
            resolve_drag_source_paths(&[r"C:\work\a.txt".to_string()], 0, true, &entries)
                .is_empty()
        );

        // Dragging unselected row 2 ("b.txt" when has_parent == true) while "a.txt" was selected -> drags "b.txt"
        assert_eq!(
            resolve_drag_source_paths(&[r"C:\work\a.txt".to_string()], 2, true, &entries),
            vec![r"C:\work\b.txt".to_string()]
        );

        // Dragging selected row 1 ("a.txt" when has_parent == true) in a multi-selection -> drags entire selection
        let multi = vec![r"C:\work\a.txt".to_string(), r"C:\work\b.txt".to_string()];
        assert_eq!(resolve_drag_source_paths(&multi, 1, true, &entries), multi);

        // Dragging a bucket row (row 3 when has_parent == true) -> must not drag
        assert!(resolve_drag_source_paths(&[], 3, true, &entries).is_empty());
    }

    #[test]
    fn test_left_to_right_drag_and_drop_emits_files_dropped_for_remote_pane() {
        use super::super::build_native_row_models;
        use crate::models::FileKind;
        use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindow};

        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap_or_default();
            let dummy_parent = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("rustduck_test_left_to_right_dnd"),
                WS_POPUP,
                0,
                0,
                800,
                400,
                None,
                None,
                Some(hinstance.into()),
                None,
            )
            .expect("dummy parent HWND");
            let _ = ShowWindow(dummy_parent, SW_SHOWNOACTIVATE);

            let host = PlatformHost::new();
            let local_entries = Arc::new(vec![FileEntry {
                name: "upload_me.txt".into(),
                path: r"C:\Users\Admin\upload_me.txt".into(),
                kind: FileKind::File,
                size: 512,
                modified_unix: None,
                permissions: None,
            }]);
            let local_rows = Arc::new(build_native_row_models(&local_entries, true, ".."));
            let left_snap = NativePaneSnapshot {
                tab_idx: 0,
                is_local: true,
                current_path: r"C:\Users\Admin".to_string(),
                bounds_px: (0, 0, 380, 360),
                scale_factor: 1.0,
                visible: true,
                is_dark: true,
                sort_col: SortColumn::Name,
                sort_ascending: true,
                has_parent: true,
                entries_revision: 1,
                rows: local_rows,
                entries: local_entries,
                selected_paths: vec![r"C:\Users\Admin\upload_me.txt".to_string()],
            };

            let remote_entries = Arc::new(vec![FileEntry {
                name: "releases".into(),
                path: "/prod-bucket/releases".into(),
                kind: FileKind::Directory,
                size: 0,
                modified_unix: None,
                permissions: None,
            }]);
            let remote_rows = Arc::new(build_native_row_models(&remote_entries, true, ".."));
            let right_snap = NativePaneSnapshot {
                tab_idx: 0,
                is_local: false,
                current_path: "/prod-bucket".to_string(),
                bounds_px: (400, 0, 380, 360),
                scale_factor: 1.0,
                visible: true,
                is_dark: true,
                sort_col: SortColumn::Name,
                sort_ascending: true,
                has_parent: true,
                entries_revision: 1,
                rows: remote_rows,
                entries: remote_entries,
                selected_paths: Vec::new(),
            };

            host.sync_pane_for_parent_hwnd(dummy_parent.0 as isize, &left_snap);
            host.sync_pane_for_parent_hwnd(dummy_parent.0 as isize, &right_snap);
            let _ = host.drain_events();

            let left_hwnd = HWND(host.state.lock().unwrap().left.hwnd as *mut c_void);

            // Begin drag on row 1 ("upload_me.txt", since row 0 is "..") in the left local pane
            let nmlv = NMLISTVIEW {
                hdr: NMHDR {
                    hwndFrom: left_hwnd,
                    idFrom: LEFT_LISTVIEW_ID,
                    code: LVN_BEGINDRAG,
                },
                iItem: 1,
                iSubItem: 0,
                ..Default::default()
            };
            let _ = send_msg(
                dummy_parent,
                WM_NOTIFY,
                LEFT_LISTVIEW_ID,
                &nmlv as *const _ as isize,
            );

            // Release mouse button at x=500, y=200 (relative to left_hwnd at x=0, y=0 -> hits right_hwnd at x=100, y=200)
            let drop_x: i16 = 500;
            let drop_y: i16 = 200;
            let packed_lparam = ((drop_y as u16 as usize) << 16) | (drop_x as u16 as usize);
            let _ = send_msg(left_hwnd, WM_LBUTTONUP, 0, packed_lparam as isize);

            let events = host.drain_events();
            let _ = DestroyWindow(dummy_parent);

            assert_eq!(
                events,
                vec![NativePaneEvent::FilesDropped {
                    target_is_local: false,
                    source_paths: vec![r"C:\Users\Admin\upload_me.txt".to_string()],
                    target_dir_override: Some("/prod-bucket".to_string()),
                }],
                "Dragging a file from the left local pane and releasing over the right remote pane must emit FilesDropped(target_is_local: false)"
            );
        }
    }

    #[test]
    fn test_modified_column_width_fits_timestamp() {
        assert!(
            MODIFIED_COLUMN_WIDTH_PX >= 170,
            "Modified column default width ({MODIFIED_COLUMN_WIDTH_PX}px) must be wide enough for YYYY-MM-DD HH:mm timestamps"
        );
    }
}
