use crate::i18n::{i18n, set_current_language, I18n};
use crate::models::{
    Bookmark, FileEntry, FileKind, FtpConfig, GoogleDriveConfig, OneDriveConfig, ProtocolConfig,
    ProtocolKind, S3AddressingStyle, S3Config, S3Preset, SftpConfig, SmbConfig, TransferDirection,
    TransferStatus, TransferTask, WebDavConfig,
};
use crate::services::bookmark_store::{BookmarkStore, KeyringCredentialVault};
use crate::services::preferences::{
    AppLanguage, LocalStartupDirMode, PreferencesStore, S3UploadAcl, ThemePreference,
};
use crate::services::runtime::tokio_runtime;
use crate::services::transfer_manager::TransferManager;
use crate::services::update_checker::{check_github_latest_release, UpdateCheckState};
use crate::storage::gdrive::authorize_google_drive_in_browser;
use crate::storage::local::LocalFileSystem;
use crate::storage::onedrive::authorize_onedrive_in_browser;
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
use crate::ui::settings_modal::{render_settings_modal, SettingsTab, APP_VERSION};
use crate::ui::transfer_drawer::render_transfer_drawer;
use crate::ui::{
    accent_amber, accent_blue, accent_green, accent_red, apply_theme_preference, bg_canvas,
    bg_elevated, border_subtle, protocol_icon_and_color, text_muted, text_primary,
};
use futures::StreamExt as _;
use gpui::{
    div, img, prelude::*, px, AnyElement, App, ClipboardItem, Context, Entity, IntoElement,
    MouseButton, PathPromptOptions, Render, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{h_flex, v_flex, Icon, Sizable as _, TitleBar};
use gpui_kit_assets::IconName;
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormFieldValues {
    pub bookmark_id: Option<Uuid>,
    pub protocol: ProtocolKind,
    pub name: String,
    pub secret: String,
    pub initial_path: String,
    pub sftp_host: String,
    pub sftp_port: String,
    pub sftp_user: String,
    pub sftp_key_path: String,
    pub ftp_host: String,
    pub ftp_port: String,
    pub ftp_user: String,
    pub ftp_use_tls: bool,
    pub s3_preset: S3Preset,
    pub s3_addressing: S3AddressingStyle,
    pub s3_endpoint: String,
    pub s3_region: String,
    pub s3_access_key: String,
    pub s3_bucket: String,
    pub smb_server: String,
    pub smb_port: String,
    pub smb_share: String,
    pub smb_user: String,
    pub smb_domain: String,
    pub webdav_endpoint: String,
    pub webdav_user: String,
    pub gdrive_client_id: String,
    pub gdrive_client_secret: String,
    pub gdrive_proxy: String,
    pub onedrive_client_id: String,
    pub onedrive_proxy: String,
}

impl FormFieldValues {
    pub fn blank_for_protocol(protocol: ProtocolKind) -> Self {
        Self {
            bookmark_id: None,
            protocol,
            name: String::new(),
            secret: String::new(),
            initial_path: "/".to_string(),
            sftp_host: String::new(),
            sftp_port: "22".to_string(),
            sftp_user: "root".to_string(),
            sftp_key_path: String::new(),
            ftp_host: String::new(),
            ftp_port: "21".to_string(),
            ftp_user: "anonymous".to_string(),
            ftp_use_tls: false,
            s3_preset: S3Preset::AlibabaOss,
            s3_addressing: S3AddressingStyle::VirtualHosted,
            s3_endpoint: "https://oss-cn-hangzhou.aliyuncs.com".to_string(),
            s3_region: "cn-hangzhou".to_string(),
            s3_access_key: String::new(),
            s3_bucket: String::new(),
            smb_server: String::new(),
            smb_port: "445".to_string(),
            smb_share: String::new(),
            smb_user: String::new(),
            smb_domain: String::new(),
            webdav_endpoint: String::new(),
            webdav_user: String::new(),
            gdrive_client_id: String::new(),
            gdrive_client_secret: String::new(),
            gdrive_proxy: String::new(),
            onedrive_client_id: String::new(),
            onedrive_proxy: String::new(),
        }
    }

    pub fn from_bookmark(bm: &Bookmark, secret: &str) -> Self {
        let mut vals = Self::blank_for_protocol(bm.config.kind());
        vals.bookmark_id = Some(bm.id);
        vals.name = bm.name.clone();
        vals.secret = secret.to_string();

        match &bm.config {
            ProtocolConfig::Sftp(c) => {
                vals.sftp_host = c.host.clone();
                vals.sftp_port = c.port.to_string();
                vals.sftp_user = c.username.clone();
                vals.sftp_key_path = c.private_key_path.clone().unwrap_or_default();
                vals.initial_path = c.initial_path.clone();
            }
            ProtocolConfig::Ftp(c) => {
                vals.ftp_host = c.host.clone();
                vals.ftp_port = c.port.to_string();
                vals.ftp_user = c.username.clone();
                vals.ftp_use_tls = c.use_tls;
                vals.initial_path = c.initial_path.clone();
            }
            ProtocolConfig::S3(c) => {
                let effective_region =
                    crate::storage::s3::infer_s3_region_from_endpoint(&c.endpoint)
                        .unwrap_or_else(|| c.region.clone());
                vals.s3_preset = c.preset;
                vals.s3_addressing = c.addressing_style;
                vals.s3_endpoint = c.endpoint.clone();
                vals.s3_region = effective_region;
                vals.s3_access_key = c.access_key_id.clone();
                vals.s3_bucket = c.bucket.clone().unwrap_or_default();
                vals.initial_path = c.initial_prefix.clone();
            }
            ProtocolConfig::Smb(c) => {
                vals.smb_server = c.server.clone();
                vals.smb_port = c.port.to_string();
                vals.smb_share = c.share.clone();
                vals.smb_user = c.username.clone();
                vals.smb_domain = c.domain.clone().unwrap_or_default();
                vals.initial_path = c.initial_path.clone();
            }
            ProtocolConfig::WebDav(c) => {
                vals.webdav_endpoint = c.endpoint.clone();
                vals.webdav_user = c.username.clone();
                vals.initial_path = c.initial_path.clone();
            }
            ProtocolConfig::GoogleDrive(c) => {
                vals.gdrive_client_id = c.client_id.clone();
                vals.gdrive_client_secret = c.client_secret.clone().unwrap_or_default();
                vals.gdrive_proxy = c.proxy_url.clone().unwrap_or_default();
                vals.initial_path = c.initial_path.clone();
            }
            ProtocolConfig::OneDrive(c) => {
                vals.onedrive_client_id = c.client_id.clone();
                vals.onedrive_proxy = c.proxy_url.clone().unwrap_or_default();
                vals.initial_path = c.initial_path.clone();
            }
        }
        vals
    }

    pub fn to_protocol_config(&self) -> ProtocolConfig {
        let initial_path = self.initial_path.trim().to_string();
        match self.protocol {
            ProtocolKind::Sftp => ProtocolConfig::Sftp(SftpConfig {
                host: self.sftp_host.trim().to_string(),
                port: self.sftp_port.trim().parse::<u16>().unwrap_or(22),
                username: if self.sftp_user.trim().is_empty() {
                    "root".to_string()
                } else {
                    self.sftp_user.trim().to_string()
                },
                private_key_path: {
                    let k = self.sftp_key_path.trim();
                    if k.is_empty() {
                        None
                    } else {
                        Some(k.to_string())
                    }
                },
                initial_path,
            }),
            ProtocolKind::Ftp => ProtocolConfig::Ftp(FtpConfig {
                host: self.ftp_host.trim().to_string(),
                port: self.ftp_port.trim().parse::<u16>().unwrap_or(21),
                username: if self.ftp_user.trim().is_empty() {
                    "anonymous".to_string()
                } else {
                    self.ftp_user.trim().to_string()
                },
                use_tls: self.ftp_use_tls,
                initial_path,
            }),
            ProtocolKind::S3 => ProtocolConfig::S3(S3Config {
                preset: self.s3_preset,
                endpoint: self.s3_endpoint.trim().to_string(),
                region: self.s3_region.trim().to_string(),
                access_key_id: self.s3_access_key.trim().to_string(),
                bucket: {
                    let b = self.s3_bucket.trim();
                    if b.is_empty() {
                        None
                    } else {
                        Some(b.to_string())
                    }
                },
                initial_prefix: initial_path,
                addressing_style: self.s3_addressing,
            }),
            ProtocolKind::Smb => ProtocolConfig::Smb(SmbConfig {
                server: self.smb_server.trim().to_string(),
                port: self.smb_port.trim().parse::<u16>().unwrap_or(445),
                share: self.smb_share.trim().to_string(),
                username: self.smb_user.trim().to_string(),
                domain: {
                    let d = self.smb_domain.trim();
                    if d.is_empty() {
                        None
                    } else {
                        Some(d.to_string())
                    }
                },
                initial_path,
            }),
            ProtocolKind::WebDav => ProtocolConfig::WebDav(WebDavConfig {
                endpoint: self.webdav_endpoint.trim().to_string(),
                username: self.webdav_user.trim().to_string(),
                initial_path,
            }),
            ProtocolKind::GoogleDrive => ProtocolConfig::GoogleDrive(GoogleDriveConfig {
                client_id: self.gdrive_client_id.trim().to_string(),
                client_secret: {
                    let cs = self.gdrive_client_secret.trim();
                    if cs.is_empty() {
                        None
                    } else {
                        Some(cs.to_string())
                    }
                },
                proxy_url: {
                    let p = self.gdrive_proxy.trim();
                    if p.is_empty() {
                        None
                    } else {
                        Some(p.to_string())
                    }
                },
                initial_path,
            }),
            ProtocolKind::OneDrive => ProtocolConfig::OneDrive(OneDriveConfig {
                client_id: self.onedrive_client_id.trim().to_string(),
                proxy_url: {
                    let p = self.onedrive_proxy.trim();
                    if p.is_empty() {
                        None
                    } else {
                        Some(p.to_string())
                    }
                },
                initial_path,
            }),
        }
    }
}

pub struct ConnectionFormInputs {
    pub bookmark_id: Option<Uuid>,
    pub protocol: ProtocolKind,
    pub s3_preset: S3Preset,
    pub s3_addressing: S3AddressingStyle,
    pub ftp_use_tls: bool,
    pub bookmark_search_input: Entity<InputState>,
    pub name_input: Entity<InputState>,
    pub sftp_host_input: Entity<InputState>,
    pub sftp_port_input: Entity<InputState>,
    pub sftp_user_input: Entity<InputState>,
    pub sftp_key_path_input: Entity<InputState>,
    pub ftp_host_input: Entity<InputState>,
    pub ftp_port_input: Entity<InputState>,
    pub ftp_user_input: Entity<InputState>,
    pub s3_endpoint_input: Entity<InputState>,
    pub s3_region_input: Entity<InputState>,
    pub s3_access_key_input: Entity<InputState>,
    pub s3_bucket_input: Entity<InputState>,
    pub smb_server_input: Entity<InputState>,
    pub smb_port_input: Entity<InputState>,
    pub smb_share_input: Entity<InputState>,
    pub smb_user_input: Entity<InputState>,
    pub smb_domain_input: Entity<InputState>,
    pub webdav_endpoint_input: Entity<InputState>,
    pub webdav_user_input: Entity<InputState>,
    pub gdrive_client_id_input: Entity<InputState>,
    pub gdrive_client_secret_input: Entity<InputState>,
    pub gdrive_proxy_input: Entity<InputState>,
    pub onedrive_client_id_input: Entity<InputState>,
    pub onedrive_proxy_input: Entity<InputState>,
    pub secret_input: Entity<InputState>,
    pub initial_path_input: Entity<InputState>,
}

impl ConnectionFormInputs {
    pub fn new(window: &mut Window, cx: &mut Context<RustduckWorkspace>) -> Self {
        let t = i18n();
        let search_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_search_bookmarks()));
        Self::new_with_search_and_protocol(search_input, ProtocolKind::S3, window, cx)
    }

    pub fn new_with_search_and_protocol(
        bookmark_search_input: Entity<InputState>,
        protocol: ProtocolKind,
        window: &mut Window,
        cx: &mut Context<RustduckWorkspace>,
    ) -> Self {
        let t = i18n();
        Self {
            bookmark_id: None,
            protocol,
            s3_preset: S3Preset::AlibabaOss,
            s3_addressing: S3AddressingStyle::VirtualHosted,
            ftp_use_tls: false,
            bookmark_search_input,
            name_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_conn_name())),
            sftp_host_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_sftp_host())),
            sftp_port_input: cx.new(|cx| InputState::new(window, cx).default_value("22")),
            sftp_user_input: cx.new(|cx| InputState::new(window, cx).default_value("root")),
            sftp_key_path_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_sftp_key_path())),
            ftp_host_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_ftp_host())),
            ftp_port_input: cx.new(|cx| InputState::new(window, cx).default_value("21")),
            ftp_user_input: cx.new(|cx| InputState::new(window, cx).default_value("anonymous")),
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
            smb_user_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("admin / guest")),
            smb_domain_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_smb_domain())),
            webdav_endpoint_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_webdav_endpoint())),
            webdav_user_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("username")),
            gdrive_client_id_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_oauth_client_id())),
            gdrive_client_secret_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t.placeholder_oauth_client_secret())
                    .masked(true)
            }),
            gdrive_proxy_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_proxy_url())),
            onedrive_client_id_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_oauth_client_id())),
            onedrive_proxy_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t.placeholder_proxy_url())),
            secret_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t.placeholder_secret())
                    .masked(true)
            }),
            initial_path_input: cx.new(|cx| InputState::new(window, cx).default_value("/")),
        }
    }

    pub fn snapshot_values(&self, cx: &App) -> FormFieldValues {
        FormFieldValues {
            bookmark_id: self.bookmark_id,
            protocol: self.protocol,
            name: self.name_input.read(cx).value().to_string(),
            secret: self.secret_input.read(cx).value().to_string(),
            initial_path: self.initial_path_input.read(cx).value().to_string(),
            sftp_host: self.sftp_host_input.read(cx).value().to_string(),
            sftp_port: self.sftp_port_input.read(cx).value().to_string(),
            sftp_user: self.sftp_user_input.read(cx).value().to_string(),
            sftp_key_path: self.sftp_key_path_input.read(cx).value().to_string(),
            ftp_host: self.ftp_host_input.read(cx).value().to_string(),
            ftp_port: self.ftp_port_input.read(cx).value().to_string(),
            ftp_user: self.ftp_user_input.read(cx).value().to_string(),
            ftp_use_tls: self.ftp_use_tls,
            s3_preset: self.s3_preset,
            s3_addressing: self.s3_addressing,
            s3_endpoint: self.s3_endpoint_input.read(cx).value().to_string(),
            s3_region: self.s3_region_input.read(cx).value().to_string(),
            s3_access_key: self.s3_access_key_input.read(cx).value().to_string(),
            s3_bucket: self.s3_bucket_input.read(cx).value().to_string(),
            smb_server: self.smb_server_input.read(cx).value().to_string(),
            smb_port: self.smb_port_input.read(cx).value().to_string(),
            smb_share: self.smb_share_input.read(cx).value().to_string(),
            smb_user: self.smb_user_input.read(cx).value().to_string(),
            smb_domain: self.smb_domain_input.read(cx).value().to_string(),
            webdav_endpoint: self.webdav_endpoint_input.read(cx).value().to_string(),
            webdav_user: self.webdav_user_input.read(cx).value().to_string(),
            gdrive_client_id: self.gdrive_client_id_input.read(cx).value().to_string(),
            gdrive_client_secret: self.gdrive_client_secret_input.read(cx).value().to_string(),
            gdrive_proxy: self.gdrive_proxy_input.read(cx).value().to_string(),
            onedrive_client_id: self.onedrive_client_id_input.read(cx).value().to_string(),
            onedrive_proxy: self.onedrive_proxy_input.read(cx).value().to_string(),
        }
    }

    pub fn apply_values(
        &mut self,
        vals: &FormFieldValues,
        window: &mut Window,
        cx: &mut Context<RustduckWorkspace>,
    ) {
        self.bookmark_id = vals.bookmark_id;
        self.protocol = vals.protocol;
        self.s3_preset = vals.s3_preset;
        self.s3_addressing = vals.s3_addressing;
        self.ftp_use_tls = vals.ftp_use_tls;

        self.name_input
            .update(cx, |s, cx| s.set_value(vals.name.clone(), window, cx));
        self.secret_input
            .update(cx, |s, cx| s.set_value(vals.secret.clone(), window, cx));
        self.initial_path_input
            .update(cx, |s, cx| s.set_value(vals.initial_path.clone(), window, cx));

        self.sftp_host_input
            .update(cx, |s, cx| s.set_value(vals.sftp_host.clone(), window, cx));
        self.sftp_port_input
            .update(cx, |s, cx| s.set_value(vals.sftp_port.clone(), window, cx));
        self.sftp_user_input
            .update(cx, |s, cx| s.set_value(vals.sftp_user.clone(), window, cx));
        self.sftp_key_path_input.update(cx, |s, cx| {
            s.set_value(vals.sftp_key_path.clone(), window, cx)
        });

        self.ftp_host_input
            .update(cx, |s, cx| s.set_value(vals.ftp_host.clone(), window, cx));
        self.ftp_port_input
            .update(cx, |s, cx| s.set_value(vals.ftp_port.clone(), window, cx));
        self.ftp_user_input
            .update(cx, |s, cx| s.set_value(vals.ftp_user.clone(), window, cx));

        self.s3_endpoint_input
            .update(cx, |s, cx| s.set_value(vals.s3_endpoint.clone(), window, cx));
        self.s3_region_input
            .update(cx, |s, cx| s.set_value(vals.s3_region.clone(), window, cx));
        self.s3_access_key_input.update(cx, |s, cx| {
            s.set_value(vals.s3_access_key.clone(), window, cx)
        });
        self.s3_bucket_input
            .update(cx, |s, cx| s.set_value(vals.s3_bucket.clone(), window, cx));

        self.smb_server_input
            .update(cx, |s, cx| s.set_value(vals.smb_server.clone(), window, cx));
        self.smb_port_input
            .update(cx, |s, cx| s.set_value(vals.smb_port.clone(), window, cx));
        self.smb_share_input
            .update(cx, |s, cx| s.set_value(vals.smb_share.clone(), window, cx));
        self.smb_user_input
            .update(cx, |s, cx| s.set_value(vals.smb_user.clone(), window, cx));
        self.smb_domain_input
            .update(cx, |s, cx| s.set_value(vals.smb_domain.clone(), window, cx));

        self.webdav_endpoint_input.update(cx, |s, cx| {
            s.set_value(vals.webdav_endpoint.clone(), window, cx)
        });
        self.webdav_user_input
            .update(cx, |s, cx| s.set_value(vals.webdav_user.clone(), window, cx));

        self.gdrive_client_id_input.update(cx, |s, cx| {
            s.set_value(vals.gdrive_client_id.clone(), window, cx)
        });
        self.gdrive_client_secret_input.update(cx, |s, cx| {
            s.set_value(vals.gdrive_client_secret.clone(), window, cx)
        });
        self.gdrive_proxy_input
            .update(cx, |s, cx| s.set_value(vals.gdrive_proxy.clone(), window, cx));

        self.onedrive_client_id_input.update(cx, |s, cx| {
            s.set_value(vals.onedrive_client_id.clone(), window, cx)
        });
        self.onedrive_proxy_input.update(cx, |s, cx| {
            s.set_value(vals.onedrive_proxy.clone(), window, cx)
        });
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
        self.ftp_host_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_ftp_host(), window, cx));
        self.s3_bucket_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_s3_bucket(), window, cx));
        self.smb_server_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_smb_server(), window, cx));
        self.smb_share_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_smb_share(), window, cx));
        self.smb_domain_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_smb_domain(), window, cx));
        self.webdav_endpoint_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_webdav_endpoint(), window, cx));
        self.gdrive_client_id_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_oauth_client_id(), window, cx));
        self.gdrive_client_secret_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_oauth_client_secret(), window, cx));
        self.gdrive_proxy_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_proxy_url(), window, cx));
        self.onedrive_client_id_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_oauth_client_id(), window, cx));
        self.onedrive_proxy_input
            .update(cx, |s, cx| s.set_placeholder(t.placeholder_proxy_url(), window, cx));
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
        Self::new_with_initial_local_dir(LocalFileSystem::initial_local_dir(), window, cx)
    }

    pub fn new_with_initial_local_dir(
        initial_local: String,
        window: &mut Window,
        cx: &mut Context<RustduckWorkspace>,
    ) -> Self {
        let t = i18n();
        let local_fs = LocalFileSystem::new();

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

#[allow(clippy::too_many_arguments)]
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
    let mut vals = FormFieldValues::blank_for_protocol(protocol);
    vals.sftp_host = sftp_host.to_string();
    vals.sftp_port = sftp_port.to_string();
    vals.sftp_user = sftp_user.to_string();
    vals.sftp_key_path = sftp_key_path.to_string();
    vals.s3_preset = s3_preset;
    vals.s3_endpoint = s3_endpoint.to_string();
    vals.s3_region = s3_region.to_string();
    vals.s3_access_key = s3_access_key.to_string();
    vals.s3_bucket = s3_bucket.to_string();
    vals.s3_addressing = s3_addressing;
    vals.smb_server = smb_server.to_string();
    vals.smb_port = smb_port.to_string();
    vals.smb_share = smb_share.to_string();
    vals.smb_user = smb_user.to_string();
    vals.smb_domain = smb_domain.to_string();
    vals.initial_path = initial_path.to_string();
    vals.to_protocol_config()
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
    window_options.focus = true;
    window_options.show = true;
    // Ensure Windows taskbar and Alt+Tab display "Rustduck" while keeping custom borderless titlebar
    if let Some(titlebar) = window_options.titlebar.as_mut() {
        titlebar.title = Some("Rustduck".into());
    }
    window_options
}

pub fn ensure_valid_local_dir_or_fallback(path: &str) -> (String, bool) {
    let trimmed = path.trim();
    #[cfg(target_os = "windows")]
    if trimmed.is_empty() || trimmed == "/" || trimmed == "\\" {
        return (trimmed.to_string(), false);
    }
    if !trimmed.is_empty()
        && std::fs::metadata(trimmed)
            .map(|m| m.is_dir())
            .unwrap_or(false)
    {
        return (trimmed.to_string(), false);
    }
    (LocalFileSystem::initial_local_dir(), true)
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
    pub settings_custom_local_dir_input: Entity<InputState>,
    pub update_state: UpdateCheckState,
    pub seen_completed_transfers: HashSet<Uuid>,
}

impl RustduckWorkspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pref_path = PreferencesStore::default_config_path();
        let preferences_store = PreferencesStore::load_or_init(pref_path)
            .expect("Failed to initialize PreferencesStore");

        let initial_lang = preferences_store.preferences().language;
        let initial_theme = preferences_store.preferences().theme;
        let initial_custom_dir = preferences_store.preferences().custom_local_dir.clone();
        let effective_initial_local = preferences_store.resolve_effective_local_dir();
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

        let settings_custom_local_dir_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(i18n().placeholder_custom_local_dir())
                .default_value(initial_custom_dir)
        });
        cx.observe(&settings_custom_local_dir_input, |ws, input, _cx| {
            let val = input.read(_cx).value().trim().to_string();
            let _ = ws.preferences_store.set_custom_local_dir(val);
        })
        .detach();

        let vault = Arc::new(KeyringCredentialVault::new());
        let config_path = BookmarkStore::default_config_path();
        let bookmark_store = BookmarkStore::load_or_default(config_path, vault)
            .expect("Failed to initialize BookmarkStore");
        let first_tab =
            TabSession::new_with_initial_local_dir(effective_initial_local, window, cx);
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
            settings_custom_local_dir_input,
            update_state: UpdateCheckState::Idle,
            seen_completed_transfers: HashSet::new(),
        };
        ws.refresh_local_pane(0, window, cx);
        ws.start_transfer_watcher(cx);
        ws.start_native_event_watcher(window, cx);
        ws.start_daily_update_checker(cx);
        ws
    }

    pub fn set_s3_default_acl(&mut self, acl: S3UploadAcl, cx: &mut Context<Self>) {
        let _ = self.preferences_store.set_s3_default_acl(acl);
        cx.notify();
    }

    pub fn set_local_startup_mode(&mut self, mode: LocalStartupDirMode, cx: &mut Context<Self>) {
        let _ = self.preferences_store.set_local_startup_mode(mode);
        cx.notify();
    }

    pub fn browse_custom_local_dir_in_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });

        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(dir_path) = paths.into_iter().next() {
                    let dir_str = dir_path.to_string_lossy().to_string();
                    let _ = this.update_in(cx, |ws, window, cx| {
                        ws.settings_custom_local_dir_input.update(cx, |s, cx| {
                            s.set_value(dir_str.clone(), window, cx);
                        });
                        let _ = ws.preferences_store.set_custom_local_dir(dir_str.clone());
                        let _ = ws
                            .preferences_store
                            .set_local_startup_mode(LocalStartupDirMode::Custom);
                        let active_idx = ws.active_tab.min(ws.tabs.len().saturating_sub(1));
                        ws.jump_to_path(active_idx, true, dir_str, window, cx);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub fn open_url_in_browser(url: &str) {
        let _ = open::that_detached(url);
    }

    pub fn trigger_update_check(&mut self, manual: bool, cx: &mut Context<Self>) {
        if matches!(self.update_state, UpdateCheckState::Checking) {
            return;
        }
        let previous_available = match &self.update_state {
            UpdateCheckState::UpdateAvailable(info) => Some(info.clone()),
            _ => None,
        };
        self.update_state = UpdateCheckState::Checking;
        cx.notify();

        let (tx, rx) = futures::channel::oneshot::channel();
        tokio_runtime().spawn(async move {
            let res = check_github_latest_release(APP_VERSION).await;
            let _ = tx.send(res);
        });

        cx.spawn(async move |this, cx| {
            if let Ok(result) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    match result {
                        Ok(state) => {
                            let now_unix = chrono::Utc::now().timestamp();
                            let _ = ws.preferences_store.set_last_update_check_unix(now_unix);
                            ws.update_state = state;
                        }
                        Err(err) => {
                            if manual {
                                ws.update_state = UpdateCheckState::Failed(
                                    format_single_line_status(&err.to_string()),
                                );
                            } else if let Some(prev) = previous_available {
                                ws.update_state = UpdateCheckState::UpdateAvailable(prev);
                            } else {
                                ws.update_state = UpdateCheckState::Idle;
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn start_daily_update_checker(&mut self, cx: &mut Context<Self>) {
        let now_unix = chrono::Utc::now().timestamp();
        if self.preferences_store.should_run_daily_update_check(now_unix) {
            self.trigger_update_check(false, cx);
        }

        cx.spawn(async move |this, cx| {
            loop {
                let (tx, rx) = futures::channel::oneshot::channel();
                tokio_runtime().spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                    let _ = tx.send(());
                });
                if rx.await.is_err() {
                    break;
                }

                let keep_going = this.update(cx, |ws, cx| {
                    let now_unix = chrono::Utc::now().timestamp();
                    if ws.preferences_store.should_run_daily_update_check(now_unix) {
                        ws.trigger_update_check(false, cx);
                    }
                });
                if keep_going.is_err() {
                    break;
                }
            }
        })
        .detach();
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
        let custom_dir_placeholder = i18n().placeholder_custom_local_dir();
        self.settings_custom_local_dir_input
            .update(cx, |s, cx| s.set_placeholder(custom_dir_placeholder, window, cx));
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
        let initial_local = self.preferences_store.resolve_effective_local_dir();
        let tab = TabSession::new_with_initial_local_dir(initial_local, window, cx);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.refresh_local_pane(self.active_tab, window, cx);
        cx.notify();
    }

    pub fn close_tab(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            let initial_local = self.preferences_store.resolve_effective_local_dir();
            self.tabs[0] = TabSession::new_with_initial_local_dir(initial_local, window, cx);
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
        let protocol = self.tabs[tab_idx].form.protocol;
        let search_input = self.tabs[tab_idx].form.bookmark_search_input.clone();
        let blank = FormFieldValues::blank_for_protocol(protocol);
        self.tabs[tab_idx].form.apply_values(&blank, window, cx);
        let fresh_form = ConnectionFormInputs::new_with_search_and_protocol(
            search_input,
            protocol,
            window,
            cx,
        );
        self.tabs[tab_idx].form = fresh_form;
        self.tabs[tab_idx].status_message = Some(t.msg_reset_blank_form());
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
        let vals = self.tabs[tab_idx].form.snapshot_values(cx);
        let cfg = vals.to_protocol_config();
        let display_name = if vals.name.trim().is_empty() {
            cfg.summary()
        } else {
            vals.name.trim().to_string()
        };

        (display_name, cfg, vals.secret)
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

    pub fn export_bookmarks_dialog(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        let default_dir = std::path::PathBuf::from(LocalFileSystem::initial_local_dir());
        let rx = cx.prompt_for_new_path(&default_dir, Some("rustduck-connections.json"));
        let tab_id = self.tabs[tab_idx].id;

        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    let t = i18n();
                    let msg = match ws.bookmark_store.export_to_path(&path) {
                        Ok(count) => t.msg_exported_bookmarks(count, &path.to_string_lossy()),
                        Err(e) => t.msg_export_failed(&e),
                    };
                    if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.status_message = Some(msg);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn import_bookmarks_dialog(&mut self, tab_idx: usize, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        let tab_id = self.tabs[tab_idx].id;

        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(path) = paths.into_iter().next() {
                    let _ = this.update(cx, |ws, cx| {
                        let t = i18n();
                        let msg = match ws.bookmark_store.import_from_path(&path) {
                            Ok(count) => t.msg_imported_bookmarks(count),
                            Err(e) => t.msg_import_failed(&e),
                        };
                        if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                            tab.status_message = Some(msg);
                        }
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub fn authorize_oauth_in_browser(
        &mut self,
        tab_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let t = i18n();
        let (_title, config, _secret) = self.read_form_config(tab_idx, cx);
        let provider = match config.kind() {
            ProtocolKind::GoogleDrive => "Google Drive",
            ProtocolKind::OneDrive => "OneDrive",
            other => other.short_tag(),
        };
        self.tabs[tab_idx].is_connecting = true;
        self.tabs[tab_idx].status_message = Some(t.msg_oauth_authorizing_browser(provider));
        cx.notify();

        let tab_id = self.tabs[tab_idx].id;
        let (tx, rx) = futures::channel::oneshot::channel();
        tokio_runtime().spawn(async move {
            let res = match config {
                ProtocolConfig::GoogleDrive(cfg) => authorize_google_drive_in_browser(&cfg).await,
                ProtocolConfig::OneDrive(cfg) => authorize_onedrive_in_browser(&cfg).await,
                _ => Err(anyhow::anyhow!(
                    "OAuth browser login is only supported for Google Drive / OneDrive"
                )),
            };
            let _ = tx.send(res);
        });

        cx.spawn_in(window, async move |this, cx| {
            if let Ok(result) = rx.await {
                let _ = this.update_in(cx, |ws, window, cx| {
                    let t = i18n();
                    if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.is_connecting = false;
                        match result {
                            Ok(refresh_token) => {
                                tab.form.secret_input.update(cx, |s, cx| {
                                    s.set_value(refresh_token.clone(), window, cx)
                                });
                                if let Some(bm_id) = tab.form.bookmark_id {
                                    let _ = ws.bookmark_store.set_secret(bm_id, &refresh_token);
                                }
                                tab.status_message = Some(t.msg_oauth_authorized_ok(provider));
                            }
                            Err(e) => {
                                tab.status_message =
                                    Some(t.msg_oauth_authorize_failed(provider, &e));
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
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
                ProtocolConfig::S3(_) | ProtocolConfig::WebDav(_) => true,
                ProtocolConfig::Sftp(c) => {
                    c.private_key_path.as_deref().unwrap_or("").trim().is_empty()
                }
                ProtocolConfig::Ftp(c) => {
                    !c.username.trim().is_empty()
                        && !c.username.trim().eq_ignore_ascii_case("anonymous")
                }
                ProtocolConfig::Smb(_)
                | ProtocolConfig::GoogleDrive(_)
                | ProtocolConfig::OneDrive(_) => false,
            };
        let vals = FormFieldValues::from_bookmark(bm, &secret);
        let tab = &mut self.tabs[tab_idx];
        tab.form.apply_values(&vals, window, cx);
        tab.status_message = Some(if secret_missing {
            t.msg_loaded_bookmark_missing_secret(&bm.name)
        } else {
            t.msg_loaded_bookmark(&bm.name)
        });
        cx.notify();
    }

    pub fn test_connection_tab(
        &mut self,
        tab_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            let res = backend
                .connect_and_verify()
                .await
                .map(|root| (root, backend.exported_secret()));
            let _ = tx.send(res);
        });

        cx.spawn_in(window, async move |this, cx| {
            if let Ok(result) = rx.await {
                let _ = this.update_in(cx, |ws, window, cx| {
                    let t = i18n();
                    if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.is_connecting = false;
                        match result {
                            Ok((root, maybe_secret)) => {
                                if let Some(new_sec) = maybe_secret {
                                    tab.form.secret_input.update(cx, |s, cx| {
                                        s.set_value(new_sec.clone(), window, cx)
                                    });
                                    if let Some(bm_id) = tab.form.bookmark_id {
                                        let _ = ws.bookmark_store.set_secret(bm_id, &new_sec);
                                    }
                                }
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

    pub fn connect_tab(
        &mut self,
        tab_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

        cx.spawn_in(window, async move |this, cx| {
            if let Ok(result) = rx.await {
                let _ = this.update_in(cx, |ws, window, cx| {
                    let t = i18n();
                    if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.is_connecting = false;
                        match result {
                            Ok((backend, initial_path, entries)) => {
                                if let Some(new_sec) = backend.exported_secret() {
                                    tab.form.secret_input.update(cx, |s, cx| {
                                        s.set_value(new_sec.clone(), window, cx)
                                    });
                                    if let Some(bm_id) = tab.form.bookmark_id {
                                        let _ = ws.bookmark_store.set_secret(bm_id, &new_sec);
                                    }
                                }
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
            match backend.list_dir(&path).await {
                Ok(entries) => {
                    let _ = tx.send(Ok((entries, None)));
                }
                Err(primary_err) => {
                    let fallback = LocalFileSystem::initial_local_dir();
                    if path != fallback {
                        if let Ok(fallback_entries) = backend.list_dir(&fallback).await {
                            let _ = tx.send(Ok((fallback_entries, Some(fallback))));
                            return;
                        }
                    }
                    let _ = tx.send(Err(primary_err));
                }
            }
        });

        cx.spawn(async move |this, cx| {
            if let Ok(res) = rx.await {
                let _ = this.update(cx, |ws, cx| {
                    if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                        match res {
                            Ok((entries, fallback_opt)) => {
                                if let Some(fallback_dir) = fallback_opt {
                                    tab.local_path = fallback_dir.clone();
                                    tab.status_message =
                                        Some(i18n().msg_local_dir_fallback(&fallback_dir));
                                    let _ = ws.preferences_store.set_last_local_dir(fallback_dir);
                                }
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
        let current = self.tabs[tab_idx].local_path.clone();
        let (effective_path, fell_back) = ensure_valid_local_dir_or_fallback(&current);
        if fell_back {
            self.tabs[tab_idx].local_path = effective_path.clone();
            self.tabs[tab_idx].status_message =
                Some(i18n().msg_local_dir_fallback(&effective_path));
        }
        let is_virtual_drive_root = matches!(effective_path.as_str(), "" | "/" | "\\");
        if !is_virtual_drive_root {
            let _ = self
                .preferences_store
                .set_last_local_dir(effective_path.clone());
        }
        self.tabs[tab_idx]
            .local_path_input
            .update(cx, |s, cx| s.set_value(effective_path, window, cx));
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
            .when_some(
                match &self.update_state {
                    UpdateCheckState::UpdateAvailable(info) => Some(info.clone()),
                    _ => None,
                },
                |el, info| {
                    let url = info.html_url.clone();
                    el.child(
                        Button::new("top-update-available-btn")
                            .xsmall()
                            .primary()
                            .icon(IconName::Download)
                            .label(t.titlebar_update_badge_label(&info.version))
                            .tooltip(t.titlebar_update_badge_tooltip(&info.version))
                            .on_click(cx.listener(move |_this, _ev, _window, _cx| {
                                Self::open_url_in_browser(&url);
                            })),
                    )
                },
            )
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
    fn test_form_field_values_from_bookmark_and_blank_reset_clears_all_credentials_and_fields() {
        let bm_s3 = Bookmark {
            id: Uuid::new_v4(),
            name: "Prod OSS".into(),
            config: ProtocolConfig::S3(S3Config {
                preset: S3Preset::AlibabaOss,
                endpoint: "https://oss-cn-shanghai.aliyuncs.com".into(),
                region: "cn-shanghai".into(),
                access_key_id: "LTAI_PROD_KEY".into(),
                bucket: Some("prod-bucket".into()),
                initial_prefix: "/assets".into(),
                addressing_style: S3AddressingStyle::VirtualHosted,
            }),
        };

        let loaded = FormFieldValues::from_bookmark(&bm_s3, "TOP_SECRET_KEY_123");
        assert_eq!(loaded.bookmark_id, Some(bm_s3.id));
        assert_eq!(loaded.name, "Prod OSS");
        assert_eq!(loaded.secret, "TOP_SECRET_KEY_123");
        assert_eq!(loaded.s3_access_key, "LTAI_PROD_KEY");
        assert_eq!(loaded.s3_bucket, "prod-bucket");
        assert_eq!(loaded.initial_path, "/assets");

        let reset = FormFieldValues::blank_for_protocol(loaded.protocol);
        assert_eq!(reset.bookmark_id, None);
        assert!(reset.name.is_empty());
        assert!(reset.secret.is_empty());
        assert!(reset.s3_access_key.is_empty());
        assert!(reset.s3_bucket.is_empty());
        assert!(reset.sftp_host.is_empty());
        assert!(reset.ftp_host.is_empty());
        assert!(reset.smb_server.is_empty());
        assert!(reset.webdav_endpoint.is_empty());
        assert!(reset.gdrive_proxy.is_empty());
        assert!(reset.onedrive_proxy.is_empty());
        assert_eq!(reset.initial_path, "/");
    }

    #[test]
    fn test_form_field_values_to_protocol_config_all_protocols() {
        let mut ftp_vals = FormFieldValues::blank_for_protocol(ProtocolKind::Ftp);
        ftp_vals.ftp_host = "ftp.example.com".into();
        ftp_vals.ftp_port = "2121".into();
        ftp_vals.ftp_user = "deploy".into();
        ftp_vals.ftp_use_tls = true;
        ftp_vals.initial_path = "/pub".into();
        match ftp_vals.to_protocol_config() {
            ProtocolConfig::Ftp(c) => {
                assert_eq!(c.host, "ftp.example.com");
                assert_eq!(c.port, 2121);
                assert_eq!(c.username, "deploy");
                assert!(c.use_tls);
                assert_eq!(c.initial_path, "/pub");
            }
            _ => panic!("expected FTP config"),
        }

        let mut dav_vals = FormFieldValues::blank_for_protocol(ProtocolKind::WebDav);
        dav_vals.webdav_endpoint = "https://dav.example.com/remote.php/dav".into();
        dav_vals.webdav_user = "alice".into();
        dav_vals.initial_path = "/notes".into();
        match dav_vals.to_protocol_config() {
            ProtocolConfig::WebDav(c) => {
                assert_eq!(c.endpoint, "https://dav.example.com/remote.php/dav");
                assert_eq!(c.username, "alice");
                assert_eq!(c.initial_path, "/notes");
            }
            _ => panic!("expected WebDAV config"),
        }

        let mut gdrive_vals = FormFieldValues::blank_for_protocol(ProtocolKind::GoogleDrive);
        gdrive_vals.gdrive_client_id = "my-cid".into();
        gdrive_vals.gdrive_client_secret = "my-csec".into();
        gdrive_vals.gdrive_proxy = "socks5://127.0.0.1:1080".into();
        match gdrive_vals.to_protocol_config() {
            ProtocolConfig::GoogleDrive(c) => {
                assert_eq!(c.client_id, "my-cid");
                assert_eq!(c.client_secret.as_deref(), Some("my-csec"));
                assert_eq!(c.proxy_url.as_deref(), Some("socks5://127.0.0.1:1080"));
            }
            _ => panic!("expected GoogleDrive config"),
        }

        let mut od_vals = FormFieldValues::blank_for_protocol(ProtocolKind::OneDrive);
        od_vals.onedrive_proxy = "http://127.0.0.1:7890".into();
        match od_vals.to_protocol_config() {
            ProtocolConfig::OneDrive(c) => {
                assert!(c.client_id.is_empty());
                assert_eq!(c.proxy_url.as_deref(), Some("http://127.0.0.1:7890"));
            }
            _ => panic!("expected OneDrive config"),
        }
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

    #[test]
    fn test_ensure_valid_local_dir_or_fallback_recovers_from_deleted_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let valid_dir = tmp.path().join("my_folder");
        std::fs::create_dir_all(&valid_dir).unwrap();
        let valid_str = valid_dir.to_string_lossy().to_string();

        let (resolved, fell_back) = ensure_valid_local_dir_or_fallback(&valid_str);
        assert!(!fell_back);
        assert_eq!(resolved, valid_str);

        std::fs::remove_dir_all(&valid_dir).unwrap();
        let (fallback_path, fell_back_after_delete) =
            ensure_valid_local_dir_or_fallback(&valid_str);
        assert!(fell_back_after_delete);
        assert_ne!(fallback_path, valid_str);
        assert!(std::fs::metadata(&fallback_path)
            .map(|m| m.is_dir())
            .unwrap_or(false));
    }
}
