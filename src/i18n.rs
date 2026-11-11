use crate::models::{S3AddressingStyle, S3Preset};
use crate::services::preferences::{
    AppLanguage, LocalStartupDirMode, S3UploadAcl, ThemePreference,
};
use std::sync::atomic::{AtomicU8, Ordering};

static ACTIVE_LANG: AtomicU8 = AtomicU8::new(0);

pub fn current_language() -> AppLanguage {
    match ACTIVE_LANG.load(Ordering::Relaxed) {
        1 => AppLanguage::EnUs,
        _ => AppLanguage::ZhCn,
    }
}

pub fn set_current_language(lang: AppLanguage) {
    let val = match lang {
        AppLanguage::ZhCn => 0,
        AppLanguage::EnUs => 1,
    };
    ACTIVE_LANG.store(val, Ordering::Relaxed);
    gpui_component::set_locale(lang.gpui_locale());
}

#[derive(Debug, Clone, Copy)]
pub struct I18n {
    pub lang: AppLanguage,
}

pub fn i18n() -> I18n {
    I18n {
        lang: current_language(),
    }
}

impl I18n {
    pub fn new(lang: AppLanguage) -> Self {
        Self { lang }
    }

    pub fn is_zh(&self) -> bool {
        self.lang == AppLanguage::ZhCn
    }

    // --- TitleBar & Tabs ---
    pub fn default_tab_title(&self) -> &'static str {
        if self.is_zh() {
            "快速连接"
        } else {
            "Quick Connect"
        }
    }

    pub fn is_default_tab_title(title: &str) -> bool {
        matches!(title, "快速连接" | "Quick Connect")
    }

    pub fn close_tab_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "关闭标签页"
        } else {
            "Close Tab"
        }
    }

    pub fn new_tab_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "新建服务器连接标签页"
        } else {
            "New Connection Tab"
        }
    }

    pub fn transfers_button_label(&self, active_count: usize, total_count: usize) -> String {
        let count = if active_count > 0 {
            active_count
        } else {
            total_count
        };
        if self.is_zh() {
            format!("传输队列 ({count})")
        } else {
            format!("Transfers ({count})")
        }
    }

    pub fn theme_button_label(&self, pref: ThemePreference) -> &'static str {
        match (self.lang, pref) {
            (AppLanguage::ZhCn, ThemePreference::System) => "随系统",
            (AppLanguage::ZhCn, ThemePreference::Light) => "浅色",
            (AppLanguage::ZhCn, ThemePreference::Dark) => "深色",
            (AppLanguage::EnUs, ThemePreference::System) => "Auto",
            (AppLanguage::EnUs, ThemePreference::Light) => "Light",
            (AppLanguage::EnUs, ThemePreference::Dark) => "Dark",
        }
    }

    pub fn theme_button_tooltip(&self, pref: ThemePreference) -> &'static str {
        match (self.lang, pref) {
            (AppLanguage::ZhCn, ThemePreference::System) => "当前主题: 跟随系统 (点击切换浅色/深色)",
            (AppLanguage::ZhCn, ThemePreference::Light) => "当前主题: 浅色模式 (点击切换深色/随系统)",
            (AppLanguage::ZhCn, ThemePreference::Dark) => "当前主题: 深色模式 (点击切换随系统/浅色)",
            (AppLanguage::EnUs, ThemePreference::System) => "Theme: System (Click to cycle Light/Dark)",
            (AppLanguage::EnUs, ThemePreference::Light) => "Theme: Light (Click to cycle Dark/System)",
            (AppLanguage::EnUs, ThemePreference::Dark) => "Theme: Dark (Click to cycle System/Light)",
        }
    }

    pub fn language_button_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "切换语言: 简体中文 / English"
        } else {
            "Switch Language: English / 简体中文"
        }
    }

    pub fn settings_button_label(&self) -> &'static str {
        if self.is_zh() {
            "设置"
        } else {
            "Settings"
        }
    }

    pub fn settings_button_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "打开应用设置与关于"
        } else {
            "Open Settings & About"
        }
    }

    pub fn settings_modal_title(&self) -> &'static str {
        if self.is_zh() {
            "设置"
        } else {
            "Settings"
        }
    }

    pub fn settings_tab_general(&self) -> &'static str {
        if self.is_zh() {
            "通用"
        } else {
            "General"
        }
    }

    pub fn settings_tab_about(&self) -> &'static str {
        if self.is_zh() {
            "关于"
        } else {
            "About"
        }
    }

    pub fn settings_section_theme(&self) -> &'static str {
        if self.is_zh() {
            "外观主题"
        } else {
            "Appearance Theme"
        }
    }

    pub fn settings_section_theme_desc(&self) -> &'static str {
        if self.is_zh() {
            "选择界面配色方案，支持跟随操作系统自动切换"
        } else {
            "Choose the interface color scheme or follow your OS appearance"
        }
    }

    pub fn theme_option_title(&self, pref: ThemePreference) -> &'static str {
        match (self.lang, pref) {
            (AppLanguage::ZhCn, ThemePreference::System) => "跟随系统",
            (AppLanguage::ZhCn, ThemePreference::Light) => "浅色模式",
            (AppLanguage::ZhCn, ThemePreference::Dark) => "深色模式",
            (AppLanguage::EnUs, ThemePreference::System) => "System",
            (AppLanguage::EnUs, ThemePreference::Light) => "Light",
            (AppLanguage::EnUs, ThemePreference::Dark) => "Dark",
        }
    }

    pub fn settings_section_language(&self) -> &'static str {
        if self.is_zh() {
            "界面语言"
        } else {
            "Interface Language"
        }
    }

    pub fn settings_section_language_desc(&self) -> &'static str {
        if self.is_zh() {
            "切换应用显示的语言，修改后立即生效"
        } else {
            "Switch display language; changes take effect immediately"
        }
    }

    pub fn settings_section_local_dir(&self) -> &'static str {
        if self.is_zh() {
            "本机初始目录"
        } else {
            "Local Initial Directory"
        }
    }

    pub fn settings_section_local_dir_desc(&self) -> &'static str {
        if self.is_zh() {
            "设置启动或新建标签页时的默认本地路径（若原目录被删除将自动回退至系统主目录）"
        } else {
            "Default local directory for new tabs (automatically falls back to home directory if missing)"
        }
    }

    pub fn local_dir_mode_title(&self, mode: LocalStartupDirMode) -> &'static str {
        match (self.lang, mode) {
            (AppLanguage::ZhCn, LocalStartupDirMode::RememberLast) => "记住上次目录",
            (AppLanguage::ZhCn, LocalStartupDirMode::Custom) => "指定初始目录",
            (AppLanguage::EnUs, LocalStartupDirMode::RememberLast) => "Remember Last",
            (AppLanguage::EnUs, LocalStartupDirMode::Custom) => "Custom Folder",
        }
    }

    pub fn placeholder_custom_local_dir(&self) -> &'static str {
        if self.is_zh() {
            "输入本地文件夹路径，或点击右侧按钮浏览选择..."
        } else {
            "Enter local folder path, or click Browse..."
        }
    }

    pub fn browse_folder_btn(&self) -> &'static str {
        if self.is_zh() {
            "选择目录..."
        } else {
            "Browse..."
        }
    }

    pub fn msg_local_dir_fallback(&self, fallback: &str) -> String {
        if self.is_zh() {
            format!("原本地目录不存在或已被删除，已自动回退至: {fallback}")
        } else {
            format!("Local directory not found; fell back to: {fallback}")
        }
    }

    pub fn settings_section_s3_acl(&self) -> &'static str {
        if self.is_zh() {
            "S3 默认上传权限 (Canned ACL)"
        } else {
            "S3 Default Upload Permissions (Canned ACL)"
        }
    }

    pub fn settings_section_s3_acl_desc(&self) -> &'static str {
        if self.is_zh() {
            "设置 S3 协议上传文件时附带的默认 x-amz-acl 权限头（若存储桶禁用了 ACL 请选择「跟随存储桶默认」）"
        } else {
            "Default x-amz-acl header for S3 uploads (choose \"Bucket Default\" if Bucket Owner Enforced is enabled)"
        }
    }

    pub fn s3_acl_option_title(&self, acl: S3UploadAcl) -> &'static str {
        match (self.lang, acl) {
            (AppLanguage::ZhCn, S3UploadAcl::BucketDefault) => "跟随存储桶默认 (不发送 ACL)",
            (AppLanguage::ZhCn, S3UploadAcl::Private) => "私有 (private)",
            (AppLanguage::ZhCn, S3UploadAcl::PublicRead) => "公共读 (public-read)",
            (AppLanguage::ZhCn, S3UploadAcl::PublicReadWrite) => "公共读写 (public-read-write)",
            (AppLanguage::ZhCn, S3UploadAcl::AuthenticatedRead) => {
                "认证用户读 (authenticated-read)"
            }
            (AppLanguage::ZhCn, S3UploadAcl::BucketOwnerRead) => {
                "桶拥有者读 (bucket-owner-read)"
            }
            (AppLanguage::ZhCn, S3UploadAcl::BucketOwnerFullControl) => {
                "桶拥有者完全控制 (bucket-owner-full-control)"
            }
            (AppLanguage::EnUs, S3UploadAcl::BucketDefault) => "Bucket Default (No ACL)",
            (AppLanguage::EnUs, S3UploadAcl::Private) => "Private (private)",
            (AppLanguage::EnUs, S3UploadAcl::PublicRead) => "Public Read (public-read)",
            (AppLanguage::EnUs, S3UploadAcl::PublicReadWrite) => {
                "Public Read-Write (public-read-write)"
            }
            (AppLanguage::EnUs, S3UploadAcl::AuthenticatedRead) => {
                "Authenticated Read (authenticated-read)"
            }
            (AppLanguage::EnUs, S3UploadAcl::BucketOwnerRead) => {
                "Bucket Owner Read (bucket-owner-read)"
            }
            (AppLanguage::EnUs, S3UploadAcl::BucketOwnerFullControl) => {
                "Bucket Owner Full Control (bucket-owner-full-control)"
            }
        }
    }

    pub fn about_tagline(&self) -> &'static str {
        if self.is_zh() {
            "基于 Rust 与 GPUI 构建的现代化跨协议文件传输工作台"
        } else {
            "Modern cross-protocol file transfer studio built with Rust & GPUI"
        }
    }

    pub fn about_protocols_label(&self) -> &'static str {
        if self.is_zh() {
            "支持协议"
        } else {
            "Supported Protocols"
        }
    }

    pub fn about_update_section_title(&self) -> &'static str {
        if self.is_zh() {
            "版本与更新"
        } else {
            "Version & Updates"
        }
    }

    pub fn about_update_placeholder(&self) -> &'static str {
        self.about_update_idle_desc()
    }

    pub fn about_update_idle_desc(&self) -> &'static str {
        if self.is_zh() {
            "每日自动检查 GitHub Releases 更新，也可随时手动检查"
        } else {
            "Automatically checks GitHub Releases daily, or check manually anytime"
        }
    }

    pub fn about_update_checking(&self) -> &'static str {
        if self.is_zh() {
            "正在检查 GitHub Releases 最新版本..."
        } else {
            "Checking GitHub Releases for updates..."
        }
    }

    pub fn about_update_up_to_date(&self, current_version: &str) -> String {
        if self.is_zh() {
            format!("当前已是最新版本 (v{current_version})")
        } else {
            format!("You are on the latest version (v{current_version})")
        }
    }

    pub fn about_update_available(&self, new_version: &str) -> String {
        if self.is_zh() {
            format!("发现新版本 v{new_version}，点击前往 GitHub 下载")
        } else {
            format!("New version v{new_version} available on GitHub")
        }
    }

    pub fn about_update_failed(&self, err: &str) -> String {
        if self.is_zh() {
            format!("检查更新失败: {err}")
        } else {
            format!("Update check failed: {err}")
        }
    }

    pub fn about_check_update_btn(&self, is_checking: bool) -> &'static str {
        match (self.is_zh(), is_checking) {
            (true, true) => "检查中...",
            (true, false) => "检查更新",
            (false, true) => "Checking...",
            (false, false) => "Check for Updates",
        }
    }

    pub fn about_download_update_btn(&self, version: &str) -> String {
        if self.is_zh() {
            format!("前往下载 v{version}")
        } else {
            format!("Download v{version}")
        }
    }

    pub fn titlebar_update_badge_label(&self, version: &str) -> String {
        if self.is_zh() {
            format!("新版本 v{version}")
        } else {
            format!("Update v{version}")
        }
    }

    pub fn titlebar_update_badge_tooltip(&self, version: &str) -> String {
        if self.is_zh() {
            format!("发现新版本 v{version}，点击前往 GitHub Release 下载")
        } else {
            format!("New version v{version} available — click to open GitHub Release")
        }
    }

    // --- Form Placeholders ---
    pub fn placeholder_search_bookmarks(&self) -> &'static str {
        if self.is_zh() {
            "搜索收藏夹服务器 / IP / Bucket..."
        } else {
            "Search favorites by name / IP / Bucket..."
        }
    }

    pub fn placeholder_conn_name(&self) -> &'static str {
        if self.is_zh() {
            "例如: 阿里云 OSS 生产桶"
        } else {
            "e.g. Production S3 Bucket"
        }
    }

    pub fn placeholder_sftp_host(&self) -> &'static str {
        if self.is_zh() {
            "例如: 47.242.10.20"
        } else {
            "e.g. 47.242.10.20"
        }
    }

    pub fn placeholder_sftp_key_path(&self) -> &'static str {
        if self.is_zh() {
            "选填: ~/.ssh/id_ed25519"
        } else {
            "Optional: ~/.ssh/id_ed25519"
        }
    }

    pub fn placeholder_s3_bucket(&self) -> &'static str {
        if self.is_zh() {
            "选填: prod-bucket (留空则列举全部 Bucket)"
        } else {
            "Optional: prod-bucket (leave empty to list all)"
        }
    }

    pub fn placeholder_smb_server(&self) -> &'static str {
        if self.is_zh() {
            "例如: 192.168.1.50 或 nas.local"
        } else {
            "e.g. 192.168.1.50 or nas.local"
        }
    }

    pub fn placeholder_smb_share(&self) -> &'static str {
        if self.is_zh() {
            "例如: team_share"
        } else {
            "e.g. team_share"
        }
    }

    pub fn placeholder_smb_domain(&self) -> &'static str {
        if self.is_zh() {
            "选填: WORKGROUP / CORP"
        } else {
            "Optional: WORKGROUP / CORP"
        }
    }

    pub fn placeholder_secret(&self) -> &'static str {
        if self.is_zh() {
            "密码 / 私钥口令 / Secret Access Key"
        } else {
            "Password / Key Passphrase / Secret Access Key"
        }
    }

    pub fn placeholder_filter_local(&self) -> &'static str {
        if self.is_zh() {
            "过滤本地文件..."
        } else {
            "Filter local files..."
        }
    }

    pub fn placeholder_filter_remote(&self) -> &'static str {
        if self.is_zh() {
            "过滤远程文件..."
        } else {
            "Filter remote files..."
        }
    }

    pub fn placeholder_inline_name(&self) -> &'static str {
        if self.is_zh() {
            "输入名称..."
        } else {
            "Enter name..."
        }
    }

    // --- New Tab View (Favorites + Quick Connect Inspector) ---
    pub fn favorites_title(&self) -> &'static str {
        if self.is_zh() {
            "收藏夹服务器 (Favorites)"
        } else {
            "Favorite Servers"
        }
    }

    pub fn new_blank_conn_btn(&self) -> &'static str {
        if self.is_zh() {
            "新建空白连接"
        } else {
            "New Connection"
        }
    }

    pub fn favorites_footer_tip(&self) -> &'static str {
        if self.is_zh() {
            "提示: 单击卡片在右侧编辑参数 · 双击卡片立即连接服务器"
        } else {
            "Tip: Click a card to inspect/edit · Double-click to connect immediately"
        }
    }

    pub fn favorites_empty_title(&self) -> &'static str {
        if self.is_zh() {
            "尚无匹配的收藏夹站点"
        } else {
            "No Saved Favorites Found"
        }
    }

    pub fn favorites_empty_desc(&self) -> &'static str {
        if self.is_zh() {
            "在右侧配置好服务器后点击「保存到收藏夹」，即可在此一键双击直连。"
        } else {
            "Configure a server on the right and click \"Save to Favorites\" for instant access."
        }
    }

    pub fn connect_short_btn(&self) -> &'static str {
        if self.is_zh() {
            "连接"
        } else {
            "Connect"
        }
    }

    pub fn remove_favorite_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "从收藏夹移除"
        } else {
            "Remove from Favorites"
        }
    }

    pub fn inspector_title(&self, has_bookmark: bool) -> &'static str {
        match (self.is_zh(), has_bookmark) {
            (true, true) => "编辑收藏夹连接 (Inspector)",
            (true, false) => "快速连接服务器 (Quick Connect)",
            (false, true) => "Edit Saved Connection",
            (false, false) => "Quick Connect Server",
        }
    }

    pub fn keyring_badge_text(&self) -> &'static str {
        if self.is_zh() {
            "系统密钥链加密保护"
        } else {
            "OS Keychain Encrypted"
        }
    }

    pub fn proto_sftp_label(&self) -> &'static str {
        "SSH / SFTP"
    }

    pub fn proto_s3_label(&self) -> &'static str {
        if self.is_zh() {
            "S3 通用对象存储"
        } else {
            "S3 Object Storage"
        }
    }

    pub fn proto_smb_label(&self) -> &'static str {
        if self.is_zh() {
            "SMB 网络共享"
        } else {
            "SMB Network Share"
        }
    }

    pub fn label_display_name(&self) -> &'static str {
        if self.is_zh() {
            "连接显示名称 (选填，留空自动按地址生成)"
        } else {
            "Display Name (Optional, auto-generated if empty)"
        }
    }

    pub fn label_sftp_host(&self) -> &'static str {
        if self.is_zh() {
            "SSH 主机地址 (Host / IP)"
        } else {
            "SSH Host / IP"
        }
    }

    pub fn label_port(&self) -> &'static str {
        if self.is_zh() {
            "端口 (Port)"
        } else {
            "Port"
        }
    }

    pub fn label_username(&self) -> &'static str {
        if self.is_zh() {
            "用户名 (Username)"
        } else {
            "Username"
        }
    }

    pub fn label_sftp_secret(&self) -> &'static str {
        if self.is_zh() {
            "密码 / 私钥口令"
        } else {
            "Password / Key Passphrase"
        }
    }

    pub fn label_sftp_key_path(&self) -> &'static str {
        if self.is_zh() {
            "SSH 私钥路径 (支持 ~/ 展开，留空使用密码)"
        } else {
            "SSH Private Key Path (Supports ~/, leave empty for password)"
        }
    }

    pub fn label_initial_remote_dir(&self) -> &'static str {
        if self.is_zh() {
            "初始远程目录"
        } else {
            "Initial Remote Path"
        }
    }

    pub fn label_s3_presets(&self) -> &'static str {
        if self.is_zh() {
            "通用 S3 协议厂商模板 (点击快速填充 Endpoint，支持任意兼容 S3 服务)"
        } else {
            "S3 Provider Presets (Click to auto-fill Endpoint for any S3-compatible service)"
        }
    }

    pub fn s3_preset_label(&self, preset: S3Preset) -> &'static str {
        match (self.is_zh(), preset) {
            (true, S3Preset::AlibabaOss) => "阿里云 OSS",
            (true, S3Preset::TencentCos) => "腾讯云 COS",
            (true, S3Preset::Minio) => "MinIO / 私有云",
            (true, S3Preset::AwsS3) => "AWS S3",
            (true, S3Preset::Custom) => "自定义 S3 Endpoint",
            (false, S3Preset::AlibabaOss) => "Alibaba Cloud OSS",
            (false, S3Preset::TencentCos) => "Tencent Cloud COS",
            (false, S3Preset::Minio) => "MinIO / Self-Hosted",
            (false, S3Preset::AwsS3) => "AWS S3",
            (false, S3Preset::Custom) => "Custom S3 Endpoint",
        }
    }

    pub fn s3_addressing_label(&self, style: S3AddressingStyle) -> &'static str {
        match (self.is_zh(), style) {
            (true, S3AddressingStyle::VirtualHosted) => "Virtual-Hosted (阿里云/腾讯云/AWS)",
            (true, S3AddressingStyle::PathStyle) => "Path-Style (MinIO/自建)",
            (false, S3AddressingStyle::VirtualHosted) => "Virtual-Hosted (AWS / OSS / COS)",
            (false, S3AddressingStyle::PathStyle) => "Path-Style (MinIO / Custom)",
        }
    }

    pub fn label_s3_endpoint(&self) -> &'static str {
        if self.is_zh() {
            "服务端点 (Endpoint URL)"
        } else {
            "Endpoint URL"
        }
    }

    pub fn label_s3_region(&self) -> &'static str {
        if self.is_zh() {
            "区域 (Region)"
        } else {
            "Region"
        }
    }

    pub fn label_s3_addressing_mode(&self) -> &'static str {
        if self.is_zh() {
            "S3 寻址模式 (Addressing Style)"
        } else {
            "S3 Addressing Style"
        }
    }

    pub fn label_s3_bucket(&self) -> &'static str {
        if self.is_zh() {
            "指定单桶 Bucket (选填，无 ListBuckets 权限时必填)"
        } else {
            "Bucket Name (Optional, required if no ListBuckets permission)"
        }
    }

    pub fn label_s3_prefix(&self) -> &'static str {
        if self.is_zh() {
            "初始目录前缀"
        } else {
            "Initial Prefix"
        }
    }

    pub fn label_smb_server(&self) -> &'static str {
        if self.is_zh() {
            "SMB 服务器地址 (Server / NAS IP)"
        } else {
            "SMB Server / NAS IP"
        }
    }

    pub fn label_smb_share(&self) -> &'static str {
        if self.is_zh() {
            "共享名称 (Share)"
        } else {
            "Share Name"
        }
    }

    pub fn label_password(&self) -> &'static str {
        if self.is_zh() {
            "密码 (Password)"
        } else {
            "Password"
        }
    }

    pub fn label_smb_domain(&self) -> &'static str {
        if self.is_zh() {
            "工作组 / 域 (Domain，选填)"
        } else {
            "Workgroup / Domain (Optional)"
        }
    }

    pub fn test_conn_btn(&self) -> &'static str {
        if self.is_zh() {
            "测试连接"
        } else {
            "Test Connection"
        }
    }

    pub fn save_bookmark_btn(&self) -> &'static str {
        if self.is_zh() {
            "保存到收藏夹"
        } else {
            "Save to Favorites"
        }
    }

    pub fn connect_now_btn(&self, is_connecting: bool) -> &'static str {
        match (self.is_zh(), is_connecting) {
            (true, true) => "正在连接...",
            (true, false) => "立即连接服务器",
            (false, true) => "Connecting...",
            (false, false) => "Connect Server",
        }
    }

    // --- Dual Pane View ---
    pub fn favorites_back_btn(&self) -> &'static str {
        if self.is_zh() {
            "收藏夹"
        } else {
            "Favorites"
        }
    }

    pub fn favorites_back_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "断开并返回收藏夹与连接面板"
        } else {
            "Disconnect and return to Favorites"
        }
    }

    pub fn sync_refresh_both_btn(&self) -> &'static str {
        if self.is_zh() {
            "同步刷新双栏"
        } else {
            "Refresh Both Panes"
        }
    }

    pub fn toggle_hidden_btn(&self, show_hidden: bool) -> &'static str {
        match (self.is_zh(), show_hidden) {
            (true, true) => "显示隐藏文件",
            (true, false) => "隐藏点文件",
            (false, true) => "Showing Hidden",
            (false, false) => "Hide Dotfiles",
        }
    }

    pub fn disconnect_btn(&self) -> &'static str {
        if self.is_zh() {
            "断开连接"
        } else {
            "Disconnect"
        }
    }

    pub fn local_pane_title(&self) -> &'static str {
        if self.is_zh() {
            "本地文件系统 (Local)"
        } else {
            "Local Filesystem"
        }
    }

    pub fn remote_pane_title(&self, title: &str) -> String {
        if self.is_zh() {
            format!("远程 · {title}")
        } else {
            format!("Remote · {title}")
        }
    }

    pub fn nav_up_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "返回上级目录"
        } else {
            "Go to Parent Directory"
        }
    }

    pub fn refresh_dir_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "刷新当前目录"
        } else {
            "Refresh Directory"
        }
    }

    pub fn upload_btn(&self) -> &'static str {
        if self.is_zh() {
            "上传"
        } else {
            "Upload"
        }
    }

    pub fn download_btn(&self) -> &'static str {
        if self.is_zh() {
            "下载"
        } else {
            "Download"
        }
    }

    pub fn upload_selected_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "上传选中项到右侧远程目录"
        } else {
            "Upload selected item to remote directory"
        }
    }

    pub fn download_selected_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "下载选中项到左侧本地目录"
        } else {
            "Download selected item to local directory"
        }
    }

    pub fn new_folder_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "在当前目录新建文件夹"
        } else {
            "Create New Folder"
        }
    }

    pub fn rename_selected_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "重命名选中项"
        } else {
            "Rename Selected Item"
        }
    }

    pub fn delete_selected_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "删除选中项"
        } else {
            "Delete Selected Item"
        }
    }

    pub fn go_path_btn(&self) -> &'static str {
        if self.is_zh() {
            "跳转"
        } else {
            "Go"
        }
    }

    pub fn edit_path_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "手动输入路径跳转"
        } else {
            "Enter Path Manually"
        }
    }

    pub fn breadcrumb_local_drives_root(&self) -> &'static str {
        if self.is_zh() {
            "磁盘"
        } else {
            "Drives"
        }
    }

    pub fn breadcrumb_remote_root(&self) -> &'static str {
        if self.is_zh() {
            "根目录"
        } else {
            "Root"
        }
    }

    pub fn err_rename_empty(&self) -> &'static str {
        if self.is_zh() {
            "新名称不能为空"
        } else {
            "New name cannot be empty"
        }
    }

    pub fn err_rename_invalid_sep(&self) -> &'static str {
        if self.is_zh() {
            "新名称不能包含路径分隔符"
        } else {
            "New name cannot contain path separators"
        }
    }

    pub fn s3_all_buckets_summary(&self) -> &'static str {
        if self.is_zh() {
            "全部 Bucket"
        } else {
            "All Buckets"
        }
    }

    pub fn action_modal_new_folder_title(&self, is_local: bool) -> &'static str {
        match (self.is_zh(), is_local) {
            (true, true) => "新建本地文件夹",
            (true, false) => "新建远程文件夹",
            (false, true) => "New Local Folder",
            (false, false) => "New Remote Folder",
        }
    }

    pub fn action_modal_new_folder_desc(&self, parent_path: &str) -> String {
        if self.is_zh() {
            format!("将在目录 {parent_path} 下创建新文件夹")
        } else {
            format!("Create a new folder inside {parent_path}")
        }
    }

    pub fn action_modal_rename_title(&self, is_local: bool) -> &'static str {
        match (self.is_zh(), is_local) {
            (true, true) => "重命名本地项目",
            (true, false) => "重命名远程项目",
            (false, true) => "Rename Local Item",
            (false, false) => "Rename Remote Item",
        }
    }

    pub fn action_modal_rename_desc(&self, original_name: &str) -> String {
        if self.is_zh() {
            format!("原名称: {original_name}")
        } else {
            format!("Original name: {original_name}")
        }
    }

    pub fn action_modal_delete_title(&self, is_local: bool) -> &'static str {
        match (self.is_zh(), is_local) {
            (true, true) => "删除本地项目",
            (true, false) => "删除远程项目",
            (false, true) => "Delete Local Item",
            (false, false) => "Delete Remote Item",
        }
    }

    pub fn action_modal_delete_warning(&self) -> &'static str {
        if self.is_zh() {
            "此操作不可撤销，请确认是否继续。"
        } else {
            "This action cannot be undone."
        }
    }

    pub fn inline_new_folder_label(&self) -> &'static str {
        if self.is_zh() {
            "新建文件夹:"
        } else {
            "New Folder:"
        }
    }

    pub fn create_btn(&self) -> &'static str {
        if self.is_zh() {
            "创建"
        } else {
            "Create"
        }
    }

    pub fn confirm_btn(&self) -> &'static str {
        if self.is_zh() {
            "确定"
        } else {
            "OK"
        }
    }

    pub fn cancel_btn(&self) -> &'static str {
        if self.is_zh() {
            "取消"
        } else {
            "Cancel"
        }
    }

    pub fn inline_rename_label(&self, name: &str) -> String {
        if self.is_zh() {
            format!("重命名「{name}」:")
        } else {
            format!("Rename \"{name}\":")
        }
    }

    pub fn inline_delete_confirm_prompt(&self, is_dir: bool, name: &str) -> String {
        if self.is_zh() {
            let kind = if is_dir {
                "文件夹及其全部内容"
            } else {
                "文件"
            };
            format!("确认永久删除{kind}「{name}」吗？")
        } else if is_dir {
            format!("Permanently delete folder \"{name}\" and all its contents?")
        } else {
            format!("Permanently delete file \"{name}\"?")
        }
    }

    pub fn confirm_delete_btn(&self) -> &'static str {
        if self.is_zh() {
            "确认删除"
        } else {
            "Delete"
        }
    }

    pub fn col_name(&self) -> &'static str {
        if self.is_zh() {
            "名称"
        } else {
            "Name"
        }
    }

    pub fn col_modified(&self) -> &'static str {
        if self.is_zh() {
            "修改时间"
        } else {
            "Date Modified"
        }
    }

    pub fn col_size(&self) -> &'static str {
        if self.is_zh() {
            "大小"
        } else {
            "Size"
        }
    }

    pub fn col_actions(&self) -> &'static str {
        if self.is_zh() {
            "快捷操作"
        } else {
            "Actions"
        }
    }

    pub fn empty_directory_text(&self) -> &'static str {
        if self.is_zh() {
            "当前目录为空"
        } else {
            "Directory is empty"
        }
    }

    pub fn parent_dir_row_text(&self) -> &'static str {
        if self.is_zh() {
            ".. (双击返回上级目录)"
        } else {
            ".. (Double-click for parent directory)"
        }
    }

    pub fn pane_footer_summary(&self, dirs: usize, files: usize, bytes_str: &str) -> String {
        if self.is_zh() {
            format!("{dirs} 个目录, {files} 个文件 · 共 {bytes_str}")
        } else {
            format!("{dirs} folders, {files} files · {bytes_str} total")
        }
    }

    pub fn pane_footer_selection(&self, selected_name: Option<&str>) -> String {
        match (self.is_zh(), selected_name) {
            (true, Some(name)) => format!("已选中: {name}"),
            (true, None) => "单击选中 · 双击目录进入 / 双击文件传输".to_string(),
            (false, Some(name)) => format!("Selected: {name}"),
            (false, None) => "Click to select · Double-click folder to open / file to transfer".to_string(),
        }
    }

    pub fn pane_footer_multi_selection(&self, count: usize, primary_name: Option<&str>) -> String {
        if count <= 1 {
            return self.pane_footer_selection(primary_name);
        }
        match (self.is_zh(), primary_name) {
            (true, Some(name)) => format!("已选 {count} 项 ({name} 等)"),
            (true, None) => format!("已选 {count} 项"),
            (false, Some(name)) => format!("{count} selected ({name}, ...)"),
            (false, None) => format!("{count} selected"),
        }
    }

    pub fn ctx_upload_to_remote(&self) -> &'static str {
        if self.is_zh() {
            "上传到远程"
        } else {
            "Upload to Remote"
        }
    }

    pub fn ctx_download_to_local(&self) -> &'static str {
        if self.is_zh() {
            "下载到本地"
        } else {
            "Download to Local"
        }
    }

    pub fn ctx_open_or_enter(&self) -> &'static str {
        if self.is_zh() {
            "打开"
        } else {
            "Open"
        }
    }

    pub fn ctx_rename(&self) -> &'static str {
        if self.is_zh() {
            "重命名 (F2)"
        } else {
            "Rename (F2)"
        }
    }

    pub fn ctx_new_folder(&self) -> &'static str {
        if self.is_zh() {
            "新建文件夹"
        } else {
            "New Folder"
        }
    }

    pub fn ctx_delete(&self) -> &'static str {
        if self.is_zh() {
            "删除"
        } else {
            "Delete"
        }
    }

    pub fn ctx_copy_path(&self) -> &'static str {
        if self.is_zh() {
            "复制路径"
        } else {
            "Copy Path"
        }
    }

    pub fn ctx_refresh(&self) -> &'static str {
        if self.is_zh() {
            "刷新"
        } else {
            "Refresh"
        }
    }

    pub fn ctx_reveal_in_shell(&self) -> &'static str {
        if self.is_zh() {
            "在文件管理器中显示"
        } else {
            "Reveal in File Manager"
        }
    }

    // --- Transfer Drawer ---
    pub fn drawer_title(&self) -> &'static str {
        if self.is_zh() {
            "传输活动队列 (Transfers)"
        } else {
            "Transfer Queue"
        }
    }

    pub fn drawer_empty_hint(&self) -> &'static str {
        if self.is_zh() {
            "暂无传输任务 — 在上方双栏双击文件或点击上传/下载图标即可加入队列"
        } else {
            "No active transfers — Double-click a file or click Upload/Download above to start"
        }
    }

    pub fn drawer_active_badge(&self, count: usize) -> String {
        if self.is_zh() {
            format!("进行中 {count}")
        } else {
            format!("Active {count}")
        }
    }

    pub fn drawer_completed_badge(&self, count: usize) -> String {
        if self.is_zh() {
            format!("已完成 {count}")
        } else {
            format!("Done {count}")
        }
    }

    pub fn drawer_failed_badge(&self, count: usize) -> String {
        if self.is_zh() {
            format!("失败 {count}")
        } else {
            format!("Failed {count}")
        }
    }

    pub fn clear_completed_btn(&self) -> &'static str {
        if self.is_zh() {
            "清理已完成"
        } else {
            "Clear Completed"
        }
    }

    pub fn toggle_drawer_btn(&self, expanded: bool) -> &'static str {
        match (self.is_zh(), expanded) {
            (true, true) => "收起",
            (true, false) => "展开队列",
            (false, true) => "Collapse",
            (false, false) => "Expand",
        }
    }

    pub fn status_queued(&self) -> String {
        if self.is_zh() {
            "排队等待".to_string()
        } else {
            "Queued".to_string()
        }
    }

    pub fn status_paused(&self) -> String {
        if self.is_zh() {
            "已暂停".to_string()
        } else {
            "Paused".to_string()
        }
    }

    pub fn status_completed(&self) -> String {
        if self.is_zh() {
            "传输完成".to_string()
        } else {
            "Completed".to_string()
        }
    }

    pub fn status_failed(&self, err: &str) -> String {
        if self.is_zh() {
            format!("失败: {err}")
        } else {
            format!("Failed: {err}")
        }
    }

    pub fn status_cancelled(&self) -> String {
        if self.is_zh() {
            "已取消".to_string()
        } else {
            "Cancelled".to_string()
        }
    }

    pub fn pause_transfer_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "暂停传输"
        } else {
            "Pause Transfer"
        }
    }

    pub fn resume_transfer_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "继续传输"
        } else {
            "Resume Transfer"
        }
    }

    pub fn cancel_transfer_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "取消传输"
        } else {
            "Cancel Transfer"
        }
    }

    // --- Workspace Status Messages ---
    pub fn msg_reset_blank_form(&self) -> String {
        if self.is_zh() {
            "已切换为新建空白服务器配置".to_string()
        } else {
            "Switched to new blank server configuration".to_string()
        }
    }

    pub fn msg_saved_bookmark(&self) -> String {
        if self.is_zh() {
            "已保存至收藏夹（敏感凭据已存入系统安全密钥链）".to_string()
        } else {
            "Saved to Favorites (secret stored in OS keychain)".to_string()
        }
    }

    pub fn msg_save_bookmark_failed(&self, err: &dyn std::fmt::Display) -> String {
        if self.is_zh() {
            format!("保存收藏夹失败: {err}")
        } else {
            format!("Failed to save favorite: {err}")
        }
    }

    pub fn msg_loaded_bookmark(&self, name: &str) -> String {
        if self.is_zh() {
            format!("已载入收藏配置「{name}」")
        } else {
            format!("Loaded favorite \"{name}\"")
        }
    }

    pub fn msg_loaded_bookmark_missing_secret(&self, name: &str) -> String {
        if self.is_zh() {
            format!("已载入「{name}」— 未检测到已保存的密钥，请在右侧补填后点击「存入收藏夹」")
        } else {
            format!("Loaded \"{name}\" — secret not found in keychain, please enter it and click \"Save to Favorites\"")
        }
    }

    pub fn err_s3_endpoint_empty(&self) -> &'static str {
        if self.is_zh() {
            "S3 服务端点 (Endpoint URL) 不能为空"
        } else {
            "S3 Endpoint URL cannot be empty"
        }
    }

    pub fn err_s3_access_key_empty(&self) -> &'static str {
        if self.is_zh() {
            "缺少 Access Key ID / SecretId"
        } else {
            "Missing Access Key ID / SecretId"
        }
    }

    pub fn err_s3_secret_key_empty(&self) -> &'static str {
        if self.is_zh() {
            "缺少 Secret Access Key / SecretKey（请在右侧填入密钥并点击「存入收藏夹」保存）"
        } else {
            "Missing Secret Access Key / SecretKey (enter it on the right and click \"Save to Favorites\")"
        }
    }

    pub fn msg_invalid_conn_params(&self, err: &dyn std::fmt::Display) -> String {
        if self.is_zh() {
            format!("连接参数无效: {err}")
        } else {
            format!("Invalid connection config: {err}")
        }
    }

    pub fn msg_testing_connection(&self) -> String {
        if self.is_zh() {
            "正在测试服务器连接与握手...".to_string()
        } else {
            "Testing server handshake...".to_string()
        }
    }

    pub fn msg_test_conn_ok(&self, root: &str) -> String {
        if self.is_zh() {
            format!("连接测试成功！远程根目录: {root}")
        } else {
            format!("Connection verified! Remote root: {root}")
        }
    }

    pub fn msg_test_conn_failed(&self, err: &dyn std::fmt::Display) -> String {
        if self.is_zh() {
            format!("连接测试失败: {err}")
        } else {
            format!("Connection test failed: {err}")
        }
    }

    pub fn msg_connecting_handshake(&self) -> String {
        if self.is_zh() {
            "正在与远程服务器握手并读取目录...".to_string()
        } else {
            "Connecting to remote server & listing directory...".to_string()
        }
    }

    pub fn msg_connected_ready(&self) -> String {
        if self.is_zh() {
            "连接就绪".to_string()
        } else {
            "Connected".to_string()
        }
    }

    pub fn msg_connect_failed(&self, err: &dyn std::fmt::Display) -> String {
        if self.is_zh() {
            format!("连接失败: {err}")
        } else {
            format!("Connection failed: {err}")
        }
    }

    pub fn msg_bucket_cannot_rename(&self) -> String {
        if self.is_zh() {
            "Bucket 不支持直接重命名".to_string()
        } else {
            "S3 Buckets cannot be renamed directly".to_string()
        }
    }

    pub fn msg_remote_dir_synced(&self) -> String {
        if self.is_zh() {
            "远程目录已同步".to_string()
        } else {
            "Remote directory synced".to_string()
        }
    }

    // --- Import / Export Bookmarks ---
    pub fn import_bookmarks_btn(&self) -> &'static str {
        if self.is_zh() {
            "导入"
        } else {
            "Import"
        }
    }

    pub fn export_bookmarks_btn(&self) -> &'static str {
        if self.is_zh() {
            "导出"
        } else {
            "Export"
        }
    }

    pub fn import_bookmarks_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "从 JSON 备份文件导入连接配置与密钥"
        } else {
            "Import connections and credentials from a JSON backup file"
        }
    }

    pub fn export_bookmarks_tooltip(&self) -> &'static str {
        if self.is_zh() {
            "将收藏夹连接与密钥导出为 JSON 文件以便迁移到其他电脑"
        } else {
            "Export saved connections and credentials to a JSON file for another computer"
        }
    }

    pub fn msg_exported_bookmarks(&self, count: usize, path: &str) -> String {
        if self.is_zh() {
            format!("已成功导出 {count} 个连接配置至: {path}")
        } else {
            format!("Exported {count} connections to: {path}")
        }
    }

    pub fn msg_export_failed(&self, err: &dyn std::fmt::Display) -> String {
        if self.is_zh() {
            format!("导出连接失败: {err}")
        } else {
            format!("Failed to export connections: {err}")
        }
    }

    pub fn msg_imported_bookmarks(&self, count: usize) -> String {
        if self.is_zh() {
            format!("已成功导入 {count} 个连接配置（含密钥凭据）")
        } else {
            format!("Successfully imported {count} connections (with credentials)")
        }
    }

    pub fn msg_import_failed(&self, err: &dyn std::fmt::Display) -> String {
        if self.is_zh() {
            format!("导入连接失败: {err}")
        } else {
            format!("Failed to import connections: {err}")
        }
    }

    // --- Extended Protocols (FTP, WebDAV, Google Drive, OneDrive) ---
    pub fn proto_ftp_label(&self) -> &'static str {
        "FTP / FTPS"
    }

    pub fn proto_webdav_label(&self) -> &'static str {
        "WebDAV"
    }

    pub fn proto_gdrive_label(&self) -> &'static str {
        "Google Drive"
    }

    pub fn proto_onedrive_label(&self) -> &'static str {
        "OneDrive"
    }

    pub fn label_ftp_host(&self) -> &'static str {
        if self.is_zh() {
            "FTP 主机地址 (Host / IP)"
        } else {
            "FTP Host / IP"
        }
    }

    pub fn placeholder_ftp_host(&self) -> &'static str {
        if self.is_zh() {
            "例如: ftp.example.com"
        } else {
            "e.g. ftp.example.com"
        }
    }

    pub fn label_ftp_tls_mode(&self) -> &'static str {
        if self.is_zh() {
            "加密传输模式 (FTP / FTPS)"
        } else {
            "Encryption Mode (FTP / FTPS)"
        }
    }

    pub fn ftp_mode_plain(&self) -> &'static str {
        if self.is_zh() {
            "标准明文 FTP"
        } else {
            "Plain FTP"
        }
    }

    pub fn ftp_mode_tls(&self) -> &'static str {
        if self.is_zh() {
            "FTPS (TLS 加密)"
        } else {
            "FTPS (Explicit/Implicit TLS)"
        }
    }

    pub fn label_webdav_endpoint(&self) -> &'static str {
        if self.is_zh() {
            "WebDAV 服务端点 URL (支持 HTTP / HTTPS)"
        } else {
            "WebDAV Server Endpoint URL (HTTP / HTTPS)"
        }
    }

    pub fn placeholder_webdav_endpoint(&self) -> &'static str {
        if self.is_zh() {
            "例如: https://dav.example.com/remote.php/webdav"
        } else {
            "e.g. https://dav.example.com/remote.php/webdav"
        }
    }

    pub fn label_proxy_url(&self) -> &'static str {
        if self.is_zh() {
            "网络代理地址 (Proxy，支持 HTTP / HTTPS / SOCKS5)"
        } else {
            "Proxy Server URL (Supports HTTP / HTTPS / SOCKS5)"
        }
    }

    pub fn placeholder_proxy_url(&self) -> &'static str {
        if self.is_zh() {
            "选填: http://127.0.0.1:7890 或 socks5://127.0.0.1:1080"
        } else {
            "Optional: http://127.0.0.1:7890 or socks5://127.0.0.1:1080"
        }
    }

    pub fn label_oauth_client_id(&self) -> &'static str {
        if self.is_zh() {
            "OAuth Client ID (选填，留空使用内置桌面端 Client ID)"
        } else {
            "OAuth Client ID (Optional, leave empty for built-in default)"
        }
    }

    pub fn placeholder_oauth_client_id(&self) -> &'static str {
        if self.is_zh() {
            "选填: 自定义 OAuth Client ID"
        } else {
            "Optional: Custom OAuth Client ID"
        }
    }

    pub fn label_oauth_client_secret(&self) -> &'static str {
        if self.is_zh() {
            "OAuth Client Secret (选填)"
        } else {
            "OAuth Client Secret (Optional)"
        }
    }

    pub fn placeholder_oauth_client_secret(&self) -> &'static str {
        if self.is_zh() {
            "选填: 自定义 Client Secret"
        } else {
            "Optional: Custom Client Secret"
        }
    }

    pub fn label_oauth_refresh_token(&self) -> &'static str {
        if self.is_zh() {
            "授权凭证 (Refresh Token，点击下方按钮通过浏览器自动获取)"
        } else {
            "OAuth Refresh Token (Click button below to authorize in browser)"
        }
    }

    pub fn oauth_browser_btn(&self) -> &'static str {
        if self.is_zh() {
            "浏览器登录授权 (本地回调)"
        } else {
            "Authorize in Browser (Loopback)"
        }
    }

    pub fn msg_oauth_authorizing_browser(&self, provider: &str) -> String {
        if self.is_zh() {
            format!("已拉起系统浏览器进行 {provider} 登录授权，请在浏览器中完成确认...")
        } else {
            format!("Opened browser for {provider} OAuth login — please complete authorization in your browser...")
        }
    }

    pub fn msg_oauth_authorized_ok(&self, provider: &str) -> String {
        if self.is_zh() {
            format!("{provider} 浏览器回调授权成功！Refresh Token 已自动填入")
        } else {
            format!("{provider} browser authorization complete! Refresh Token populated")
        }
    }

    pub fn msg_oauth_authorize_failed(&self, provider: &str, err: &dyn std::fmt::Display) -> String {
        if self.is_zh() {
            format!("{provider} 浏览器授权失败: {err}")
        } else {
            format!("{provider} authorization failed: {err}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i18n_bilingual_strings_and_default_tab_detection() {
        let zh = I18n::new(AppLanguage::ZhCn);
        let en = I18n::new(AppLanguage::EnUs);

        assert_eq!(zh.default_tab_title(), "快速连接");
        assert_eq!(en.default_tab_title(), "Quick Connect");
        assert!(I18n::is_default_tab_title("快速连接"));
        assert!(I18n::is_default_tab_title("Quick Connect"));
        assert!(!I18n::is_default_tab_title("Prod S3"));

        assert_eq!(zh.theme_button_label(ThemePreference::System), "随系统");
        assert_eq!(en.theme_button_label(ThemePreference::System), "Auto");
        assert_eq!(zh.transfers_button_label(2, 5), "传输队列 (2)");
        assert_eq!(en.transfers_button_label(0, 5), "Transfers (5)");
    }

    #[test]
    fn test_settings_and_about_i18n_strings() {
        let zh = I18n::new(AppLanguage::ZhCn);
        let en = I18n::new(AppLanguage::EnUs);

        assert_eq!(zh.settings_button_label(), "设置");
        assert_eq!(en.settings_button_label(), "Settings");
        assert_eq!(zh.settings_tab_general(), "通用");
        assert_eq!(en.settings_tab_general(), "General");
        assert_eq!(zh.settings_tab_about(), "关于");
        assert_eq!(en.settings_tab_about(), "About");
        assert_eq!(zh.theme_option_title(ThemePreference::System), "跟随系统");
        assert_eq!(en.theme_option_title(ThemePreference::System), "System");
        assert!(!zh.about_update_placeholder().is_empty());
        assert!(!en.about_update_placeholder().is_empty());
        assert_eq!(zh.about_check_update_btn(false), "检查更新");
        assert_eq!(en.about_check_update_btn(false), "Check for Updates");
        assert_eq!(zh.about_check_update_btn(true), "检查中...");
        assert_eq!(en.about_check_update_btn(true), "Checking...");
        assert!(zh.about_update_available("0.2.0").contains("v0.2.0"));
        assert!(en.about_update_available("0.2.0").contains("v0.2.0"));
        assert_eq!(zh.titlebar_update_badge_label("0.2.0"), "新版本 v0.2.0");
        assert_eq!(en.titlebar_update_badge_label("0.2.0"), "Update v0.2.0");
        assert_eq!(
            zh.local_dir_mode_title(LocalStartupDirMode::RememberLast),
            "记住上次目录"
        );
        assert_eq!(
            en.local_dir_mode_title(LocalStartupDirMode::Custom),
            "Custom Folder"
        );
        assert_eq!(
            zh.s3_acl_option_title(S3UploadAcl::PublicRead),
            "公共读 (public-read)"
        );
        assert_eq!(
            en.s3_acl_option_title(S3UploadAcl::BucketDefault),
            "Bucket Default (No ACL)"
        );
    }

    #[test]
    fn test_native_context_menu_and_multi_selection_i18n() {
        let zh = I18n::new(AppLanguage::ZhCn);
        let en = I18n::new(AppLanguage::EnUs);

        assert_eq!(zh.ctx_upload_to_remote(), "上传到远程");
        assert_eq!(en.ctx_upload_to_remote(), "Upload to Remote");
        assert_eq!(zh.ctx_download_to_local(), "下载到本地");
        assert_eq!(en.ctx_download_to_local(), "Download to Local");
        assert_eq!(zh.ctx_rename(), "重命名 (F2)");
        assert_eq!(en.ctx_rename(), "Rename (F2)");
        assert_eq!(zh.ctx_copy_path(), "复制路径");
        assert_eq!(en.ctx_copy_path(), "Copy Path");
        assert_eq!(
            zh.pane_footer_multi_selection(3, Some("a.txt")),
            "已选 3 项 (a.txt 等)"
        );
        assert_eq!(
            en.pane_footer_multi_selection(3, Some("a.txt")),
            "3 selected (a.txt, ...)"
        );
    }
}
