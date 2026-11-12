use super::{
    MAX_SAFE_PANE_POINTS, NativeContextMenuAction, NativeEventSink, NativePaneEvent,
    NativePaneSnapshot, NativeRowViewModel, collect_paths_from_index_set, compute_appkit_frame,
    map_row_index_to_entry, resolve_drop_destination_dir, resolve_inline_rename_entry,
    should_commit_inline_rename,
};
use crate::i18n::i18n;
use crate::models::FileEntry;
use crate::ui::file_pane::SortColumn;
use futures::channel::mpsc::UnboundedReceiver;
use objc2::rc::autoreleasepool;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{class, msg_send, sel};
use objc2_app_kit::{
    NSDragOperation, NSFocusRingType, NSTableViewColumnAutoresizingStyle, NSTableViewStyle,
    NSWindowOrderingMode,
};
use objc2_foundation::{
    NSArray, NSInteger, NSNotFound, NSPoint, NSRect, NSSize, NSString, NSUInteger, NSURL,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, c_void};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once, Weak};

const DELEGATE_IVAR: &CStr = c"rustduckDelegateCtx";
const RUSTDUCK_LOCAL_PB_TYPE: &str = "app.rustduck.local-paths";
const RUSTDUCK_REMOTE_PB_TYPE: &str = "app.rustduck.remote-paths";
const NS_VIEW_WIDTH_SIZABLE_MASK: NSUInteger = 2;

static REGISTER_CLASSES: Once = Once::new();
static DELEGATE_CLASS: AtomicUsize = AtomicUsize::new(0);
static TABLE_VIEW_CLASS: AtomicUsize = AtomicUsize::new(0);

struct SelectionSuppressGuard<'a>(&'a AtomicBool);

impl<'a> SelectionSuppressGuard<'a> {
    fn enter(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::SeqCst);
        Self(flag)
    }
}

impl Drop for SelectionSuppressGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

struct MacPaneState {
    scroll_view: usize,
    table_view: usize,
    delegate_obj: usize,
    visible: bool,
    bounds_px: (i32, i32, i32, i32),
    last_frame_quantized: (i64, i64, i64, i64),
    scale_factor: f32,
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

impl MacPaneState {
    fn new() -> Self {
        Self {
            scroll_view: 0,
            table_view: 0,
            delegate_obj: 0,
            visible: false,
            bounds_px: (0, 0, 0, 0),
            last_frame_quantized: (0, 0, 0, 0),
            scale_factor: 1.0,
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

    fn take_hide_scroll_view(&mut self) -> Option<usize> {
        if self.visible && self.scroll_view != 0 {
            self.visible = false;
            Some(self.scroll_view)
        } else {
            None
        }
    }
}

struct HostInner {
    root_ns_view: usize,
    host_ns_view: usize,
    left: MacPaneState,
    right: MacPaneState,
    icon_cache: HashMap<String, usize>,
    events: NativeEventSink,
}

struct DelegateContext {
    is_local: bool,
    state: Weak<Mutex<HostInner>>,
    suppress_selection: Arc<AtomicBool>,
}

pub struct PlatformHost {
    state: Arc<Mutex<HostInner>>,
    suppress_selection: Arc<AtomicBool>,
}

impl Default for PlatformHost {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformHost {
    pub fn new() -> Self {
        ensure_objc_classes();
        Self {
            state: Arc::new(Mutex::new(HostInner {
                root_ns_view: 0,
                host_ns_view: 0,
                left: MacPaneState::new(),
                right: MacPaneState::new(),
                icon_cache: HashMap::new(),
                events: NativeEventSink::default(),
            })),
            suppress_selection: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn sync_pane(&self, window: &gpui::Window, snapshot: &NativePaneSnapshot) {
        let Ok(raw_handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(appkit_handle) = raw_handle.as_raw() else {
            return;
        };
        let root_view_ptr = appkit_handle.ns_view.as_ptr() as usize;
        if root_view_ptr == 0 {
            return;
        }

        autoreleasepool(|_| {
            self.ensure_views_created(root_view_ptr);

            let mut guard = self.state.lock().unwrap();
            let host_view_ptr = if guard.host_ns_view != 0 {
                guard.host_ns_view
            } else {
                root_view_ptr
            };
            let pane = if snapshot.is_local {
                &mut guard.left
            } else {
                &mut guard.right
            };
            if pane.scroll_view == 0 || pane.table_view == 0 {
                return;
            }

            let sv = pane.scroll_view as *mut AnyObject;
            let tv = pane.table_view as *mut AnyObject;
            let root_view = root_view_ptr as *mut AnyObject;
            let host_view = host_view_ptr as *mut AnyObject;

            let vis_changed = pane.visible != snapshot.visible;
            let bounds_changed = pane.bounds_px != snapshot.bounds_px
                || (pane.scale_factor - snapshot.scale_factor).abs() > f32::EPSILON;
            pane.visible = snapshot.visible;
            pane.bounds_px = snapshot.bounds_px;
            pane.scale_factor = snapshot.scale_factor;

            if !snapshot.visible {
                drop(guard);
                if vis_changed {
                    unsafe {
                        let _: () = msg_send![sv, setHidden: Bool::YES];
                        restore_focus_if_inside_scroll_view(root_view, sv);
                    }
                }
                return;
            }

            let need_appearance = if pane.is_dark != Some(snapshot.is_dark) {
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
            let sort_changed = pane.sort_col != snapshot.sort_col
                || pane.sort_ascending != snapshot.sort_ascending;
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

            let need_sel_sync = data_changed || pane.selected_paths != snapshot.selected_paths;
            if need_sel_sync {
                pane.selected_paths = snapshot.selected_paths.clone();
            }
            let prev_quantized = pane.last_frame_quantized;
            let rows_clone = Arc::clone(&pane.rows);
            let selected_clone = pane.selected_paths.clone();
            let bounds_px = snapshot.bounds_px;
            let scale_factor = snapshot.scale_factor;
            drop(guard);

            unsafe {
                if bounds_changed || vis_changed {
                    let host_bounds: NSRect = msg_send![host_view, bounds];
                    let is_flipped: Bool = msg_send![host_view, isFlipped];
                    let max_pts = MAX_SAFE_PANE_POINTS as f64;
                    let safe_host_w =
                        if host_bounds.size.width.is_finite() && host_bounds.size.width > 0.0 {
                            host_bounds.size.width.min(max_pts)
                        } else {
                            max_pts
                        };
                    let safe_host_h =
                        if host_bounds.size.height.is_finite() && host_bounds.size.height > 0.0 {
                            host_bounds.size.height.min(max_pts)
                        } else {
                            max_pts
                        };

                    let (raw_x, raw_y, raw_w, raw_h) = compute_appkit_frame(
                        bounds_px,
                        scale_factor,
                        safe_host_h,
                        is_flipped.as_bool(),
                    );
                    let x = raw_x.clamp(0.0, (safe_host_w - 1.0).max(0.0));
                    let y = raw_y.clamp(0.0, (safe_host_h - 1.0).max(0.0));
                    let w = raw_w.clamp(1.0, (safe_host_w - x).max(1.0));
                    let h = raw_h.clamp(1.0, (safe_host_h - y).max(1.0));
                    let quantized = (
                        (x * 8.0).round() as i64,
                        (y * 8.0).round() as i64,
                        (w * 8.0).round() as i64,
                        (h * 8.0).round() as i64,
                    );
                    if quantized != prev_quantized {
                        let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
                        let _: () = msg_send![sv, setFrame: frame];
                        resize_mac_columns(tv, w);
                        let mut guard = self.state.lock().unwrap();
                        let pane = if snapshot.is_local {
                            &mut guard.left
                        } else {
                            &mut guard.right
                        };
                        pane.last_frame_quantized = quantized;
                    }
                }
                if vis_changed {
                    let _: () = msg_send![sv, setHidden: Bool::NO];
                }
                if let Some(is_dark) = need_appearance {
                    apply_mac_appearance(sv, is_dark);
                }
                if let Some((labels, sort_col, sort_asc)) = need_headers {
                    update_mac_column_headers(tv, &labels, sort_col, sort_asc);
                }
                if data_changed || need_sel_sync {
                    let _suppress = SelectionSuppressGuard::enter(&self.suppress_selection);
                    if data_changed {
                        let _: () = msg_send![tv, reloadData];
                    }
                    if need_sel_sync {
                        sync_mac_table_selection(tv, &rows_clone, &selected_clone);
                    }
                }
            }
        });
    }

    pub fn hide_all(&self) {
        let (left_sv, right_sv, root_view_ptr) = {
            let mut guard = self.state.lock().unwrap();
            (
                guard.left.take_hide_scroll_view(),
                guard.right.take_hide_scroll_view(),
                guard.root_ns_view,
            )
        };
        if left_sv.is_none() && right_sv.is_none() {
            return;
        }
        autoreleasepool(|_| unsafe {
            let root_view = root_view_ptr as *mut AnyObject;
            for sv_ptr in [left_sv, right_sv].into_iter().flatten() {
                let sv = sv_ptr as *mut AnyObject;
                let _: () = msg_send![sv, setHidden: Bool::YES];
                if !root_view.is_null() {
                    restore_focus_if_inside_scroll_view(root_view, sv);
                }
            }
        });
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

    fn ensure_views_created(&self, root_view_ptr: usize) {
        {
            let guard = self.state.lock().unwrap();
            if guard.root_ns_view == root_view_ptr
                && guard.left.scroll_view != 0
                && guard.right.scroll_view != 0
            {
                return;
            }
        }

        let root_view = root_view_ptr as *mut AnyObject;
        let weak_state = Arc::downgrade(&self.state);
        unsafe {
            let host_view = resolve_host_container_view(root_view);
            if host_view.is_null() {
                return;
            }
            let (l_sv, l_tv, l_del) = create_single_mac_table(
                host_view,
                root_view,
                true,
                weak_state.clone(),
                Arc::clone(&self.suppress_selection),
            );
            let (r_sv, r_tv, r_del) = create_single_mac_table(
                host_view,
                root_view,
                false,
                weak_state,
                Arc::clone(&self.suppress_selection),
            );
            let mut guard = self.state.lock().unwrap();
            guard.root_ns_view = root_view_ptr;
            guard.host_ns_view = host_view as usize;
            guard.left.scroll_view = l_sv as usize;
            guard.left.table_view = l_tv as usize;
            guard.left.delegate_obj = l_del as usize;
            guard.right.scroll_view = r_sv as usize;
            guard.right.table_view = r_tv as usize;
            guard.right.delegate_obj = r_del as usize;
        }
    }
}

impl Drop for HostInner {
    fn drop(&mut self) {
        unsafe {
            for pane in [&mut self.left, &mut self.right] {
                if pane.table_view != 0 {
                    let tv = pane.table_view as *mut AnyObject;
                    let _: () = msg_send![tv, setDelegate: null::<AnyObject>()];
                    let _: () = msg_send![tv, setDataSource: null::<AnyObject>()];
                    let _: () = msg_send![tv, setTarget: null::<AnyObject>()];
                    pane.table_view = 0;
                }
                if pane.scroll_view != 0 {
                    let sv = pane.scroll_view as *mut AnyObject;
                    let _: () = msg_send![sv, removeFromSuperview];
                    let _: () = msg_send![sv, release];
                    pane.scroll_view = 0;
                }
                if pane.delegate_obj != 0 {
                    let del = pane.delegate_obj as *mut AnyObject;
                    let _: () = msg_send![del, release];
                    pane.delegate_obj = 0;
                }
            }
            for (_, icon_ptr) in self.icon_cache.drain() {
                if icon_ptr != 0 {
                    let icon = icon_ptr as *mut AnyObject;
                    let _: () = msg_send![icon, release];
                }
            }
        }
    }
}

unsafe fn resolve_host_container_view(root_view: *mut AnyObject) -> *mut AnyObject {
    if root_view.is_null() {
        return null_mut();
    }
    // `root_view` is GPUIView (backed directly by a raw CAMetalLayer). Adding AppKit subviews to
    // NSWindow.contentView ([root_view superview]) ordered above GPUIView avoids mutating
    // GPUIView's CAMetalLayer subtree and prevents FullSizeContentView hit-test conflicts.
    let superview: *mut AnyObject = unsafe { msg_send![root_view, superview] };
    if !superview.is_null() {
        superview
    } else {
        root_view
    }
}

unsafe fn restore_focus_if_inside_scroll_view(root_view: *mut AnyObject, sv: *mut AnyObject) {
    if root_view.is_null() || sv.is_null() {
        return;
    }
    unsafe {
        let win: *mut AnyObject = msg_send![root_view, window];
        if win.is_null() {
            return;
        }
        let first_resp: *mut AnyObject = msg_send![win, firstResponder];
        if first_resp.is_null() {
            return;
        }
        let is_view: Bool = msg_send![first_resp, isKindOfClass: class!(NSView)];
        if is_view.as_bool() {
            let inside: Bool = msg_send![first_resp, isDescendantOf: sv];
            if inside.as_bool() {
                let _: Bool = msg_send![win, makeFirstResponder: root_view];
            }
        }
    }
}

fn ensure_objc_classes() {
    REGISTER_CLASSES.call_once(|| unsafe {
        let mut del_decl =
            ClassBuilder::new(c"RustduckTableDelegate", class!(NSObject)).expect("delegate class");
        del_decl.add_ivar::<*mut c_void>(DELEGATE_IVAR);
        del_decl.add_method(sel!(dealloc), delegate_dealloc as extern "C" fn(_, Sel));
        del_decl.add_method(
            sel!(numberOfRowsInTableView:),
            number_of_rows as extern "C" fn(_, Sel, *mut AnyObject) -> NSInteger,
        );
        del_decl.add_method(
            sel!(tableView:viewForTableColumn:row:),
            view_for_table_column
                as extern "C" fn(
                    _,
                    Sel,
                    *mut AnyObject,
                    *mut AnyObject,
                    NSInteger,
                ) -> *mut AnyObject,
        );
        del_decl.add_method(
            sel!(tableViewSelectionDidChange:),
            selection_did_change as extern "C" fn(_, Sel, *mut AnyObject),
        );
        del_decl.add_method(
            sel!(tableView:didClickTableColumn:),
            did_click_table_column as extern "C" fn(_, Sel, *mut AnyObject, *mut AnyObject),
        );
        del_decl.add_method(
            sel!(onDoubleClick:),
            on_double_click as extern "C" fn(_, Sel, *mut AnyObject),
        );
        del_decl.add_method(
            sel!(controlTextDidBeginEditing:),
            text_did_begin_editing as extern "C" fn(_, Sel, *mut AnyObject),
        );
        del_decl.add_method(
            sel!(controlTextDidEndEditing:),
            text_did_end_editing as extern "C" fn(_, Sel, *mut AnyObject),
        );
        del_decl.add_method(
            sel!(onContextMenuItem:),
            on_context_menu_item as extern "C" fn(_, Sel, *mut AnyObject),
        );
        del_decl.add_method(
            sel!(menuNeedsUpdate:),
            menu_needs_update as extern "C" fn(_, Sel, *mut AnyObject),
        );
        del_decl.add_method(
            sel!(tableView:writeRowsWithIndexes:toPasteboard:),
            write_rows_to_pasteboard
                as extern "C" fn(_, Sel, *mut AnyObject, *mut AnyObject, *mut AnyObject) -> Bool,
        );
        del_decl.add_method(
            sel!(tableView:validateDrop:proposedRow:proposedDropOperation:),
            validate_drop
                as extern "C" fn(
                    _,
                    Sel,
                    *mut AnyObject,
                    *mut AnyObject,
                    NSInteger,
                    NSUInteger,
                ) -> NSDragOperation,
        );
        del_decl.add_method(
            sel!(tableView:acceptDrop:row:dropOperation:),
            accept_drop
                as extern "C" fn(
                    _,
                    Sel,
                    *mut AnyObject,
                    *mut AnyObject,
                    NSInteger,
                    NSUInteger,
                ) -> Bool,
        );
        DELEGATE_CLASS.store(
            del_decl.register() as *const AnyClass as usize,
            Ordering::Release,
        );

        let mut tv_decl =
            ClassBuilder::new(c"RustduckFinderTableView", class!(NSTableView)).expect("tv class");
        tv_decl.add_method(
            sel!(keyDown:),
            table_key_down as extern "C" fn(_, Sel, *mut AnyObject),
        );
        tv_decl.add_method(
            sel!(mouseDownCanMoveWindow),
            table_mouse_down_can_move_window as extern "C" fn(_, Sel) -> Bool,
        );
        tv_decl.add_method(
            sel!(acceptsFirstMouse:),
            table_accepts_first_mouse as extern "C" fn(_, Sel, *mut AnyObject) -> Bool,
        );
        TABLE_VIEW_CLASS.store(
            tv_decl.register() as *const AnyClass as usize,
            Ordering::Release,
        );
    });
}

unsafe fn create_single_mac_table(
    host_view: *mut AnyObject,
    root_view: *mut AnyObject,
    is_local: bool,
    state: Weak<Mutex<HostInner>>,
    suppress_selection: Arc<AtomicBool>,
) -> (*mut AnyObject, *mut AnyObject, *mut AnyObject) {
    unsafe {
        let ctx_ptr = Box::into_raw(Box::new(DelegateContext {
            is_local,
            state,
            suppress_selection,
        })) as *mut c_void;

        let del_cls = DELEGATE_CLASS.load(Ordering::Acquire) as *const AnyClass;
        let tv_cls = TABLE_VIEW_CLASS.load(Ordering::Acquire) as *const AnyClass;

        let del: *mut AnyObject = msg_send![del_cls, new];
        set_delegate_ctx_ptr(del, ctx_ptr);

        let init_rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(200.0, 200.0));
        let sv: *mut AnyObject = msg_send![class!(NSScrollView), alloc];
        let sv: *mut AnyObject = msg_send![sv, initWithFrame: init_rect];
        let _: () = msg_send![sv, setWantsLayer: Bool::YES];
        let _: () = msg_send![sv, setHasVerticalScroller: Bool::YES];
        let _: () = msg_send![sv, setAutohidesScrollers: Bool::YES];
        let _: () = msg_send![sv, setDrawsBackground: Bool::YES];
        let _: () = msg_send![sv, setHidden: Bool::YES];

        let tv: *mut AnyObject = msg_send![tv_cls, alloc];
        let tv: *mut AnyObject = msg_send![tv, initWithFrame: init_rect];

        let has_set_style: Bool = msg_send![tv, respondsToSelector: sel!(setStyle:)];
        if has_set_style.as_bool() {
            let _: () = msg_send![tv, setStyle: NSTableViewStyle::Plain];
        }
        let _: () = msg_send![
            tv,
            setColumnAutoresizingStyle: NSTableViewColumnAutoresizingStyle::NoColumnAutoresizing
        ];
        let _: () = msg_send![tv, setFocusRingType: NSFocusRingType::None];
        let _: () = msg_send![tv, setUsesAlternatingRowBackgroundColors: Bool::YES];
        let _: () = msg_send![tv, setAllowsMultipleSelection: Bool::YES];
        let _: () = msg_send![tv, setAllowsColumnReordering: Bool::NO];
        let _: () = msg_send![tv, setAllowsColumnResizing: Bool::YES];
        let _: () = msg_send![tv, setRowHeight: 24.0f64];

        add_mac_column(tv, "name", "Name", 240.0);
        add_mac_column(tv, "modified", "Date Modified", 170.0);
        add_mac_column(tv, "size", "Size", 95.0);

        let _: () = msg_send![tv, setDataSource: del];
        let _: () = msg_send![tv, setDelegate: del];
        let _: () = msg_send![tv, setTarget: del];
        let _: () = msg_send![tv, setDoubleAction: sel!(onDoubleClick:)];

        // Context menu
        let menu: *mut AnyObject = msg_send![class!(NSMenu), new];
        let _: () = msg_send![menu, setDelegate: del];
        let _: () = msg_send![tv, setMenu: menu];
        let _: *mut AnyObject = msg_send![menu, autorelease];

        // Drag & Drop registration (Finder file URLs + cross-pane local/remote paths)
        let file_url_type = NSString::from_str("public.file-url");
        let local_pb_type = NSString::from_str(RUSTDUCK_LOCAL_PB_TYPE);
        let remote_pb_type = NSString::from_str(RUSTDUCK_REMOTE_PB_TYPE);
        let drag_types =
            NSArray::from_retained_slice(&[file_url_type, local_pb_type, remote_pb_type]);
        let _: () = msg_send![tv, registerForDraggedTypes: &*drag_types];

        let _: () = msg_send![sv, setDocumentView: tv];
        let _: *mut AnyObject = msg_send![tv, autorelease];

        if host_view != root_view && !root_view.is_null() {
            let _: () = msg_send![
                host_view,
                addSubview: sv,
                positioned: NSWindowOrderingMode::Above,
                relativeTo: root_view
            ];
        } else {
            let _: () = msg_send![host_view, addSubview: sv];
        }

        (sv, tv, del)
    }
}

unsafe fn add_mac_column(tv: *mut AnyObject, id: &str, title: &str, width: f64) {
    unsafe {
        let ns_id = NSString::from_str(id);
        let ns_title = NSString::from_str(title);
        let col: *mut AnyObject = msg_send![class!(NSTableColumn), alloc];
        let col: *mut AnyObject = msg_send![col, initWithIdentifier: &*ns_id];
        let _: () = msg_send![col, setTitle: &*ns_title];
        let _: () = msg_send![col, setWidth: width];
        let _: () = msg_send![col, setMinWidth: 64.0f64];
        let _: () = msg_send![tv, addTableColumn: col];
        let _: *mut AnyObject = msg_send![col, autorelease];
    }
}

unsafe fn resize_mac_columns(tv: *mut AnyObject, total_width: f64) {
    if !total_width.is_finite() || total_width <= 0.0 {
        return;
    }
    unsafe {
        let cols: *mut AnyObject = msg_send![tv, tableColumns];
        if cols.is_null() {
            return;
        }
        let count: NSUInteger = msg_send![cols, count];
        if count < 3 {
            return;
        }
        let clamped_total = total_width.min(MAX_SAFE_PANE_POINTS as f64);
        let mod_w = 170.0f64;
        let size_w = 95.0f64;
        let name_w = (clamped_total - mod_w - size_w - 24.0).max(120.0);
        let c0: *mut AnyObject = msg_send![cols, objectAtIndex: 0usize];
        let c1: *mut AnyObject = msg_send![cols, objectAtIndex: 1usize];
        let c2: *mut AnyObject = msg_send![cols, objectAtIndex: 2usize];
        let _: () = msg_send![c0, setWidth: name_w];
        let _: () = msg_send![c1, setWidth: mod_w];
        let _: () = msg_send![c2, setWidth: size_w];
    }
}

unsafe fn apply_mac_appearance(sv: *mut AnyObject, is_dark: bool) {
    let name = if is_dark {
        NSString::from_str("NSAppearanceNameDarkAqua")
    } else {
        NSString::from_str("NSAppearanceNameAqua")
    };
    unsafe {
        let appearance: *mut AnyObject = msg_send![class!(NSAppearance), appearanceNamed: &*name];
        if !appearance.is_null() {
            let _: () = msg_send![sv, setAppearance: appearance];
        }
    }
}

unsafe fn update_mac_column_headers(
    tv: *mut AnyObject,
    labels: &(String, String, String),
    sort_col: SortColumn,
    ascending: bool,
) {
    unsafe {
        let cols: *mut AnyObject = msg_send![tv, tableColumns];
        if cols.is_null() {
            return;
        }
        let count: NSUInteger = msg_send![cols, count];
        if count < 3 {
            return;
        }
        let arrow = if ascending { " ↑" } else { " ↓" };
        let titles = [
            if sort_col == SortColumn::Name {
                format!("{}{arrow}", labels.0)
            } else {
                labels.0.clone()
            },
            if sort_col == SortColumn::Modified {
                format!("{}{arrow}", labels.1)
            } else {
                labels.1.clone()
            },
            if sort_col == SortColumn::Size {
                format!("{}{arrow}", labels.2)
            } else {
                labels.2.clone()
            },
        ];

        for (idx, text) in titles.iter().enumerate() {
            let col: *mut AnyObject = msg_send![cols, objectAtIndex: idx];
            let ns_t = NSString::from_str(text);
            let _: () = msg_send![col, setTitle: &*ns_t];
        }
    }
}

unsafe fn sync_mac_table_selection(
    tv: *mut AnyObject,
    rows: &[NativeRowViewModel],
    selected_paths: &[String],
) {
    let set: HashSet<&str> = selected_paths.iter().map(|s| s.as_str()).collect();
    unsafe {
        let idx_set: *mut AnyObject = msg_send![class!(NSMutableIndexSet), indexSet];
        for (idx, row) in rows.iter().enumerate() {
            if !row.is_parent_row && set.contains(row.path.as_str()) {
                let _: () = msg_send![idx_set, addIndex: idx as NSUInteger];
            }
        }
        let _: () = msg_send![tv, selectRowIndexes: idx_set, byExtendingSelection: Bool::NO];
    }
}

unsafe fn resolve_or_cache_mac_icon(
    state: &Mutex<HostInner>,
    is_dir: bool,
    extension: &str,
) -> *mut AnyObject {
    let cache_key = if is_dir {
        "__dir__"
    } else if extension.is_empty() {
        "__file__"
    } else {
        extension
    };
    {
        let guard = state.lock().unwrap();
        if let Some(&ptr) = guard.icon_cache.get(cache_key) {
            if ptr != 0 {
                return ptr as *mut AnyObject;
            }
        }
    }

    unsafe {
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return null_mut();
        }
        let ft = if is_dir {
            NSString::from_str("public.folder")
        } else if extension.is_empty() {
            NSString::from_str("public.data")
        } else {
            NSString::from_str(extension)
        };
        let shared_icon: *mut AnyObject = msg_send![ws, iconForFileType: &*ft];
        if shared_icon.is_null() {
            return null_mut();
        }
        let owned_icon: *mut AnyObject = msg_send![shared_icon, copy];
        if owned_icon.is_null() {
            return null_mut();
        }
        let _: () = msg_send![owned_icon, setSize: NSSize::new(16.0, 16.0)];

        let mut guard = state.lock().unwrap();
        if let Some(&existing_ptr) = guard.icon_cache.get(cache_key) {
            if existing_ptr != 0 {
                let _: () = msg_send![owned_icon, release];
                return existing_ptr as *mut AnyObject;
            }
        }
        guard
            .icon_cache
            .insert(cache_key.to_string(), owned_icon as usize);
        owned_icon
    }
}

unsafe fn set_delegate_ctx_ptr(obj: *mut AnyObject, ctx_ptr: *mut c_void) {
    let cls = DELEGATE_CLASS.load(Ordering::Acquire) as *const AnyClass;
    if obj.is_null() || cls.is_null() {
        return;
    }
    unsafe {
        if let Some(ivar) = (*cls).instance_variable(DELEGATE_IVAR) {
            *ivar.load_mut::<*mut c_void>(&mut *obj) = ctx_ptr;
        }
    }
}

unsafe fn get_delegate_ctx_ptr(obj: *mut AnyObject) -> *mut c_void {
    let cls = DELEGATE_CLASS.load(Ordering::Acquire) as *const AnyClass;
    if obj.is_null() || cls.is_null() {
        return null_mut();
    }
    unsafe {
        match (*cls).instance_variable(DELEGATE_IVAR) {
            Some(ivar) => *ivar.load::<*mut c_void>(&*obj),
            None => null_mut(),
        }
    }
}

unsafe fn get_delegate_ctx<'a>(obj: *mut AnyObject) -> Option<&'a DelegateContext> {
    unsafe {
        let ptr = get_delegate_ctx_ptr(obj);
        if ptr.is_null() {
            None
        } else {
            Some(&*(ptr as *const DelegateContext))
        }
    }
}

extern "C" fn table_mouse_down_can_move_window(_this: *mut AnyObject, _cmd: Sel) -> Bool {
    Bool::NO
}

extern "C" fn table_accepts_first_mouse(
    _this: *mut AnyObject,
    _cmd: Sel,
    _event: *mut AnyObject,
) -> Bool {
    Bool::YES
}

extern "C" fn delegate_dealloc(this: *mut AnyObject, _cmd: Sel) {
    unsafe {
        let ptr = get_delegate_ctx_ptr(this);
        if !ptr.is_null() {
            set_delegate_ctx_ptr(this, null_mut());
            let _ = Box::from_raw(ptr as *mut DelegateContext);
        }
        let _: () = msg_send![super(this, class!(NSObject)), dealloc];
    }
}

extern "C" fn number_of_rows(this: *mut AnyObject, _cmd: Sel, _tv: *mut AnyObject) -> NSInteger {
    unsafe {
        let Some(ctx) = get_delegate_ctx(this) else {
            return 0;
        };
        let Some(state) = ctx.state.upgrade() else {
            return 0;
        };
        let guard = state.lock().unwrap();
        let pane = if ctx.is_local {
            &guard.left
        } else {
            &guard.right
        };
        pane.rows.len() as NSInteger
    }
}

extern "C" fn view_for_table_column(
    this: *mut AnyObject,
    _cmd: Sel,
    tv: *mut AnyObject,
    col: *mut AnyObject,
    row: NSInteger,
) -> *mut AnyObject {
    unsafe {
        let Some(ctx) = get_delegate_ctx(this) else {
            return null_mut();
        };
        let Some(state) = ctx.state.upgrade() else {
            return null_mut();
        };
        if row < 0 || tv.is_null() || col.is_null() {
            return null_mut();
        }
        let row_opt = {
            let guard = state.lock().unwrap();
            let pane = if ctx.is_local {
                &guard.left
            } else {
                &guard.right
            };
            pane.rows.get(row as usize).cloned()
        };
        let Some(row_vm) = row_opt else {
            return null_mut();
        };

        let col_id_obj: *mut AnyObject = msg_send![col, identifier];
        if col_id_obj.is_null() {
            return null_mut();
        }
        let utf8_ptr: *const i8 = msg_send![col_id_obj, UTF8String];
        let col_id = if utf8_ptr.is_null() {
            ""
        } else {
            CStr::from_ptr(utf8_ptr).to_str().unwrap_or("")
        };

        let col_width: f64 = msg_send![col, width];
        let safe_col_w = if col_width.is_finite() && col_width > 40.0 {
            col_width.min(MAX_SAFE_PANE_POINTS as f64)
        } else {
            200.0
        };

        let mut cell: *mut AnyObject =
            msg_send![tv, makeViewWithIdentifier: col_id_obj, owner: null::<AnyObject>()];
        if cell.is_null() {
            let cell_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(safe_col_w, 24.0));
            let alloc_cell: *mut AnyObject = msg_send![class!(NSTableCellView), alloc];
            cell = msg_send![alloc_cell, initWithFrame: cell_frame];
            let _: () = msg_send![cell, setIdentifier: col_id_obj];

            let tf_x = if col_id == "name" { 24.0 } else { 4.0 };
            let tf_w = (safe_col_w - tf_x - 4.0).max(24.0);
            let tf_frame = NSRect::new(NSPoint::new(tf_x, 2.0), NSSize::new(tf_w, 20.0));
            let tf: *mut AnyObject = msg_send![class!(NSTextField), alloc];
            let tf: *mut AnyObject = msg_send![tf, initWithFrame: tf_frame];
            let _: () = msg_send![tf, setDrawsBackground: Bool::NO];
            let _: () = msg_send![tf, setBordered: Bool::NO];
            let _: () = msg_send![tf, setBezeled: Bool::NO];
            let _: () = msg_send![tf, setUsesSingleLineMode: Bool::YES];
            let _: () = msg_send![tf, setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE_MASK];
            let _: () = msg_send![tf, setDelegate: this];

            if col_id == "name" {
                let iv_frame = NSRect::new(NSPoint::new(4.0, 4.0), NSSize::new(16.0, 16.0));
                let iv: *mut AnyObject = msg_send![class!(NSImageView), alloc];
                let iv: *mut AnyObject = msg_send![iv, initWithFrame: iv_frame];
                let _: () = msg_send![cell, addSubview: iv];
                let _: *mut AnyObject = msg_send![iv, autorelease];
                let _: () = msg_send![cell, setImageView: iv];
            }

            let _: () = msg_send![cell, addSubview: tf];
            let _: *mut AnyObject = msg_send![tf, autorelease];
            let _: () = msg_send![cell, setTextField: tf];
            let _: *mut AnyObject = msg_send![cell, autorelease];
        }

        let tf: *mut AnyObject = msg_send![cell, textField];
        if !tf.is_null() {
            let editable = col_id == "name" && !row_vm.is_parent_row && !row_vm.is_bucket;
            let _: () = msg_send![tf, setEditable: Bool::new(editable)];
            let text = match col_id {
                "name" => row_vm.name.as_str(),
                "modified" => row_vm.modified_text.as_str(),
                "size" => row_vm.size_text.as_str(),
                _ => "",
            };
            let ns_val = NSString::from_str(text);
            let _: () = msg_send![tf, setStringValue: &*ns_val];
        }

        if col_id == "name" {
            let iv: *mut AnyObject = msg_send![cell, imageView];
            if !iv.is_null() {
                let icon = resolve_or_cache_mac_icon(&state, row_vm.is_dir, &row_vm.extension);
                if !icon.is_null() {
                    let _: () = msg_send![iv, setImage: icon];
                }
            }
        }

        cell
    }
}

unsafe fn collect_mac_selected_paths(
    tv: *mut AnyObject,
    rows: &[NativeRowViewModel],
) -> Vec<String> {
    unsafe {
        let idx_set: *mut AnyObject = msg_send![tv, selectedRowIndexes];
        collect_mac_paths_from_ns_index_set(idx_set, rows)
    }
}

unsafe fn collect_mac_paths_from_ns_index_set(
    idx_set: *mut AnyObject,
    rows: &[NativeRowViewModel],
) -> Vec<String> {
    if idx_set.is_null() || rows.is_empty() {
        return Vec::new();
    }
    let not_found = NSNotFound as NSUInteger;
    let first: NSUInteger = unsafe { msg_send![idx_set, firstIndex] };
    if first == not_found {
        return Vec::new();
    }
    collect_paths_from_index_set(rows, first, |cur| {
        let next: NSUInteger = unsafe { msg_send![idx_set, indexGreaterThanIndex: cur] };
        if next == not_found { usize::MAX } else { next }
    })
}

extern "C" fn selection_did_change(this: *mut AnyObject, _cmd: Sel, _notif: *mut AnyObject) {
    autoreleasepool(|_| unsafe {
        let Some(ctx) = get_delegate_ctx(this) else {
            return;
        };
        if ctx.suppress_selection.load(Ordering::SeqCst) {
            return;
        }
        let Some(state) = ctx.state.upgrade() else {
            return;
        };
        let (tv, rows) = {
            let guard = state.lock().unwrap();
            if ctx.is_local {
                (
                    guard.left.table_view as *mut AnyObject,
                    Arc::clone(&guard.left.rows),
                )
            } else {
                (
                    guard.right.table_view as *mut AnyObject,
                    Arc::clone(&guard.right.rows),
                )
            }
        };
        if tv.is_null() {
            return;
        }
        let sel = collect_mac_selected_paths(tv, &rows);
        let mut guard = state.lock().unwrap();
        let pane = if ctx.is_local {
            &mut guard.left
        } else {
            &mut guard.right
        };
        if pane.selected_paths != sel {
            pane.selected_paths = sel.clone();
            guard.events.push(NativePaneEvent::SelectionChanged {
                is_local: ctx.is_local,
                selected_paths: sel,
            });
        }
    });
}

extern "C" fn did_click_table_column(
    this: *mut AnyObject,
    _cmd: Sel,
    _tv: *mut AnyObject,
    col: *mut AnyObject,
) {
    autoreleasepool(|_| unsafe {
        let Some(ctx) = get_delegate_ctx(this) else {
            return;
        };
        let Some(state) = ctx.state.upgrade() else {
            return;
        };
        if col.is_null() {
            return;
        }
        let col_id_obj: *mut AnyObject = msg_send![col, identifier];
        if col_id_obj.is_null() {
            return;
        }
        let utf8_ptr: *const i8 = msg_send![col_id_obj, UTF8String];
        let col_id = if utf8_ptr.is_null() {
            ""
        } else {
            CStr::from_ptr(utf8_ptr).to_str().unwrap_or("")
        };
        let sort_col = match col_id {
            "name" => Some(SortColumn::Name),
            "modified" => Some(SortColumn::Modified),
            "size" => Some(SortColumn::Size),
            _ => None,
        };
        if let Some(column) = sort_col {
            state
                .lock()
                .unwrap()
                .events
                .push(NativePaneEvent::SortChanged {
                    is_local: ctx.is_local,
                    column,
                });
        }
    });
}

extern "C" fn on_double_click(this: *mut AnyObject, _cmd: Sel, sender: *mut AnyObject) {
    autoreleasepool(|_| unsafe {
        let Some(ctx) = get_delegate_ctx(this) else {
            return;
        };
        let Some(state) = ctx.state.upgrade() else {
            return;
        };
        if sender.is_null() {
            return;
        }
        let clicked_row: NSInteger = msg_send![sender, clickedRow];
        if clicked_row < 0 {
            return;
        }
        let mut guard = state.lock().unwrap();
        let (has_parent, entries) = if ctx.is_local {
            (guard.left.has_parent, Arc::clone(&guard.left.entries))
        } else {
            (guard.right.has_parent, Arc::clone(&guard.right.entries))
        };
        match map_row_index_to_entry(clicked_row as usize, has_parent, &entries) {
            Some(Err(())) => guard.events.push(NativePaneEvent::NavigateUp {
                is_local: ctx.is_local,
            }),
            Some(Ok(entry)) => guard.events.push(NativePaneEvent::ActivateEntry {
                is_local: ctx.is_local,
                entry: entry.clone(),
            }),
            None => {}
        }
    });
}

extern "C" fn text_did_begin_editing(this: *mut AnyObject, _cmd: Sel, notif: *mut AnyObject) {
    autoreleasepool(|_| unsafe {
        let Some(ctx) = get_delegate_ctx(this) else {
            return;
        };
        let Some(state) = ctx.state.upgrade() else {
            return;
        };
        if notif.is_null() {
            return;
        }
        let tf: *mut AnyObject = msg_send![notif, object];
        if tf.is_null() {
            return;
        }
        let tv = {
            let guard = state.lock().unwrap();
            if ctx.is_local {
                guard.left.table_view as *mut AnyObject
            } else {
                guard.right.table_view as *mut AnyObject
            }
        };
        if tv.is_null() {
            return;
        }
        let row: NSInteger = msg_send![tv, rowForView: tf];
        if row < 0 {
            return;
        }
        let mut guard = state.lock().unwrap();
        let pane = if ctx.is_local {
            &mut guard.left
        } else {
            &mut guard.right
        };
        pane.editing_original_path =
            match map_row_index_to_entry(row as usize, pane.has_parent, &pane.entries) {
                Some(Ok(entry)) => Some(entry.path.clone()),
                _ => None,
            };
    });
}

extern "C" fn text_did_end_editing(this: *mut AnyObject, _cmd: Sel, notif: *mut AnyObject) {
    autoreleasepool(|_| unsafe {
        let Some(ctx) = get_delegate_ctx(this) else {
            return;
        };
        let Some(state) = ctx.state.upgrade() else {
            return;
        };
        if notif.is_null() {
            return;
        }
        let tf: *mut AnyObject = msg_send![notif, object];
        if tf.is_null() {
            return;
        }
        let tv = {
            let guard = state.lock().unwrap();
            if ctx.is_local {
                guard.left.table_view as *mut AnyObject
            } else {
                guard.right.table_view as *mut AnyObject
            }
        };
        let live_row = if tv.is_null() {
            None
        } else {
            let r: NSInteger = msg_send![tv, rowForView: tf];
            (r >= 0).then_some(r as usize)
        };

        let ns_str: *mut AnyObject = msg_send![tf, stringValue];
        let edited = if ns_str.is_null() {
            String::new()
        } else {
            let utf8: *const i8 = msg_send![ns_str, UTF8String];
            if utf8.is_null() {
                String::new()
            } else {
                CStr::from_ptr(utf8).to_string_lossy().to_string()
            }
        };

        let mut guard = state.lock().unwrap();
        let (tracked_path, has_parent, entries) = if ctx.is_local {
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
        if let Some(entry) =
            resolve_inline_rename_entry(tracked_path.as_deref(), live_row, has_parent, &entries)
        {
            if let Some(new_name) = should_commit_inline_rename(&entry.name, &edited) {
                guard.events.push(NativePaneEvent::InlineRenameCommitted {
                    is_local: ctx.is_local,
                    original_path: entry.path.clone(),
                    new_name,
                });
            }
        }
    });
}

extern "C" fn menu_needs_update(this: *mut AnyObject, _cmd: Sel, menu: *mut AnyObject) {
    autoreleasepool(|_| unsafe {
        let Some(ctx) = get_delegate_ctx(this) else {
            return;
        };
        let Some(state) = ctx.state.upgrade() else {
            return;
        };
        if menu.is_null() {
            return;
        }
        let _: () = msg_send![menu, removeAllItems];
        let has_sel = {
            let guard = state.lock().unwrap();
            let pane = if ctx.is_local {
                &guard.left
            } else {
                &guard.right
            };
            !pane.selected_paths.is_empty()
        };

        let t = i18n();
        let transfer_title = if ctx.is_local {
            t.ctx_upload_to_remote()
        } else {
            t.ctx_download_to_local()
        };

        add_nsmenu_item(menu, this, transfer_title, 1, has_sel);
        add_nsmenu_item(menu, this, t.ctx_open_or_enter(), 2, has_sel);
        if ctx.is_local {
            add_nsmenu_item(menu, this, t.ctx_reveal_in_shell(), 8, has_sel);
        }
        let sep1: *mut AnyObject = msg_send![class!(NSMenuItem), separatorItem];
        let _: () = msg_send![menu, addItem: sep1];
        add_nsmenu_item(menu, this, t.ctx_new_folder(), 3, true);
        add_nsmenu_item(menu, this, t.ctx_rename(), 4, has_sel);
        add_nsmenu_item(menu, this, t.ctx_delete(), 5, has_sel);
        let sep2: *mut AnyObject = msg_send![class!(NSMenuItem), separatorItem];
        let _: () = msg_send![menu, addItem: sep2];
        add_nsmenu_item(menu, this, t.ctx_copy_path(), 6, has_sel);
        add_nsmenu_item(menu, this, t.ctx_refresh(), 7, true);
    });
}

unsafe fn add_nsmenu_item(
    menu: *mut AnyObject,
    target: *mut AnyObject,
    title: &str,
    tag: NSInteger,
    enabled: bool,
) {
    let ns_title = NSString::from_str(title);
    let empty_key = NSString::from_str("");
    unsafe {
        let item: *mut AnyObject = msg_send![class!(NSMenuItem), alloc];
        let item: *mut AnyObject = msg_send![
            item,
            initWithTitle: &*ns_title,
            action: if enabled { Some(sel!(onContextMenuItem:)) } else { None },
            keyEquivalent: &*empty_key
        ];
        let _: () = msg_send![item, setTarget: target];
        let _: () = msg_send![item, setTag: tag];
        let _: () = msg_send![item, setEnabled: Bool::new(enabled)];
        let _: () = msg_send![menu, addItem: item];
        let _: *mut AnyObject = msg_send![item, autorelease];
    }
}

extern "C" fn on_context_menu_item(this: *mut AnyObject, _cmd: Sel, sender: *mut AnyObject) {
    autoreleasepool(|_| unsafe {
        let Some(ctx) = get_delegate_ctx(this) else {
            return;
        };
        let Some(state) = ctx.state.upgrade() else {
            return;
        };
        if sender.is_null() {
            return;
        }
        let tag: NSInteger = msg_send![sender, tag];

        if tag == 8 && ctx.is_local {
            let paths = state.lock().unwrap().left.selected_paths.clone();
            let urls: Vec<_> = paths
                .iter()
                .map(|p| {
                    let ns = NSString::from_str(p);
                    NSURL::fileURLWithPath(&ns)
                })
                .collect();
            if !urls.is_empty() {
                let arr = NSArray::from_retained_slice(&urls);
                let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
                if !ws.is_null() {
                    let _: () = msg_send![ws, activateFileViewerSelectingURLs: &*arr];
                }
            }
            return;
        }

        let action = match tag {
            1 => Some(NativeContextMenuAction::TransferSelected),
            2 => Some(NativeContextMenuAction::OpenSelected),
            3 => Some(NativeContextMenuAction::NewFolder),
            4 => Some(NativeContextMenuAction::RenameSelected),
            5 => Some(NativeContextMenuAction::DeleteSelected),
            6 => Some(NativeContextMenuAction::CopySelectedPath),
            7 => Some(NativeContextMenuAction::Refresh),
            _ => None,
        };
        if let Some(act) = action {
            state
                .lock()
                .unwrap()
                .events
                .push(NativePaneEvent::ContextMenuCommand {
                    is_local: ctx.is_local,
                    action: act,
                });
        }
    });
}

extern "C" fn table_key_down(this: *mut AnyObject, _cmd: Sel, event: *mut AnyObject) {
    autoreleasepool(|_| unsafe {
        let del: *mut AnyObject = msg_send![this, delegate];
        let Some(ctx) = get_delegate_ctx(del) else {
            let _: () = msg_send![super(this, class!(NSTableView)), keyDown: event];
            return;
        };
        let Some(state) = ctx.state.upgrade() else {
            let _: () = msg_send![super(this, class!(NSTableView)), keyDown: event];
            return;
        };
        if !event.is_null() {
            let key_code: u16 = msg_send![event, keyCode];
            let flags: NSUInteger = msg_send![event, modifierFlags];
            let cmd_held = (flags & (1 << 20)) != 0;

            // Finder shortcuts: Cmd+Up (126) -> parent, Cmd+Down (125) -> open, Cmd+Backspace (51) -> delete, Return (36) -> inline rename
            if cmd_held && key_code == 126 {
                state
                    .lock()
                    .unwrap()
                    .events
                    .push(NativePaneEvent::NavigateUp {
                        is_local: ctx.is_local,
                    });
                return;
            }
            if cmd_held && key_code == 51 {
                state
                    .lock()
                    .unwrap()
                    .events
                    .push(NativePaneEvent::ContextMenuCommand {
                        is_local: ctx.is_local,
                        action: NativeContextMenuAction::DeleteSelected,
                    });
                return;
            }
            if cmd_held && key_code == 125 {
                let sel_row: NSInteger = msg_send![this, selectedRow];
                if sel_row >= 0 {
                    let mut guard = state.lock().unwrap();
                    let (has_parent, entries) = if ctx.is_local {
                        (guard.left.has_parent, Arc::clone(&guard.left.entries))
                    } else {
                        (guard.right.has_parent, Arc::clone(&guard.right.entries))
                    };
                    match map_row_index_to_entry(sel_row as usize, has_parent, &entries) {
                        Some(Err(())) => guard.events.push(NativePaneEvent::NavigateUp {
                            is_local: ctx.is_local,
                        }),
                        Some(Ok(entry)) => guard.events.push(NativePaneEvent::ActivateEntry {
                            is_local: ctx.is_local,
                            entry: entry.clone(),
                        }),
                        None => {}
                    }
                    return;
                }
            }
            if !cmd_held && key_code == 36 {
                // Finder Return -> edit selected row's Name cell inline
                let sel_row: NSInteger = msg_send![this, selectedRow];
                if sel_row >= 0 {
                    let can_edit = {
                        let mut guard = state.lock().unwrap();
                        let pane = if ctx.is_local {
                            &mut guard.left
                        } else {
                            &mut guard.right
                        };
                        match map_row_index_to_entry(
                            sel_row as usize,
                            pane.has_parent,
                            &pane.entries,
                        ) {
                            Some(Ok(entry))
                                if !matches!(entry.kind, crate::models::FileKind::Bucket) =>
                            {
                                pane.editing_original_path = Some(entry.path.clone());
                                true
                            }
                            _ => false,
                        }
                    };
                    if can_edit {
                        let _: () = msg_send![
                            this,
                            editColumn: 0isize,
                            row: sel_row,
                            withEvent: null::<AnyObject>(),
                            select: Bool::YES
                        ];
                        return;
                    }
                }
            }
        }
        let _: () = msg_send![super(this, class!(NSTableView)), keyDown: event];
    });
}

extern "C" fn write_rows_to_pasteboard(
    this: *mut AnyObject,
    _cmd: Sel,
    _tv: *mut AnyObject,
    row_indexes: *mut AnyObject,
    pboard: *mut AnyObject,
) -> Bool {
    autoreleasepool(|_| unsafe {
        let Some(ctx) = get_delegate_ctx(this) else {
            return Bool::NO;
        };
        let Some(state) = ctx.state.upgrade() else {
            return Bool::NO;
        };
        if row_indexes.is_null() || pboard.is_null() {
            return Bool::NO;
        }
        let rows = {
            let guard = state.lock().unwrap();
            if ctx.is_local {
                Arc::clone(&guard.left.rows)
            } else {
                Arc::clone(&guard.right.rows)
            }
        };

        let dragged = collect_mac_paths_from_ns_index_set(row_indexes, &rows);
        if dragged.is_empty() {
            return Bool::NO;
        }

        let _: () = msg_send![pboard, clearContents];
        let joined = dragged.join("\n");
        let ns_payload = NSString::from_str(&joined);
        if ctx.is_local {
            let urls: Vec<_> = dragged
                .iter()
                .map(|p| NSURL::fileURLWithPath(&NSString::from_str(p)))
                .collect();
            let arr = NSArray::from_retained_slice(&urls);
            let _: Bool = msg_send![pboard, writeObjects: &*arr];
            let pb_type = NSString::from_str(RUSTDUCK_LOCAL_PB_TYPE);
            let _: Bool = msg_send![pboard, setString: &*ns_payload, forType: &*pb_type];
        } else {
            let pb_type = NSString::from_str(RUSTDUCK_REMOTE_PB_TYPE);
            let _: Bool = msg_send![pboard, setString: &*ns_payload, forType: &*pb_type];
        }
        Bool::YES
    })
}

extern "C" fn validate_drop(
    _this: *mut AnyObject,
    _cmd: Sel,
    _tv: *mut AnyObject,
    _info: *mut AnyObject,
    _row: NSInteger,
    _op: NSUInteger,
) -> NSDragOperation {
    NSDragOperation::Copy
}

extern "C" fn accept_drop(
    this: *mut AnyObject,
    _cmd: Sel,
    _tv: *mut AnyObject,
    info: *mut AnyObject,
    row: NSInteger,
    _op: NSUInteger,
) -> Bool {
    autoreleasepool(|_| unsafe {
        let Some(ctx) = get_delegate_ctx(this) else {
            return Bool::NO;
        };
        let Some(state) = ctx.state.upgrade() else {
            return Bool::NO;
        };
        if info.is_null() {
            return Bool::NO;
        }
        let pboard: *mut AnyObject = msg_send![info, draggingPasteboard];
        if pboard.is_null() {
            return Bool::NO;
        }

        let mut source_paths = Vec::new();
        if ctx.is_local {
            let pb_type = NSString::from_str(RUSTDUCK_REMOTE_PB_TYPE);
            let s: *mut AnyObject = msg_send![pboard, stringForType: &*pb_type];
            if !s.is_null() {
                let utf8: *const i8 = msg_send![s, UTF8String];
                if !utf8.is_null() {
                    for line in CStr::from_ptr(utf8).to_string_lossy().lines() {
                        if !line.trim().is_empty() {
                            source_paths.push(line.to_string());
                        }
                    }
                }
            }
        } else {
            let local_pb_type = NSString::from_str(RUSTDUCK_LOCAL_PB_TYPE);
            let s: *mut AnyObject = msg_send![pboard, stringForType: &*local_pb_type];
            if !s.is_null() {
                let utf8: *const i8 = msg_send![s, UTF8String];
                if !utf8.is_null() {
                    for line in CStr::from_ptr(utf8).to_string_lossy().lines() {
                        if !line.trim().is_empty() {
                            source_paths.push(line.to_string());
                        }
                    }
                }
            }
            if source_paths.is_empty() {
                let classes: *mut AnyObject =
                    msg_send![class!(NSArray), arrayWithObject: class!(NSURL)];
                let urls: *mut AnyObject =
                    msg_send![pboard, readObjectsForClasses: classes, options: null::<AnyObject>()];
                if !urls.is_null() {
                    let count: NSUInteger = msg_send![urls, count];
                    for i in 0..count {
                        let url: *mut AnyObject = msg_send![urls, objectAtIndex: i];
                        if !url.is_null() {
                            let path_ns: *mut AnyObject = msg_send![url, path];
                            if !path_ns.is_null() {
                                let utf8: *const i8 = msg_send![path_ns, UTF8String];
                                if !utf8.is_null() {
                                    let p = CStr::from_ptr(utf8).to_string_lossy().to_string();
                                    if !p.is_empty() {
                                        source_paths.push(p);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if source_paths.is_empty() {
            return Bool::NO;
        }

        let mut guard = state.lock().unwrap();
        let pane = if ctx.is_local {
            &guard.left
        } else {
            &guard.right
        };
        let hovered = if row >= 0 {
            match map_row_index_to_entry(row as usize, pane.has_parent, &pane.entries) {
                Some(Ok(e)) => Some(e),
                _ => None,
            }
        } else {
            None
        };
        let dest = resolve_drop_destination_dir(&pane.current_path, hovered, ctx.is_local);

        guard.events.push(NativePaneEvent::FilesDropped {
            target_is_local: ctx.is_local,
            source_paths,
            target_dir_override: Some(dest),
        });
        Bool::YES
    })
}
