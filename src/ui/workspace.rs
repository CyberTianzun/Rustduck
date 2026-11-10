use crate::i18n::{i18n, set_current_language, I18n};
use crate::models::{
    Bookmark, FileEntry, FileKind, ProtocolConfig, ProtocolKind, S3AddressingStyle, S3Config,
    S3Preset, SftpConfig, SmbConfig, TransferDirection, TransferStatus, TransferTask,
};
use crate::services::bookmark_store::{BookmarkStore, KeyringCredentialVault};
use crate::services::preferences::{AppLanguage, PreferencesStore, ThemePreference};
use crate::services::runtime::tokio_runtime;
use crate::services::transfer_manager::TransferManager;
use crate::storage::local::LocalFileSystem;
use crate::storage::{create_backend, StorageBackend};
use crate::ui::dual_pane_view::render_dual_pane_view;
use crate::ui::file_pane::{
    filter_and_sort_entries, join_local_or_remote, parent_local_or_remote,
    resolve_rename_target_path, summarize_pane_entries, PaneInlineAction, SortColumn,
};
use crate::ui::native_file_list::{
    build_native_row_models, NativeContextMenuAction, NativeFileListHost, NativePaneEvent,
    NativeRowViewModel,
};
use crate::ui::new_tab_view::render_new_tab_view;
use crate::ui::settings_modal::{render_settings_modal, SettingsTab};
use crate::ui::transfer_drawer::render_transfer_drawer;
use crate::ui::{
    accent_amber, accent_blue, accent_green, accent_red, apply_theme_preference, bg_canvas,
    bg_elevated, border_subtle, protocol_icon_and_color, text_muted, text_primary,
};
use futures::StreamExt as _;
use gpui::{
    div, img, prelude::*, px, AnyElement, App, ClipboardItem, Context, Entity, IntoElement,
    MouseButton, Render, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{h_flex, v_flex, Icon, Sizable as _, TitleBar};
use gpui_kit_assets::IconName;
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

pub struct ConnectionFormInputs {
    pub bookmark_id: Option<Uuid>,
    pub protocol: ProtocolKind,
    pub s3_preset: S3Preset,
    pub s3_addressing: S3AddressingStyle,
    pub bookmark_search_input: Entity<InputState>,
    pub name_input: Entity<InputState>,
    pub sftp_host_input: Entity<InputState>,
    pub sftp_port_input: Entity<InputState>,
    pub sftp_user_input: Entity<InputState>,
    pub sftp_key_path_input: Entity<InputState>,
    pub s3_endpoint_input: Entity<InputState>,
    pub s3_region_input: Entity<InputState>,
    pub s3_access_key_input: Entity<InputState>,
    pub s3_bucket_input: Entity<InputState>,
    pub smb_server_input: Entity<InputState>,
    pub smb_port_input: Entity<InputState>,
    pub smb_share_input: Entity<InputState>,
    pub smb_user_input: Entity<InputState>,
    pub smb_domain_input: Entity<InputState>,
    pub secret_input: Entity<InputState>,
    pub initial_path_input: Entity<InputState>,
}

impl ConnectionFormInputs {
    pub fn new(window: &mut Window, cx: &mut Context<RustduckWorkspace>) -> Self {
        let t = i18n();
        Self {
            bookmark_id: None,
            protocol: ProtocolKind::S3,
            s3_preset: S3Preset::AlibabaOss,
            s3_addressing: S3AddressingStyle::VirtualHosted,
            bookmark_search_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_search_bookmarks())),
            name_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_conn_name())),
            sftp_host_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_sftp_host())),
            sftp_port_input: cx.new(|cx| InputState::new(window, cx).default_value("22")),
            sftp_user_input: cx.new(|cx| InputState::new(window, cx).default_value("root")),
            sftp_key_path_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_sftp_key_path())),
            s3_endpoint_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value("https://oss-cn-hangzhou.aliyuncs.com")
            }),
            s3_region_input: cx
                .new(|cx| InputState::new(window, cx).default_value("cn-hangzhou")),
            s3_access_key_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("Access Key ID / SecretId")),
            s3_bucket_input: cx.new(|cx| {
                InputState::new(window, cx).placeholder(t.placeholder_s3_bucket())
            }),
            smb_server_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_smb_server())),
            smb_port_input: cx.new(|cx| InputState::new(window, cx).default_value("445")),
            smb_share_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_smb_share())),
            smb_user_input: cx.new(|cx| InputState::new(window, cx).default_value("admin")),
            smb_domain_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_smb_domain())),
            secret_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t.placeholder_secret())
                    .masked(true)
            }),
            initial_path_input: cx.new(|cx| InputState::new(window, cx).default_value("/")),
        }
    }

    pub fn refresh_placeholders(&self, window: &mut Window, cx: &mut Context<RustduckWorkspace>) {
        let t = i18n();
        self.bookmark_search_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_search_bookmarks(), window, cx));
        self.name_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_conn_name(), window, cx));
        self.sftp_host_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_sftp_host(), window, cx));
        self.sftp_key_path_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_sftp_key_path(), window, cx));
        self.s3_bucket_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_s3_bucket(), window, cx));
        self.smb_server_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_smb_server(), window, cx));
        self.smb_share_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_smb_share(), window, cx));
        self.smb_domain_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_smb_domain(), window, cx));
        self.secret_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_secret(), window, cx));
    }
}

#[derive(Debug, Clone)]
pub struct PaneVisibleCache {
    pub visible: Arc<Vec<FileEntry>>,
    pub rows: Arc<Vec<NativeRowViewModel>>,
    pub summary: (usize, usize, u64),
    pub revision: u64,
    initialized: bool,
    last_query: String,
    last_show_hidden: bool,
    last_sort_col: SortColumn,
    last_sort_ascending: bool,
    last_has_parent: bool,
    last_parent_label: String,
}

impl Default for PaneVisibleCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneVisibleCache {
    pub fn new() -> Self {
        Self {
            visible: Arc::new(Vec::new()),
            rows: Arc::new(Vec::new()),
            summary: (0, 0, 0),
            revision: 0,
            initialized: false,
            last_query: String::new(),
            last_show_hidden: false,
            last_sort_col: SortColumn::Name,
            last_sort_ascending: true,
            last_has_parent: false,
            last_parent_label: String::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn force_rebuild(
        &mut self,
        raw_entries: &[FileEntry],
        query: &str,
        show_hidden: bool,
        sort_col: SortColumn,
        sort_ascending: bool,
        has_parent: bool,
        parent_label: &str,
    ) {
        let sorted =
            filter_and_sort_entries(raw_entries, query, show_hidden, sort_col, sort_ascending);
        let summary = summarize_pane_entries(&sorted);
        let rows = build_native_row_models(&sorted, has_parent, parent_label);

        self.visible = Arc::new(sorted);
        self.rows = Arc::new(rows);
        self.summary = summary;
        self.revision = self.revision.wrapping_add(1);
        self.initialized = true;
        self.last_query = query.to_string();
        self.last_show_hidden = show_hidden;
        self.last_sort_col = sort_col;
        self.last_sort_ascending = sort_ascending;
        self.last_has_parent = has_parent;
        self.last_parent_label = parent_label.to_string();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sync_if_needed(
        &mut self,
        raw_entries: &[FileEntry],
        query: &str,
        show_hidden: bool,
        sort_col: SortColumn,
        sort_ascending: bool,
        has_parent: bool,
        parent_label: &str,
    ) -> bool {
        if self.initialized
            && self.last_query == query
            && self.last_show_hidden == show_hidden
            && self.last_sort_col == sort_col
            && self.last_sort_ascending == sort_ascending
            && self.last_has_parent == has_parent
            && self.last_parent_label == parent_label
        {
            return false;
        }
        self.force_rebuild(
            raw_entries,
            query,
            show_hidden,
            sort_col,
            sort_ascending,
            has_parent,
            parent_label,
        );
        true
    }
}

pub fn sync_pane_selection(
    selected_paths: &mut Vec<String>,
    primary_path: &mut Option<String>,
    new_paths: Vec<String>,
) {
    let mut deduped = Vec::with_capacity(new_paths.len());
    let mut seen = HashSet::with_capacity(new_paths.len());
    for p in new_paths {
        if !p.is_empty() && seen.insert(p.clone()) {
            deduped.push(p);
        }
    }
    *primary_path = deduped.first().cloned();
    *selected_paths = deduped;
}

pub fn prune_selection_against_entries(
    selected_paths: &mut Vec<String>,
    primary_path: &mut Option<String>,
    entries: &[FileEntry],
) {
    let valid: HashSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    selected_paths.retain(|p| valid.contains(p.as_str()));
    *primary_path = selected_paths.first().cloned();
}

pub fn resolve_local_drop_sources(
    local_entries: &[FileEntry],
    source_paths: &[String],
) -> Vec<FileEntry> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw_path in source_paths {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        if let Some(existing) = local_entries.iter().find(|e| e.path == trimmed) {
            if !matches!(existing.kind, FileKind::Bucket) {
                out.push(existing.clone());
            }
            continue;
        }
        let path_obj = std::path::Path::new(trimmed);
        let Ok(meta) = std::fs::metadata(path_obj) else {
            continue;
        };
        let Some(name_os) = path_obj.file_name() else {
            continue;
        };
        let name = name_os.to_string_lossy().to_string();
        if name.is_empty() {
            continue;
        }
        let kind = if meta.is_dir() {
            FileKind::Directory
        } else {
            FileKind::File
        };
        let size = if meta.is_dir() { 0 } else { meta.len() };
        out.push(FileEntry {
            name,
            path: trimmed.to_string(),
            kind,
            size,
            modified_unix: None,
            permissions: None,
        });
    }
    out
}

pub fn resolve_remote_drop_sources(
    remote_entries: &[FileEntry],
    source_paths: &[String],
) -> Vec<FileEntry> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw_path in source_paths {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        if let Some(existing) = remote_entries.iter().find(|e| e.path == trimmed) {
            if !matches!(existing.kind, FileKind::Bucket) {
                out.push(existing.clone());
            }
        }
    }
    out
}

pub struct TabSession {
    pub id: Uuid,
    pub title: String,
    pub is_connected: bool,
    pub is_connecting: bool,
    pub status_message: Option<String>,
    pub form: ConnectionFormInputs,
    pub local_backend: Arc<dyn StorageBackend>,
    pub remote_backend: Option<Arc<dyn StorageBackend>>,
    pub local_path: String,
    pub remote_path: String,
    pub local_entries: Vec<FileEntry>,
    pub remote_entries: Vec<FileEntry>,
    pub selected_local_path: Option<String>,
    pub selected_remote_path: Option<String>,
    pub selected_local_paths: Vec<String>,
    pub selected_remote_paths: Vec<String>,
    pub cached_local_visible: Arc<Vec<FileEntry>>,
    pub cached_remote_visible: Arc<Vec<FileEntry>>,
    pub cached_local_rows: Arc<Vec<NativeRowViewModel>>,
    pub cached_remote_rows: Arc<Vec<NativeRowViewModel>>,
    pub cached_local_summary: (usize, usize, u64),
    pub cached_remote_summary: (usize, usize, u64),
    pub local_entries_revision: u64,
    pub remote_entries_revision: u64,
    local_cache: PaneVisibleCache,
    remote_cache: PaneVisibleCache,
    pub local_path_editing: bool,
    pub remote_path_editing: bool,
    pub local_inline_action: Option<PaneInlineAction>,
    pub remote_inline_action: Option<PaneInlineAction>,
    pub local_path_input: Entity<InputState>,
    pub remote_path_input: Entity<InputState>,
    pub local_filter_input: Entity<InputState>,
    pub remote_filter_input: Entity<InputState>,
    pub local_action_input: Entity<InputState>,
    pub remote_action_input: Entity<InputState>,
    pub new_folder_input: Entity<InputState>,
    pub show_hidden: bool,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
}

impl TabSession {
    pub fn new(window: &mut Window, cx: &mut Context<RustduckWorkspace>) -> Self {
        let t = i18n();
        let local_fs = LocalFileSystem::new();
        let initial_local = LocalFileSystem::initial_local_dir();

        let local_for_input = initial_local.clone();
        let local_filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t.placeholder_filter_local()));
        let remote_filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t.placeholder_filter_remote()));

        cx.observe(&local_filter_input, |ws, _input, cx| {
            for tab in &mut ws.tabs {
                tab.sync_visible_caches_if_needed(cx);
            }
            cx.notify();
        })
        .detach();
        cx.observe(&remote_filter_input, |ws, _input, cx| {
            for tab in &mut ws.tabs {
                tab.sync_visible_caches_if_needed(cx);
            }
            cx.notify();
        })
        .detach();

        let mut session = Self {
            id: Uuid::new_v4(),
            title: t.default_tab_title().to_string(),
            is_connected: false,
            is_connecting: false,
            status_message: None,
            form: ConnectionFormInputs::new(window, cx),
            local_backend: Arc::new(local_fs),
            remote_backend: None,
            local_path: initial_local,
            remote_path: "/".to_string(),
            local_entries: Vec::new(),
            remote_entries: Vec::new(),
            selected_local_path: None,
            selected_remote_path: None,
            selected_local_paths: Vec::new(),
            selected_remote_paths: Vec::new(),
            cached_local_visible: Arc::new(Vec::new()),
            cached_remote_visible: Arc::new(Vec::new()),
            cached_local_rows: Arc::new(Vec::new()),
            cached_remote_rows: Arc::new(Vec::new()),
            cached_local_summary: (0, 0, 0),
            cached_remote_summary: (0, 0, 0),
            local_entries_revision: 0,
            remote_entries_revision: 0,
            local_cache: PaneVisibleCache::new(),
            remote_cache: PaneVisibleCache::new(),
            local_path_editing: false,
            remote_path_editing: false,
            local_inline_action: None,
            remote_inline_action: None,
            local_path_input: cx
                .new(|cx| InputState::new(window, cx).default_value(local_for_input)),
            remote_path_input: cx.new(|cx| InputState::new(window, cx).default_value("/")),
            local_filter_input,
            remote_filter_input,
            local_action_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_inline_name())),
            remote_action_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_inline_name())),
            new_folder_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_inline_name())),
            show_hidden: false,
            sort_column: SortColumn::Name,
            sort_ascending: true,
        };
        session.rebuild_visible_cache(true, cx);
        session.rebuild_visible_cache(false, cx);
        session
    }

    pub fn rebuild_visible_cache(&mut self, is_local: bool, cx: &App) {
        let parent_label = i18n().parent_dir_row_text();
        if is_local {
            let query = self.local_filter_input.read(cx).value().to_string();
            let has_parent = parent_local_or_remote(&self.local_path, true).is_some();
            self.local_cache.force_rebuild(
                &self.local_entries,
                &query,
                self.show_hidden,
                self.sort_column,
                self.sort_ascending,
                has_parent,
                parent_label,
            );
            self.cached_local_visible = Arc::clone(&self.local_cache.visible);
            self.cached_local_rows = Arc::clone(&self.local_cache.rows);
            self.cached_local_summary = self.local_cache.summary;
            self.local_entries_revision = self.local_cache.revision;
        } else {
            let query = self.remote_filter_input.read(cx).value().to_string();
            let has_parent = parent_local_or_remote(&self.remote_path, false).is_some();
            self.remote_cache.force_rebuild(
                &self.remote_entries,
                &query,
                self.show_hidden,
                self.sort_column,
                self.sort_ascending,
                has_parent,
                parent_label,
            );
            self.cached_remote_visible = Arc::clone(&self.remote_cache.visible);
            self.cached_remote_rows = Arc::clone(&self.remote_cache.rows);
            self.cached_remote_summary = self.remote_cache.summary;
            self.remote_entries_revision = self.remote_cache.revision;
        }
    }

    pub fn sync_visible_caches_if_needed(&mut self, cx: &App) {
        let parent_label = i18n().parent_dir_row_text();
        let local_query = self.local_filter_input.read(cx).value().to_string();
        let local_has_parent = parent_local_or_remote(&self.local_path, true).is_some();
        if self.local_cache.sync_if_needed(
            &self.local_entries,
            &local_query,
            self.show_hidden,
            self.sort_column,
            self.sort_ascending,
            local_has_parent,
            parent_label,
        ) {
            self.cached_local_visible = Arc::clone(&self.local_cache.visible);
            self.cached_local_rows = Arc::clone(&self.local_cache.rows);
            self.cached_local_summary = self.local_cache.summary;
            self.local_entries_revision = self.local_cache.revision;
        }

        let remote_query = self.remote_filter_input.read(cx).value().to_string();
        let remote_has_parent = parent_local_or_remote(&self.remote_path, false).is_some();
        if self.remote_cache.sync_if_needed(
            &self.remote_entries,
            &remote_query,
            self.show_hidden,
            self.sort_column,
            self.sort_ascending,
            remote_has_parent,
            parent_label,
        ) {
            self.cached_remote_visible = Arc::clone(&self.remote_cache.visible);
            self.cached_remote_rows = Arc::clone(&self.remote_cache.rows);
            self.cached_remote_summary = self.remote_cache.summary;
            self.remote_entries_revision = self.remote_cache.revision;
        }
    }

    pub fn refresh_placeholders(&mut self, window: &mut Window, cx: &mut Context<RustduckWorkspace>) {
        let t = i18n();
        if !self.is_connected && I18n::is_default_tab_title(&self.title) {
            self.title = t.default_tab_title().to_string();
        }
        self.form.refresh_placeholders(window, cx);
        self.local_filter_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_filter_local(), window, cx));
        self.remote_filter_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_filter_remote(), window, cx));
        self.local_action_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_inline_name(), window, cx));
        self.remote_action_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_inline_name(), window, cx));
        self.rebuild_visible_cache(true, cx);
        self.rebuild_visible_cache(false, cx);
    }
}

pub fn build_protocol_config_from_values(
    protocol: ProtocolKind,
    sftp_host: &str,
    sftp_port: &str,
    sftp_user: &str,
    sftp_key_path: &str,
    s3_preset: S3Preset,
    s3_endpoint: &str,
    s3_region: &str,
    s3_access_key: &str,
    s3_bucket: &str,
    s3_addressing: S3AddressingStyle,
    smb_server: &str,
    smb_port: &str,
    smb_share: &str,
    smb_user: &str,
    smb_domain: &str,
    initial_path: &str,
) -> ProtocolConfig {
    match protocol {
        ProtocolKind::Sftp => ProtocolConfig::Sftp(SftpConfig {
            host: sftp_host.trim().to_string(),
            port: sftp_port.trim().parse::<u16>().unwrap_or(22),
            username: if sftp_user.trim().is_empty() {
                "root".to_string()
            } else {
                sftp_user.trim().to_string()
            },
            private_key_path: {
                let k = sftp_key_path.trim();
                if k.is_empty() {
                    None
                } else {
                    Some(k.to_string())
                }
            },
            initial_path: initial_path.trim().to_string(),
        }),
        ProtocolKind::S3 => ProtocolConfig::S3(S3Config {
            preset: s3_preset,
            endpoint: s3_endpoint.trim().to_string(),
            region: s3_region.trim().to_string(),
            access_key_id: s3_access_key.trim().to_string(),
            bucket: {
                let b = s3_bucket.trim();
                if b.is_empty() {
                    None
                } else {
                    Some(b.to_string())
                }
            },
            initial_prefix: initial_path.trim().to_string(),
            addressing_style: s3_addressing,
        }),
        ProtocolKind::Smb => ProtocolConfig::Smb(SmbConfig {
            server: smb_server.trim().to_string(),
            port: smb_port.trim().parse::<u16>().unwrap_or(445),
            share: smb_share.trim().to_string(),
            username: smb_user.trim().to_string(),
            domain: {
                let d = smb_domain.trim();
                if d.is_empty() {
                    None
                } else {
                    Some(d.to_string())
                }
            },
            initial_path: initial_path.trim().to_string(),
        }),
    }
}

pub fn detect_newly_completed_transfers(
    seen_completed: &mut HashSet<Uuid>,
    tasks: &[TransferTask],
) -> (bool, bool, bool) {
    let mut has_active = false;
    let mut newly_completed_upload = false;
    let mut newly_completed_download = false;

    for task in tasks {
        match task.status {
            TransferStatus::Queued | TransferStatus::InProgress => {
                has_active = true;
            }
            TransferStatus::Completed => {
                if seen_completed.insert(task.id) {
                    match task.direction {
                        TransferDirection::Upload => newly_completed_upload = true,
                        TransferDirection::Download => newly_completed_download = true,
                    }
                }
            }
            _ => {}
        }
    }

    (
        has_active,
        newly_completed_upload,
        newly_completed_download,
    )
}

pub fn workspace_window_options(bounds: gpui::Bounds<gpui::Pixels>) -> gpui::WindowOptions {
    let mut window_options = TitleBar::window_options();
    window_options.window_bounds = Some(gpui::WindowBounds::Windowed(bounds));
    // Ensure Windows taskbar and Alt+Tab display "Rustduck" while keeping custom borderless titlebar
    if let Some(titlebar) = window_options.titlebar.as_mut() {
        titlebar.title = Some("Rustduck".into());
    }
    window_options
}

pub struct RustduckWorkspace {
    pub tabs: Vec<TabSession>,
    pub active_tab: usize,
    pub bookmark_store: BookmarkStore,
    pub preferences_store: PreferencesStore,
    pub transfer_manager: TransferManager,
    pub native_file_list_host: Arc<NativeFileListHost>,
    pub drawer_expanded: bool,
    pub settings_open: bool,
    pub settings_tab: SettingsTab,
    pub seen_completed_transfers: HashSet<Uuid>,
}

impl RustduckWorkspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pref_path = PreferencesStore::default_config_path();
        let preferences_store = PreferencesStore::load_or_init(pref_path)
            .expect("Failed to initialize PreferencesStore");

        let initial_lang = preferences_store.preferences().language;
        let initial_theme = preferences_store.preferences().theme;
        set_current_language(initial_lang);
        apply_theme_preference(initial_theme, Some(window), cx);

        cx.observe_window_appearance(window, |this, window, cx| {
            let pref = this.preferences_store.preferences().theme;
            if pref == ThemePreference::System {
                apply_theme_preference(ThemePreference::System, Some(window), cx);
                cx.notify();
            }
        })
        .detach();

        let vault = Arc::new(KeyringCredentialVault::new());
        let config_path = BookmarkStore::default_config_path();
        let bookmark_store = BookmarkStore::load_or_default(config_path, vault)
            .expect("Failed to initialize BookmarkStore");
        let first_tab = TabSession::new(window, cx);
        let native_file_list_host = Arc::new(NativeFileListHost::new());

        let mut ws = Self {
            tabs: vec![first_tab],
            active_tab: 0,
            bookmark_store,
            preferences_store,
            transfer_manager: TransferManager::new(3),
            native_file_list_host,
            drawer_expanded: false,
            settings_open: false,
            settings_tab: SettingsTab::General,
            seen_completed_transfers: HashSet::new(),
        };
        ws.refresh_local_pane(0, window, cx);
        ws.start_transfer_watcher(cx);
        ws.start_native_event_watcher(window, cx);
        ws
    }

    fn start_native_event_watcher(&self, window: &Window, cx: &mut Context<Self>) {
        let mut wakeup_rx = self.native_file_list_host.subscribe_wakeup();
        cx.spawn_in(window, async move |this, cx| {
            while wakeup_rx.next().await.is_some() {
                let alive = this
                    .update_in(cx, |ws, window, cx| {
                        let events = ws.native_file_list_host.drain_events();
                        if !events.is_empty() {
                            let active_idx = ws.active_tab.min(ws.tabs.len().saturating_sub(1));
                            ws.handle_native_pane_events(active_idx, events, window, cx);
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn set_language(
        &mut self,
        lang: AppLanguage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = self.preferences_store.set_language(lang);
        set_current_language(lang);
        for tab in &mut self.tabs {
            tab.refresh_placeholders(window, cx);
        }
        cx.notify();
    }

    pub fn toggle_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next_lang = self.preferences_store.preferences().language.toggle();
        self.set_language(next_lang, window, cx);
    }

    pub fn set_theme(
        &mut self,
        theme: ThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = self.preferences_store.set_theme(theme);
        apply_theme_preference(theme, Some(window), cx);
        cx.notify();
    }

    pub fn cycle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next_theme = self.preferences_store.preferences().theme.cycle_next();
        self.set_theme(next_theme, window, cx);
    }

    fn start_transfer_watcher(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                let (tx, rx) = futures::channel::oneshot::channel();
                tokio_runtime().spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                    let _ = tx.send(());
                });
                if rx.await.is_err() {
                    break;
                }

                let keep_going = this.update(cx, |ws, cx| {
                    let snap = ws.transfer_manager.snapshot();
                    let (has_active, up_done, down_done) =
                        detect_newly_completed_transfers(&mut ws.seen_completed_transfers, &snap);
                    let active_idx = ws.active_tab.min(ws.tabs.len().saturating_sub(1));
                    if down_done {
                        ws.spawn_local_list(active_idx, cx);
                    }
                    if up_done {
                        ws.spawn_remote_list(active_idx, cx);
                    }
                    if has_active || up_done || down_done {
                        cx.notify();
                    }
                });
                if keep_going.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = TabSession::new(window, cx);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.refresh_local_pane(self.active_tab, window, cx);
        cx.notify();
    }

    pub fn close_tab(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            self.tabs[0] = TabSession::new(window, cx);
            self.active_tab = 0;
            self.native_file_list_host.hide_all();
            self.refresh_local_pane(0, window, cx);
        } else {
            self.tabs.remove(idx);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
            if !self.tabs[self.active_tab].is_connected {
                self.native_file_list_host.hide_all();
            }
        }
        cx.notify();
    }

    pub fn reset_form_for_new_connection(
        &mut self,
        tab_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let t = i18n();
        let tab = &mut self.tabs[tab_idx];
        tab.form.bookmark_id = None;
        tab.form
            .name_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        tab.form
            .secret_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        tab.form
            .initial_path_input
            .update(cx, |s, cx| s.set_value("/", window, cx));
        tab.status_message = Some(t.msg_reset_blank_form());
        cx.notify();
    }

    pub fn apply_s3_preset(
        &mut self,
        tab_idx: usize,
        preset: S3Preset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = &mut self.tabs[tab_idx];
        tab.form.s3_preset = preset;
        tab.form.s3_addressing = preset.default_addressing_style();
        let region = tab.form.s3_region_input.read(cx).value().to_string();
        let endpoint = preset.default_endpoint(&region);
        tab.form
            .s3_endpoint_input
            .update(cx, |s, cx| s.set_value(endpoint, window, cx));
        cx.notify();
    }

    pub fn read_form_config(
        &self,
        tab_idx: usize,
        cx: &Context<Self>,
    ) -> (String, ProtocolConfig, String) {
        let form = &self.tabs[tab_idx].form;
        let name = form.name_input.read(cx).value().to_string();
        let sftp_host = form.sftp_host_input.read(cx).value().to_string();
        let sftp_port = form.sftp_port_input.read(cx).value().to_string();
        let sftp_user = form.sftp_user_input.read(cx).value().to_string();
        let sftp_key = form.sftp_key_path_input.read(cx).value().to_string();
        let s3_endpoint = form.s3_endpoint_input.read(cx).value().to_string();
        let s3_region = form.s3_region_input.read(cx).value().to_string();
        let s3_ak = form.s3_access_key_input.read(cx).value().to_string();
        let s3_bucket = form.s3_bucket_input.read(cx).value().to_string();
        let smb_server = form.smb_server_input.read(cx).value().to_string();
        let smb_port = form.smb_port_input.read(cx).value().to_string();
        let smb_share = form.smb_share_input.read(cx).value().to_string();
        let smb_user = form.smb_user_input.read(cx).value().to_string();
        let smb_domain = form.smb_domain_input.read(cx).value().to_string();
        let secret = form.secret_input.read(cx).value().to_string();
        let initial_path = form.initial_path_input.read(cx).value().to_string();

        let cfg = build_protocol_config_from_values(
            form.protocol,
            &sftp_host,
            &sftp_port,
            &sftp_user,
            &sftp_key,
            form.s3_preset,
            &s3_endpoint,
            &s3_region,
            &s3_ak,
            &s3_bucket,
            form.s3_addressing,
            &smb_server,
            &smb_port,
            &smb_share,
            &smb_user,
            &smb_domain,
            &initial_path,
        );

        let display_name = if name.trim().is_empty() {
            cfg.summary()
        } else {
            name.trim().to_string()
        };

        (display_name, cfg, secret)
    }

    pub fn save_current_form_as_bookmark(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        let t = i18n();
        let (name, config, secret) = self.read_form_config(tab_idx, cx);
        let id = self.tabs[tab_idx]
            .form
            .bookmark_id
            .unwrap_or_else(Uuid::new_v4);
        self.tabs[tab_idx].form.bookmark_id = Some(id);
        let bm = Bookmark { id, name, config };
        match self.bookmark_store.upsert(bm, Some(&secret)) {
            Ok(()) => {
                self.tabs[tab_idx].status_message = Some(t.msg_saved_bookmark());
            }
            Err(e) => {
                self.tabs[tab_idx].status_message = Some(t.msg_save_bookmark_failed(&e));
            }
        }
        cx.notify();
    }

    pub fn load_bookmark_into_form(
        &mut self,
        tab_idx: usize,
        bm: &Bookmark,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let t = i18n();
        let secret = self
            .bookmark_store
            .get_secret(bm.id)
            .ok()
            .flatten()
            .unwrap_or_default();
        let secret_missing = secret.trim().is_empty()
            && match &bm.config {
                ProtocolConfig::S3(_) => true,
                ProtocolConfig::Sftp(c) => {
                    c.private_key_path.as_deref().unwrap_or("").trim().is_empty()
                }
                ProtocolConfig::Smb(_) => false,
            };
        let tab = &mut self.tabs[tab_idx];
        tab.form.bookmark_id = Some(bm.id);
        tab.form
            .name_input
            .update(cx, |s, cx| s.set_value(bm.name.clone(), window, cx));
        tab.form
            .secret_input
            .update(cx, |s, cx| s.set_value(secret, window, cx));

        match &bm.config {
            ProtocolConfig::Sftp(c) => {
                tab.form.protocol = ProtocolKind::Sftp;
                tab.form
                    .sftp_host_input
                    .update(cx, |s, cx| s.set_value(c.host.clone(), window, cx));
                tab.form
                    .sftp_port_input
                    .update(cx, |s, cx| s.set_value(c.port.to_string(), window, cx));
                tab.form
                    .sftp_user_input
                    .update(cx, |s, cx| s.set_value(c.username.clone(), window, cx));
                tab.form.sftp_key_path_input.update(cx, |s, cx| {
                    s.set_value(c.private_key_path.clone().unwrap_or_default(), window, cx)
                });
                tab.form
                    .initial_path_input
                    .update(cx, |s, cx| s.set_value(c.initial_path.clone(), window, cx));
            }
            ProtocolConfig::S3(c) => {
                let effective_region =
                    crate::storage::s3::infer_s3_region_from_endpoint(&c.endpoint)
                        .unwrap_or_else(|| c.region.clone());
                tab.form.protocol = ProtocolKind::S3;
                tab.form.s3_preset = c.preset;
                tab.form.s3_addressing = c.addressing_style;
                tab.form
                    .s3_endpoint_input
                    .update(cx, |s, cx| s.set_value(c.endpoint.clone(), window, cx));
                tab.form
                    .s3_region_input
                    .update(cx, |s, cx| s.set_value(effective_region, window, cx));
                tab.form
                    .s3_access_key_input
                    .update(cx, |s, cx| s.set_value(c.access_key_id.clone(), window, cx));
                tab.form.s3_bucket_input.update(cx, |s, cx| {
                    s.set_value(c.bucket.clone().unwrap_or_default(), window, cx)
                });
                tab.form.initial_path_input.update(cx, |s, cx| {
                    s.set_value(c.initial_prefix.clone(), window, cx)
                });
            }
            ProtocolConfig::Smb(c) => {
                tab.form.protocol = ProtocolKind::Smb;
                tab.form
                    .smb_server_input
                    .update(cx, |s, cx| s.set_value(c.server.clone(), window, cx));
                tab.form
                    .smb_port_input
                    .update(cx, |s, cx| s.set_value(c.port.to_string(), window, cx));
                tab.form
                    .smb_share_input
                    .update(cx, |s, cx| s.set_value(c.share.clone(), window, cx));
                tab.form
                    .smb_user_input
                    .update(cx, |s, cx| s.set_value(c.username.clone(), window, cx));
                tab.form.smb_domain_input.update(cx, |s, cx| {
                    s.set_value(c.domain.clone().unwrap_or_default(), window, cx)
                });
                tab.form
                    .initial_path_input
                    .update(cx, |s, cx| s.set_value(c.initial_path.clone(), window, cx));
            }
        }
        tab.status_message = Some(if secret_missing {
            t.msg_loaded_bookmark_missing_secret(&bm.name)
        } else {
            t.msg_loaded_bookmark(&bm.name)
        });
        cx.notify();
    }

    pub fn test_connection_tab(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        let t = i18n();
        let (_title, config, secret) = self.read_form_config(tab_idx, cx);
        let backend = match create_backend(&config, &secret) {
            Ok(b) => b,
            Err(e) => {
                self.tabs[tab_idx].status_message = Some(t.msg_invalid_conn_params(&e));
                cx.notify();
                return;
            }
        };

        self.tabs[tab_idx].is_connecting = true;
        self.tabs[tab_idx].status_message = Some(t.msg_testing_connection());
        cx.notify();

        let tab_id = self.tabs[tab_idx].id;
        let (tx, rx) = futures::channel::oneshot::channel();
        tokio_runtime().spawn(async move {
            let res = backend.connect_and_verify().await;
            let _ = tx.send(res);
        });

        cx.spawn(async move |this, cx| {
            if let Ok(result) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    let t = i18n();
                    if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.is_connecting = false;
                        match result {
                            Ok(root) => {
                                tab.status_message = Some(t.msg_test_conn_ok(&root));
                            }
                            Err(e) => {
                                tab.status_message = Some(t.msg_test_conn_failed(&e));
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn connect_tab(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        let t = i18n();
        let (title, config, secret) = self.read_form_config(tab_idx, cx);
        let backend = match create_backend(&config, &secret) {
            Ok(b) => b,
            Err(e) => {
                self.tabs[tab_idx].status_message = Some(t.msg_invalid_conn_params(&e));
                cx.notify();
                return;
            }
        };

        self.tabs[tab_idx].is_connecting = true;
        self.tabs[tab_idx].status_message = Some(t.msg_connecting_handshake());
        cx.notify();

        let tab_id = self.tabs[tab_idx].id;
        let (tx, rx) = futures::channel::oneshot::channel();
        tokio_runtime().spawn(async move {
            let res = async {
                let initial_path = backend.connect_and_verify().await?;
                let entries = backend.list_dir(&initial_path).await?;
                Ok::<_, anyhow::Error>((backend, initial_path, entries))
            }
            .await;
            let _ = tx.send(res);
        });

        cx.spawn(async move |this, cx| {
            if let Ok(result) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    let t = i18n();
                    if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.is_connecting = false;
                        match result {
                            Ok((backend, initial_path, entries)) => {
                                tab.title = title;
                                tab.is_connected = true;
                                tab.remote_backend = Some(backend);
                                tab.remote_path = initial_path;
                                tab.remote_entries = entries;
                                sync_pane_selection(
                                    &mut tab.selected_remote_paths,
                                    &mut tab.selected_remote_path,
                                    Vec::new(),
                                );
                                tab.remote_inline_action = None;
                                tab.rebuild_visible_cache(false, cx);
                                tab.status_message = Some(t.msg_connected_ready());
                            }
                            Err(e) => {
                                tab.status_message = Some(t.msg_connect_failed(&e));
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn disconnect_tab(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        let t = i18n();
        let tab = &mut self.tabs[tab_idx];
        tab.is_connected = false;
        tab.remote_backend = None;
        tab.remote_entries.clear();
        sync_pane_selection(
            &mut tab.selected_remote_paths,
            &mut tab.selected_remote_path,
            Vec::new(),
        );
        tab.remote_inline_action = None;
        tab.rebuild_visible_cache(false, cx);
        tab.title = t.default_tab_title().to_string();
        tab.status_message = None;
        self.native_file_list_host.hide_all();
        cx.notify();
    }

    pub fn set_selected_paths(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        paths: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let tab = &mut self.tabs[tab_idx];
        if is_local {
            sync_pane_selection(
                &mut tab.selected_local_paths,
                &mut tab.selected_local_path,
                paths,
            );
        } else {
            sync_pane_selection(
                &mut tab.selected_remote_paths,
                &mut tab.selected_remote_path,
                paths,
            );
        }
        cx.notify();
    }

    pub fn select_entry(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        path: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.set_selected_paths(tab_idx, is_local, path.into_iter().collect(), cx);
    }

    pub fn selected_entry(&self, tab_idx: usize, is_local: bool) -> Option<FileEntry> {
        let tab = &self.tabs[tab_idx];
        let target = if is_local {
            tab.selected_local_path.as_deref()?
        } else {
            tab.selected_remote_path.as_deref()?
        };
        let list = if is_local {
            &tab.local_entries
        } else {
            &tab.remote_entries
        };
        list.iter().find(|e| e.path == target).cloned()
    }

    pub fn selected_entries(&self, tab_idx: usize, is_local: bool) -> Vec<FileEntry> {
        let tab = &self.tabs[tab_idx];
        let (paths, list) = if is_local {
            (&tab.selected_local_paths, &tab.local_entries)
        } else {
            (&tab.selected_remote_paths, &tab.remote_entries)
        };
        paths
            .iter()
            .filter_map(|p| list.iter().find(|e| e.path == *p).cloned())
            .collect()
    }

    pub fn open_new_folder_prompt(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = &mut self.tabs[tab_idx];
        if is_local {
            tab.local_action_input
                .update(cx, |s, cx| s.set_value("", window, cx));
            tab.local_inline_action = Some(PaneInlineAction::NewFolder);
        } else {
            tab.remote_action_input
                .update(cx, |s, cx| s.set_value("", window, cx));
            tab.remote_inline_action = Some(PaneInlineAction::NewFolder);
        }
        cx.notify();
    }

    pub fn open_rename_prompt(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        entry: &FileEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(entry.kind, FileKind::Bucket) {
            self.tabs[tab_idx].status_message = Some(i18n().msg_bucket_cannot_rename());
            cx.notify();
            return;
        }
        let tab = &mut self.tabs[tab_idx];
        let action = PaneInlineAction::Rename {
            original_name: entry.name.clone(),
            original_path: entry.path.clone(),
        };
        if is_local {
            sync_pane_selection(
                &mut tab.selected_local_paths,
                &mut tab.selected_local_path,
                vec![entry.path.clone()],
            );
            tab.local_action_input
                .update(cx, |s, cx| s.set_value(entry.name.clone(), window, cx));
            tab.local_inline_action = Some(action);
        } else {
            sync_pane_selection(
                &mut tab.selected_remote_paths,
                &mut tab.selected_remote_path,
                vec![entry.path.clone()],
            );
            tab.remote_action_input
                .update(cx, |s, cx| s.set_value(entry.name.clone(), window, cx));
            tab.remote_inline_action = Some(action);
        }
        cx.notify();
    }

    pub fn open_delete_confirm(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        entry: &FileEntry,
        cx: &mut Context<Self>,
    ) {
        if matches!(entry.kind, FileKind::Bucket) {
            return;
        }
        let tab = &mut self.tabs[tab_idx];
        let action = PaneInlineAction::ConfirmDelete {
            entry: entry.clone(),
        };
        if is_local {
            sync_pane_selection(
                &mut tab.selected_local_paths,
                &mut tab.selected_local_path,
                vec![entry.path.clone()],
            );
            tab.local_inline_action = Some(action);
        } else {
            sync_pane_selection(
                &mut tab.selected_remote_paths,
                &mut tab.selected_remote_path,
                vec![entry.path.clone()],
            );
            tab.remote_inline_action = Some(action);
        }
        cx.notify();
    }

    pub fn open_delete_selected_confirm(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        cx: &mut Context<Self>,
    ) {
        let deletable: Vec<FileEntry> = self
            .selected_entries(tab_idx, is_local)
            .into_iter()
            .filter(|e| !matches!(e.kind, FileKind::Bucket))
            .collect();
        if deletable.is_empty() {
            return;
        }
        if deletable.len() == 1 {
            self.open_delete_confirm(tab_idx, is_local, &deletable[0], cx);
            return;
        }
        let tab = &mut self.tabs[tab_idx];
        let action = PaneInlineAction::ConfirmBatchDelete { entries: deletable };
        if is_local {
            tab.local_inline_action = Some(action);
        } else {
            tab.remote_inline_action = Some(action);
        }
        cx.notify();
    }

    pub fn cancel_inline_action(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        cx: &mut Context<Self>,
    ) {
        let tab = &mut self.tabs[tab_idx];
        if is_local {
            tab.local_inline_action = None;
        } else {
            tab.remote_inline_action = None;
        }
        cx.notify();
    }

    pub fn submit_inline_action(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = if is_local {
            self.tabs[tab_idx].local_inline_action.take()
        } else {
            self.tabs[tab_idx].remote_inline_action.take()
        };
        let Some(action) = action else {
            return;
        };

        match action {
            PaneInlineAction::NewFolder => {
                let name = if is_local {
                    self.tabs[tab_idx]
                        .local_action_input
                        .read(cx)
                        .value()
                        .trim()
                        .to_string()
                } else {
                    self.tabs[tab_idx]
                        .remote_action_input
                        .read(cx)
                        .value()
                        .trim()
                        .to_string()
                };
                self.tabs[tab_idx]
                    .new_folder_input
                    .update(cx, |s, cx| s.set_value(name, window, cx));
                self.create_folder(tab_idx, is_local, window, cx);
            }
            PaneInlineAction::Rename {
                original_name,
                original_path,
            } => {
                let new_name = if is_local {
                    self.tabs[tab_idx]
                        .local_action_input
                        .read(cx)
                        .value()
                        .trim()
                        .to_string()
                } else {
                    self.tabs[tab_idx]
                        .remote_action_input
                        .read(cx)
                        .value()
                        .trim()
                        .to_string()
                };
                self.tabs[tab_idx]
                    .new_folder_input
                    .update(cx, |s, cx| s.set_value(new_name, window, cx));
                let dummy = FileEntry {
                    name: original_name,
                    path: original_path,
                    kind: FileKind::File,
                    size: 0,
                    modified_unix: None,
                    permissions: None,
                };
                self.rename_entry(tab_idx, is_local, &dummy, window, cx);
            }
            PaneInlineAction::ConfirmDelete { entry } => {
                self.delete_entry(tab_idx, is_local, &entry, window, cx);
            }
            PaneInlineAction::ConfirmBatchDelete { entries } => {
                self.delete_entries(tab_idx, is_local, entries, window, cx);
            }
        }
    }

    pub fn jump_to_path(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if is_local {
            let tab = &mut self.tabs[tab_idx];
            tab.local_path = path;
            tab.local_path_editing = false;
            sync_pane_selection(
                &mut tab.selected_local_paths,
                &mut tab.selected_local_path,
                Vec::new(),
            );
            tab.rebuild_visible_cache(true, cx);
            self.refresh_local_pane(tab_idx, window, cx);
        } else {
            let tab = &mut self.tabs[tab_idx];
            tab.remote_path = path;
            tab.remote_path_editing = false;
            sync_pane_selection(
                &mut tab.selected_remote_paths,
                &mut tab.selected_remote_path,
                Vec::new(),
            );
            tab.rebuild_visible_cache(false, cx);
            self.refresh_remote_pane(tab_idx, window, cx);
        }
    }

    fn spawn_local_list(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        if tab_idx >= self.tabs.len() {
            return;
        }
        let path = self.tabs[tab_idx].local_path.clone();
        let backend = self.tabs[tab_idx].local_backend.clone();
        let tab_id = self.tabs[tab_idx].id;
        let (tx, rx) = futures::channel::oneshot::channel();
        tokio_runtime().spawn(async move {
            let res = backend.list_dir(&path).await;
            let _ = tx.send(res);
        });

        cx.spawn(async move |this, cx| {
            if let Ok(res) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                        match res {
                            Ok(entries) => {
                                tab.local_entries = entries;
                                prune_selection_against_entries(
                                    &mut tab.selected_local_paths,
                                    &mut tab.selected_local_path,
                                    &tab.local_entries,
                                );
                                tab.rebuild_visible_cache(true, cx);
                            }
                            Err(e) => {
                                tab.status_message = Some(format!("{e}"));
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn spawn_remote_list(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        if tab_idx >= self.tabs.len() {
            return;
        }
        let Some(backend) = self.tabs[tab_idx].remote_backend.clone() else {
            return;
        };
        let path = self.tabs[tab_idx].remote_path.clone();
        let tab_id = self.tabs[tab_idx].id;
        let (tx, rx) = futures::channel::oneshot::channel();
        tokio_runtime().spawn(async move {
            let res = backend.list_dir(&path).await;
            let _ = tx.send(res);
        });

        cx.spawn(async move |this, cx| {
            if let Ok(res) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    let t = i18n();
                    if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                        match res {
                            Ok(entries) => {
                                tab.remote_entries = entries;
                                prune_selection_against_entries(
                                    &mut tab.selected_remote_paths,
                                    &mut tab.selected_remote_path,
                                    &tab.remote_entries,
                                );
                                tab.rebuild_visible_cache(false, cx);
                                tab.status_message = Some(t.msg_remote_dir_synced());
                            }
                            Err(e) => {
                                tab.status_message = Some(format!("{e}"));
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn refresh_local_pane(
        &mut self,
        tab_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.tabs[tab_idx].local_path.clone();
        self.tabs[tab_idx]
            .local_path_input
            .update(cx, |s, cx| s.set_value(path, window, cx));
        self.spawn_local_list(tab_idx, cx);
    }

    pub fn refresh_remote_pane(
        &mut self,
        tab_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.tabs[tab_idx].remote_path.clone();
        self.tabs[tab_idx]
            .remote_path_input
            .update(cx, |s, cx| s.set_value(path, window, cx));
        self.spawn_remote_list(tab_idx, cx);
    }

    pub fn navigate_up(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if is_local {
            if let Some(parent) = parent_local_or_remote(&self.tabs[tab_idx].local_path, true) {
                let tab = &mut self.tabs[tab_idx];
                tab.local_path = parent;
                sync_pane_selection(
                    &mut tab.selected_local_paths,
                    &mut tab.selected_local_path,
                    Vec::new(),
                );
                tab.local_inline_action = None;
                tab.rebuild_visible_cache(true, cx);
                self.refresh_local_pane(tab_idx, window, cx);
            }
        } else if let Some(parent) = parent_local_or_remote(&self.tabs[tab_idx].remote_path, false)
        {
            let tab = &mut self.tabs[tab_idx];
            tab.remote_path = parent;
            sync_pane_selection(
                &mut tab.selected_remote_paths,
                &mut tab.selected_remote_path,
                Vec::new(),
            );
            tab.remote_inline_action = None;
            tab.rebuild_visible_cache(false, cx);
            self.refresh_remote_pane(tab_idx, window, cx);
        }
    }

    pub fn enter_directory(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        entry: &FileEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !entry.kind.is_dir_like() {
            return;
        }
        if is_local {
            let tab = &mut self.tabs[tab_idx];
            tab.local_path = entry.path.clone();
            sync_pane_selection(
                &mut tab.selected_local_paths,
                &mut tab.selected_local_path,
                Vec::new(),
            );
            tab.local_inline_action = None;
            tab.rebuild_visible_cache(true, cx);
            self.refresh_local_pane(tab_idx, window, cx);
        } else {
            let tab = &mut self.tabs[tab_idx];
            tab.remote_path = entry.path.clone();
            sync_pane_selection(
                &mut tab.selected_remote_paths,
                &mut tab.selected_remote_path,
                Vec::new(),
            );
            tab.remote_inline_action = None;
            tab.rebuild_visible_cache(false, cx);
            self.refresh_remote_pane(tab_idx, window, cx);
        }
    }

    pub fn create_folder(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder_name = self.tabs[tab_idx]
            .new_folder_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if folder_name.is_empty() {
            return;
        }

        let (backend, target) = if is_local {
            (
                self.tabs[tab_idx].local_backend.clone(),
                join_local_or_remote(&self.tabs[tab_idx].local_path, &folder_name, true),
            )
        } else {
            let Some(remote) = self.tabs[tab_idx].remote_backend.clone() else {
                return;
            };
            (
                remote,
                join_local_or_remote(&self.tabs[tab_idx].remote_path, &folder_name, false),
            )
        };

        self.tabs[tab_idx]
            .new_folder_input
            .update(cx, |s, cx| s.set_value("", window, cx));

        let tab_id = self.tabs[tab_idx].id;
        let (tx, rx) = futures::channel::oneshot::channel();
        tokio_runtime().spawn(async move {
            let res = backend.create_dir(&target).await;
            let _ = tx.send(res);
        });

        cx.spawn(async move |this, cx| {
            if let Ok(res) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    if let Some(idx) = ws.tabs.iter().position(|t| t.id == tab_id) {
                        match res {
                            Ok(()) => {
                                if is_local {
                                    ws.spawn_local_list(idx, cx);
                                } else {
                                    ws.spawn_remote_list(idx, cx);
                                }
                            }
                            Err(e) => {
                                ws.tabs[idx].status_message = Some(format!("{e}"));
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn commit_inline_rename(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        original_path: &str,
        new_name: &str,
        cx: &mut Context<Self>,
    ) {
        let entries = if is_local {
            &self.tabs[tab_idx].local_entries
        } else {
            &self.tabs[tab_idx].remote_entries
        };
        if entries
            .iter()
            .any(|e| e.path == original_path && matches!(e.kind, FileKind::Bucket))
        {
            self.tabs[tab_idx].status_message = Some(i18n().msg_bucket_cannot_rename());
            cx.notify();
            return;
        }

        let target_path = match resolve_rename_target_path(original_path, new_name, is_local) {
            Ok(p) => p,
            Err(e) => {
                self.tabs[tab_idx].status_message = Some(format!("{e}"));
                cx.notify();
                return;
            }
        };

        let backend = if is_local {
            self.tabs[tab_idx].local_backend.clone()
        } else {
            let Some(remote) = self.tabs[tab_idx].remote_backend.clone() else {
                return;
            };
            remote
        };

        let src_path = original_path.to_string();
        let dst_for_select = target_path.clone();
        let tab_id = self.tabs[tab_idx].id;
        let (tx, rx) = futures::channel::oneshot::channel();
        tokio_runtime().spawn(async move {
            let res = backend.rename(&src_path, &target_path).await;
            let _ = tx.send(res);
        });

        cx.spawn(async move |this, cx| {
            if let Ok(res) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    if let Some(idx) = ws.tabs.iter().position(|t| t.id == tab_id) {
                        match res {
                            Ok(()) => {
                                ws.set_selected_paths(
                                    idx,
                                    is_local,
                                    vec![dst_for_select],
                                    cx,
                                );
                                if is_local {
                                    ws.spawn_local_list(idx, cx);
                                } else {
                                    ws.spawn_remote_list(idx, cx);
                                }
                            }
                            Err(e) => {
                                ws.tabs[idx].status_message = Some(format!("{e}"));
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn rename_entry(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        entry: &FileEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_name = self.tabs[tab_idx]
            .new_folder_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        self.tabs[tab_idx]
            .new_folder_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.commit_inline_rename(tab_idx, is_local, &entry.path, &new_name, cx);
    }

    pub fn delete_entry(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        entry: &FileEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_entries(tab_idx, is_local, vec![entry.clone()], window, cx);
    }

    pub fn delete_entries(
        &mut self,
        tab_idx: usize,
        is_local: bool,
        entries: Vec<FileEntry>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let targets: Vec<(String, bool)> = entries
            .into_iter()
            .filter(|e| !matches!(e.kind, FileKind::Bucket))
            .map(|e| (e.path, matches!(e.kind, FileKind::Directory)))
            .collect();
        if targets.is_empty() {
            return;
        }

        let backend = if is_local {
            self.tabs[tab_idx].local_backend.clone()
        } else {
            let Some(remote) = self.tabs[tab_idx].remote_backend.clone() else {
                return;
            };
            remote
        };

        let tab_id = self.tabs[tab_idx].id;
        let (tx, rx) = futures::channel::oneshot::channel();
        tokio_runtime().spawn(async move {
            let mut first_err: Option<anyhow::Error> = None;
            for (target_path, is_dir) in targets {
                if let Err(e) = backend.remove(&target_path, is_dir).await {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
            let _ = tx.send(first_err);
        });

        cx.spawn(async move |this, cx| {
            if let Ok(first_err) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    if let Some(idx) = ws.tabs.iter().position(|t| t.id == tab_id) {
                        if let Some(e) = first_err {
                            ws.tabs[idx].status_message = Some(format!("{e}"));
                        }
                        ws.set_selected_paths(idx, is_local, Vec::new(), cx);
                        if is_local {
                            ws.spawn_local_list(idx, cx);
                        } else {
                            ws.spawn_remote_list(idx, cx);
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn enqueue_file_transfer(
        &mut self,
        tab_idx: usize,
        from_local: bool,
        entry: &FileEntry,
        cx: &mut Context<Self>,
    ) {
        let dest_base_dir = if from_local {
            self.tabs[tab_idx].remote_path.clone()
        } else {
            self.tabs[tab_idx].local_path.clone()
        };
        self.enqueue_file_transfer_to_dir(tab_idx, from_local, entry, &dest_base_dir, cx);
    }

    pub fn enqueue_file_transfer_to_dir(
        &mut self,
        tab_idx: usize,
        from_local: bool,
        entry: &FileEntry,
        dest_base_dir: &str,
        cx: &mut Context<Self>,
    ) {
        if matches!(entry.kind, FileKind::Bucket) {
            return;
        }
        if !from_local && cfg!(windows) && dest_base_dir.trim() == "/" {
            return;
        }
        let Some(remote_backend) = self.tabs[tab_idx].remote_backend.clone() else {
            return;
        };
        let local_backend = self.tabs[tab_idx].local_backend.clone();
        self.drawer_expanded = true;

        if matches!(entry.kind, FileKind::Directory) {
            let mgr = self.transfer_manager.clone();
            let src_path = entry.path.clone();
            let (direction, src_b, dst_b, dst_path, src_local, dst_local) = if from_local {
                (
                    TransferDirection::Upload,
                    local_backend,
                    remote_backend,
                    join_local_or_remote(dest_base_dir, &entry.name, false),
                    true,
                    false,
                )
            } else {
                (
                    TransferDirection::Download,
                    remote_backend,
                    local_backend,
                    join_local_or_remote(dest_base_dir, &entry.name, true),
                    false,
                    true,
                )
            };
            let tab_id = self.tabs[tab_idx].id;
            let (tx, rx) = futures::channel::oneshot::channel();
            tokio_runtime().spawn(async move {
                let res = mgr
                    .enqueue_directory_recursive(
                        direction, src_b, src_path, src_local, dst_b, dst_path, dst_local,
                    )
                    .await;
                let _ = tx.send(res);
            });

            cx.spawn(async move |this, cx| {
                if let Ok(res) = rx.await {
                    let _ = this.update(cx, |ws, cx| {
                        if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                            if let Err(e) = res {
                                tab.status_message = Some(format!("{e}"));
                            }
                        }
                        cx.notify();
                    });
                }
            })
            .detach();
            cx.notify();
            return;
        }

        if from_local {
            let dest_path = join_local_or_remote(dest_base_dir, &entry.name, false);
            self.transfer_manager.enqueue(
                TransferDirection::Upload,
                local_backend,
                entry.path.clone(),
                remote_backend,
                dest_path,
                entry.name.clone(),
                entry.size,
            );
        } else {
            let dest_path = join_local_or_remote(dest_base_dir, &entry.name, true);
            self.transfer_manager.enqueue(
                TransferDirection::Download,
                remote_backend,
                entry.path.clone(),
                local_backend,
                dest_path,
                entry.name.clone(),
                entry.size,
            );
        }
        cx.notify();
    }

    pub fn enqueue_selected_transfers(
        &mut self,
        tab_idx: usize,
        from_local: bool,
        cx: &mut Context<Self>,
    ) {
        let entries = self.selected_entries(tab_idx, from_local);
        for entry in entries {
            if !matches!(entry.kind, FileKind::Bucket) {
                self.enqueue_file_transfer(tab_idx, from_local, &entry, cx);
            }
        }
    }

    pub fn toggle_show_hidden(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[tab_idx];
        tab.show_hidden = !tab.show_hidden;
        tab.rebuild_visible_cache(true, cx);
        tab.rebuild_visible_cache(false, cx);
        cx.notify();
    }

    pub fn toggle_sort(&mut self, tab_idx: usize, col: SortColumn, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[tab_idx];
        if tab.sort_column == col {
            tab.sort_ascending = !tab.sort_ascending;
        } else {
            tab.sort_column = col;
            tab.sort_ascending = true;
        }
        tab.rebuild_visible_cache(true, cx);
        tab.rebuild_visible_cache(false, cx);
        cx.notify();
    }

    pub fn handle_native_pane_events(
        &mut self,
        tab_idx: usize,
        events: Vec<NativePaneEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if tab_idx >= self.tabs.len() {
            return;
        }

        for event in events {
            match event {
                NativePaneEvent::SelectionChanged {
                    is_local,
                    selected_paths,
                } => {
                    self.set_selected_paths(tab_idx, is_local, selected_paths, cx);
                }
                NativePaneEvent::ActivateEntry { is_local, entry } => {
                    self.set_selected_paths(tab_idx, is_local, vec![entry.path.clone()], cx);
                    if entry.kind.is_dir_like() {
                        self.enter_directory(tab_idx, is_local, &entry, window, cx);
                    } else {
                        self.enqueue_file_transfer(tab_idx, is_local, &entry, cx);
                    }
                }
                NativePaneEvent::NavigateUp { is_local } => {
                    self.navigate_up(tab_idx, is_local, window, cx);
                }
                NativePaneEvent::SortChanged {
                    is_local: _,
                    column,
                } => {
                    self.toggle_sort(tab_idx, column, cx);
                }
                NativePaneEvent::InlineRenameCommitted {
                    is_local,
                    original_path,
                    new_name,
                } => {
                    self.commit_inline_rename(tab_idx, is_local, &original_path, &new_name, cx);
                }
                NativePaneEvent::ContextMenuCommand { is_local, action } => match action {
                    NativeContextMenuAction::TransferSelected => {
                        self.enqueue_selected_transfers(tab_idx, is_local, cx);
                    }
                    NativeContextMenuAction::OpenSelected => {
                        let selected = self.selected_entries(tab_idx, is_local);
                        if selected.len() == 1 && selected[0].kind.is_dir_like() {
                            self.enter_directory(tab_idx, is_local, &selected[0], window, cx);
                        } else {
                            self.enqueue_selected_transfers(tab_idx, is_local, cx);
                        }
                    }
                    NativeContextMenuAction::RenameSelected => {
                        if let Some(primary) = self.selected_entry(tab_idx, is_local) {
                            self.open_rename_prompt(tab_idx, is_local, &primary, window, cx);
                        }
                    }
                    NativeContextMenuAction::NewFolder => {
                        self.open_new_folder_prompt(tab_idx, is_local, window, cx);
                    }
                    NativeContextMenuAction::DeleteSelected => {
                        self.open_delete_selected_confirm(tab_idx, is_local, cx);
                    }
                    NativeContextMenuAction::CopySelectedPath => {
                        let paths = if is_local {
                            &self.tabs[tab_idx].selected_local_paths
                        } else {
                            &self.tabs[tab_idx].selected_remote_paths
                        };
                        if !paths.is_empty() {
                            cx.write_to_clipboard(ClipboardItem::new_string(paths.join("\n")));
                        }
                    }
                    NativeContextMenuAction::Refresh => {
                        if is_local {
                            self.refresh_local_pane(tab_idx, window, cx);
                        } else {
                            self.refresh_remote_pane(tab_idx, window, cx);
                        }
                    }
                    NativeContextMenuAction::RevealInShell => {
                        if is_local {
                            if let Some(primary) = self.tabs[tab_idx].selected_local_path.as_deref()
                            {
                                reveal_local_path_in_os(primary);
                            }
                        }
                    }
                },
                NativePaneEvent::FilesDropped {
                    target_is_local,
                    source_paths,
                    target_dir_override,
                } => {
                    if target_is_local {
                        let dest_dir = target_dir_override
                            .unwrap_or_else(|| self.tabs[tab_idx].local_path.clone());
                        let entries = resolve_remote_drop_sources(
                            &self.tabs[tab_idx].remote_entries,
                            &source_paths,
                        );
                        for entry in entries {
                            self.enqueue_file_transfer_to_dir(
                                tab_idx, false, &entry, &dest_dir, cx,
                            );
                        }
                    } else {
                        let dest_dir = target_dir_override
                            .unwrap_or_else(|| self.tabs[tab_idx].remote_path.clone());
                        let entries = resolve_local_drop_sources(
                            &self.tabs[tab_idx].local_entries,
                            &source_paths,
                        );
                        for entry in entries {
                            self.enqueue_file_transfer_to_dir(
                                tab_idx, true, &entry, &dest_dir, cx,
                            );
                        }
                    }
                }
            }
        }
    }

    fn render_custom_title_bar(
        &self,
        active_idx: usize,
        active_transfer_count: usize,
        total_transfers: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = i18n();
        let close_tip = t.close_tab_tooltip();
        let tab_items: Vec<Tab> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(idx, tab)| {
                let (proto_icon, proto_color) = protocol_icon_and_color(tab.form.protocol);
                let dot_color = if tab.is_connected {
                    accent_green()
                } else if tab.is_connecting {
                    accent_amber()
                } else {
                    text_muted()
                };

                Tab::new()
                    .prefix(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(div().size(px(6.0)).rounded(px(99.0)).bg(dot_color))
                            .child(Icon::new(proto_icon).xsmall().text_color(proto_color)),
                    )
                    .label(tab.title.clone())
                    .suffix(
                        div()
                            .id(("close-tab-stop", idx))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                Button::new(("close-tab", idx))
                                    .xsmall()
                                    .ghost()
                                    .compact()
                                    .icon(IconName::X)
                                    .tooltip(close_tip)
                                    .on_click(cx.listener(move |this, _ev, window, cx| {
                                        this.close_tab(idx, window, cx);
                                    })),
                            ),
                    )
            })
            .collect();

        let tab_bar = TabBar::new("workspace-titlebar-tabs")
            .folder()
            .small()
            .selected_index(active_idx)
            .on_click(cx.listener(|this, &ix, _window, cx| {
                this.active_tab = ix;
                cx.notify();
            }))
            .children(tab_items)
            .suffix(
                div()
                    .pl_1()
                    .flex()
                    .items_center()
                    .child(
                        Button::new("new-tab-btn")
                            .xsmall()
                            .ghost()
                            .compact()
                            .icon(IconName::Plus)
                            .tooltip(t.new_tab_tooltip())
                            .on_click(cx.listener(|this, _ev, window, cx| {
                                this.add_tab(window, cx);
                            })),
                    ),
            );

        let left_brand_and_tabs = h_flex()
            .h_full()
            .gap_2()
            .items_center()
            .overflow_hidden()
            .child(
                h_flex()
                    .gap_1p5()
                    .px_2()
                    .py_0p5()
                    .rounded(px(6.0))
                    .bg(bg_elevated())
                    .items_center()
                    .child(img("icons/brand_logo.png").size(px(16.0)))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(text_primary())
                            .child("Rustduck"),
                    ),
            )
            .child(
                div()
                    .id("titlebar-tab-strip-interactive")
                    .h_full()
                    .flex()
                    .items_end()
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(tab_bar),
            );

        let right_actions = h_flex()
            .id("titlebar-right-actions")
            .pr_2()
            .gap_1p5()
            .items_center()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when_some(self.tabs[active_idx].status_message.clone(), |el, msg| {
                let single_line = format_single_line_status(&msg);
                let is_err = is_error_status_message(&single_line);
                el.child(
                    h_flex()
                        .max_w(px(420.0))
                        .px_2p5()
                        .py_0p5()
                        .rounded(px(99.0))
                        .bg(bg_elevated())
                        .border_1()
                        .border_color(if is_err {
                            accent_red()
                        } else {
                            border_subtle()
                        })
                        .overflow_hidden()
                        .items_center()
                        .child(
                            div()
                                .max_w(px(396.0))
                                .truncate()
                                .text_xs()
                                .text_color(if is_err { accent_red() } else { accent_blue() })
                                .child(single_line),
                        ),
                )
            })
            .child(
                Button::new("top-open-settings")
                    .xsmall()
                    .ghost()
                    .icon(IconName::Settings)
                    .label(t.settings_button_label())
                    .tooltip(t.settings_button_tooltip())
                    .on_click(cx.listener(|this, _ev, _window, cx| {
                        this.settings_open = !this.settings_open;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("top-toggle-transfers")
                    .xsmall()
                    .secondary()
                    .icon(IconName::ArrowUpDown)
                    .label(t.transfers_button_label(active_transfer_count, total_transfers))
                    .on_click(cx.listener(|this, _ev, _win, cx| {
                        this.drawer_expanded = !this.drawer_expanded;
                        cx.notify();
                    })),
            );

        TitleBar::new()
            .child(left_brand_and_tabs)
            .child(right_actions)
            .into_any_element()
    }
}

pub fn format_single_line_status(msg: &str) -> String {
    msg.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn is_error_status_message(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    msg.contains("失败")
        || msg.contains("无效")
        || msg.contains("错误")
        || msg.contains("缺少")
        || msg.contains("未检测到")
        || lower.contains("failed")
        || lower.contains("invalid")
        || lower.contains("error")
        || lower.contains("missing")
        || lower.contains("secret not found")
}

fn reveal_local_path_in_os(path: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{path}"))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
        }
    }
}

impl Render for RustduckWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_idx = self.active_tab.min(self.tabs.len().saturating_sub(1));
        let is_connected = self.tabs[active_idx].is_connected;
        if !is_connected || self.settings_open {
            self.native_file_list_host.hide_all();
        } else {
            self.tabs[active_idx].sync_visible_caches_if_needed(cx);
        }
        let active_title = &self.tabs[active_idx].title;
        window.set_window_title(&format!("Rustduck — {active_title}"));

        let transfer_tasks = self.transfer_manager.snapshot();
        let active_transfer_count = transfer_tasks
            .iter()
            .filter(|t| matches!(t.status, TransferStatus::InProgress | TransferStatus::Queued))
            .count();

        let title_bar_el = self.render_custom_title_bar(
            active_idx,
            active_transfer_count,
            transfer_tasks.len(),
            cx,
        );
        let body_el = if is_connected {
            render_dual_pane_view(active_idx, self, cx)
        } else {
            render_new_tab_view(active_idx, self, cx)
        };
        let drawer_el = render_transfer_drawer(&transfer_tasks, self.drawer_expanded, cx);
        let settings_modal_el = self
            .settings_open
            .then(|| render_settings_modal(self, cx));

        v_flex()
            .relative()
            .size_full()
            .bg(bg_canvas())
            .text_color(text_primary())
            .child(title_bar_el)
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .child(body_el),
            )
            .child(drawer_el)
            .when_some(settings_modal_el, |el, modal| el.child(modal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{px, size, Bounds, Point};

    #[test]
    fn test_workspace_window_options_sets_taskbar_title_and_custom_titlebar() {
        let bounds = Bounds::new(Point::default(), size(px(1280.0), px(800.0)));
        let opts = workspace_window_options(bounds);
        let tb = opts.titlebar.expect("titlebar options must be present");
        assert_eq!(tb.title.as_deref(), Some("Rustduck"));
        assert!(tb.appears_transparent);
    }

    #[test]
    fn test_build_protocol_config_from_values_handles_single_bucket_and_empty_bucket() {
        let cfg = build_protocol_config_from_values(
            ProtocolKind::S3,
            "",
            "22",
            "root",
            "",
            S3Preset::TencentCos,
            "https://cos.ap-shanghai.myqcloud.com",
            "ap-shanghai",
            "AKID123",
            "  ",
            S3AddressingStyle::VirtualHosted,
            "",
            "445",
            "",
            "",
            "",
            "/releases",
        );

        match cfg {
            ProtocolConfig::S3(s3) => {
                assert_eq!(s3.bucket, None);
                assert_eq!(s3.endpoint, "https://cos.ap-shanghai.myqcloud.com");
                assert_eq!(s3.region, "ap-shanghai");
            }
            _ => panic!("expected S3 config"),
        }
    }

    #[test]
    fn test_detect_newly_completed_transfers_and_smb_port_domain_config() {
        let smb_cfg = build_protocol_config_from_values(
            ProtocolKind::Smb,
            "",
            "22",
            "root",
            "",
            S3Preset::Custom,
            "",
            "",
            "",
            "",
            S3AddressingStyle::PathStyle,
            "10.0.0.8",
            "1445",
            "engineering",
            "alice",
            "CORP",
            "/docs",
        );

        match smb_cfg {
            ProtocolConfig::Smb(smb) => {
                assert_eq!(smb.server, "10.0.0.8");
                assert_eq!(smb.port, 1445);
                assert_eq!(smb.share, "engineering");
                assert_eq!(smb.username, "alice");
                assert_eq!(smb.domain.as_deref(), Some("CORP"));
            }
            _ => panic!("expected SMB config"),
        }

        let mut seen = HashSet::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tasks = vec![
            TransferTask {
                id: id1,
                file_name: "a.txt".to_string(),
                source_path: "/a.txt".to_string(),
                dest_path: "/b.txt".to_string(),
                direction: TransferDirection::Upload,
                total_bytes: 100,
                transferred_bytes: 100,
                speed_bytes_per_sec: 0,
                status: TransferStatus::Completed,
            },
            TransferTask {
                id: id2,
                file_name: "c.txt".to_string(),
                source_path: "/c.txt".to_string(),
                dest_path: "/d.txt".to_string(),
                direction: TransferDirection::Download,
                total_bytes: 100,
                transferred_bytes: 50,
                speed_bytes_per_sec: 10,
                status: TransferStatus::InProgress,
            },
        ];

        let (active, up_done, down_done) = detect_newly_completed_transfers(&mut seen, &tasks);
        assert!(active);
        assert!(up_done);
        assert!(!down_done);

        let (active2, up_done2, down_done2) = detect_newly_completed_transfers(&mut seen, &tasks);
        assert!(active2);
        assert!(!up_done2);
        assert!(!down_done2);
    }

    #[test]
    fn test_pane_visible_cache_rebuilds_only_on_change_and_bumps_revision() {
        let entries = vec![
            FileEntry {
                name: "b.txt".into(),
                path: "/b.txt".into(),
                kind: FileKind::File,
                size: 10,
                modified_unix: None,
                permissions: None,
            },
            FileEntry {
                name: ".secret".into(),
                path: "/.secret".into(),
                kind: FileKind::File,
                size: 5,
                modified_unix: None,
                permissions: None,
            },
            FileEntry {
                name: "a.txt".into(),
                path: "/a.txt".into(),
                kind: FileKind::File,
                size: 20,
                modified_unix: None,
                permissions: None,
            },
        ];

        let mut cache = PaneVisibleCache::new();
        cache.force_rebuild(&entries, "", false, SortColumn::Name, true, true, "..");
        assert_eq!(cache.revision, 1);
        assert_eq!(cache.rows.len(), 3);
        assert!(cache.rows[0].is_parent_row);
        assert_eq!(cache.rows[1].name, "a.txt");
        assert_eq!(cache.rows[2].name, "b.txt");
        assert_eq!(cache.summary, (0, 2, 30));

        let rebuilt = cache.sync_if_needed(&entries, "", false, SortColumn::Name, true, true, "..");
        assert!(!rebuilt);
        assert_eq!(cache.revision, 1);

        let rebuilt2 =
            cache.sync_if_needed(&entries, "b", false, SortColumn::Name, true, true, "..");
        assert!(rebuilt2);
        assert_eq!(cache.revision, 2);
        assert_eq!(cache.rows.len(), 2);
        assert_eq!(cache.rows[1].name, "b.txt");
    }

    #[test]
    fn test_multi_selection_state_syncs_primary_and_prunes_stale_entries() {
        let mut selected_paths = Vec::new();
        let mut primary_path = None;

        sync_pane_selection(
            &mut selected_paths,
            &mut primary_path,
            vec!["/a.txt".to_string(), "/b.txt".to_string()],
        );
        assert_eq!(selected_paths, vec!["/a.txt", "/b.txt"]);
        assert_eq!(primary_path.as_deref(), Some("/a.txt"));

        let refreshed = vec![
            FileEntry {
                name: "b.txt".into(),
                path: "/b.txt".into(),
                kind: FileKind::File,
                size: 20,
                modified_unix: None,
                permissions: None,
            },
            FileEntry {
                name: "c.txt".into(),
                path: "/c.txt".into(),
                kind: FileKind::File,
                size: 30,
                modified_unix: None,
                permissions: None,
            },
        ];
        prune_selection_against_entries(&mut selected_paths, &mut primary_path, &refreshed);
        assert_eq!(selected_paths, vec!["/b.txt"]);
        assert_eq!(primary_path.as_deref(), Some("/b.txt"));

        sync_pane_selection(&mut selected_paths, &mut primary_path, Vec::new());
        assert!(selected_paths.is_empty());
        assert_eq!(primary_path, None);
    }

    #[test]
    fn test_resolve_drop_sources_handles_external_local_files_and_remote_entries() {
        let tmp_dir = std::env::temp_dir().join(format!("rustduck_drop_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let ext_file = tmp_dir.join("external_finder_drop.bin");
        std::fs::write(&ext_file, b"hello-native-drag-drop").unwrap();

        let ext_path_str = ext_file.to_string_lossy().to_string();
        let resolved_local = resolve_local_drop_sources(&[], std::slice::from_ref(&ext_path_str));
        assert_eq!(resolved_local.len(), 1);
        assert_eq!(resolved_local[0].name, "external_finder_drop.bin");
        assert_eq!(resolved_local[0].kind, FileKind::File);
        assert_eq!(resolved_local[0].size, 22);

        let _ = std::fs::remove_dir_all(&tmp_dir);

        let remote_entries = vec![
            FileEntry {
                name: "my-bucket".into(),
                path: "/my-bucket".into(),
                kind: FileKind::Bucket,
                size: 0,
                modified_unix: None,
                permissions: None,
            },
            FileEntry {
                name: "archive.tar.gz".into(),
                path: "/my-bucket/archive.tar.gz".into(),
                kind: FileKind::File,
                size: 4096,
                modified_unix: None,
                permissions: None,
            },
        ];
        let resolved_remote = resolve_remote_drop_sources(
            &remote_entries,
            &[
                "/my-bucket".to_string(),
                "/my-bucket/archive.tar.gz".to_string(),
            ],
        );
        assert_eq!(resolved_remote.len(), 1);
        assert_eq!(resolved_remote[0].name, "archive.tar.gz");
    }

    #[test]
    fn test_format_single_line_status_and_error_classification() {
        let multiline = "Connection failed:\n\tSignatureDoesNotMatch   \n  The Signature is invalid.";
        let clean = format_single_line_status(multiline);
        assert_eq!(
            clean,
            "Connection failed: SignatureDoesNotMatch The Signature is invalid."
        );
        assert!(!clean.contains('\n'));
        assert!(!clean.contains('\t'));

        assert!(is_error_status_message("连接失败: S3 签名错误"));
        assert!(is_error_status_message("Connection failed: 403 Forbidden"));
        assert!(is_error_status_message("连接参数无效: 请填写 SecretKey"));
        assert!(is_error_status_message("Invalid connection config: missing SecretKey"));
        assert!(!is_error_status_message("已载入收藏配置「ace-cdn-na」"));
        assert!(!is_error_status_message("Loaded favorite \"ace-cdn-na\""));
    }
}
