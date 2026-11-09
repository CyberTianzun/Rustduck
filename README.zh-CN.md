<div align="center">
  <img src="assets/icons/windows/rustduck_256.png" width="128" height="128" alt="Rustduck Logo" />
  <h1>Rustduck</h1>
  <p><strong>纯 Rust 编写、GPU 硬件加速的跨协议云存储与远程文件传输客户端</strong></p>
  <p>致敬 <em>Cyberduck</em> 与 <em>Panic Transmit 5</em> — 基于 Zed <code>gpui</code> + <code>gpui-component</code> 构建</p>
  <p>
    <a href="README.md">English</a> · <strong>简体中文</strong>
  </p>
</div>

---

## 项目简介

**Rustduck** 是一款使用 **100% 纯 Rust** 从零重写的高性能远程文件与云对象存储桌面客户端。它在保留 Cyberduck 经典小黄鸭神韵的基础上打造了专属的**氧化铁锈质感（Rusty Duck）**品牌图标，并全面对标 **Panic Transmit 5** 的双态工作区布局与交互细节。

借助 Zed 编辑器的 GPU 硬件加速界面框架（`gpui` + `gpui-component`）与 `tokio` 异步运行时，Rustduck 实现了零卡顿的双栏文件浏览与高并发流式传输，原生支持 **SSH / SFTP**、**S3 通用对象存储（阿里云 OSS / 腾讯云 COS / MinIO / AWS S3）** 以及 **SMB2 / SMB3 网络共享**，完全不依赖任何 C/C++ 协议动态库或 OpenSSL。

---

## 核心特性

- **100% 纯 Rust 多协议存储引擎**
  - **SSH / SFTP（`russh` + `russh-sftp`）**：基于纯 Rust SSH2/SFTP v3 实现，支持密码认证与私钥文件认证（如 `~/.ssh/id_ed25519`、`~/.ssh/id_rsa`，支持 `~/` 主目录自动展开）。
  - **S3 通用对象存储（`reqwest` + `rustls` + AWS SigV4）**：内置 **阿里云 OSS**、**腾讯云 COS**、**MinIO / 私有云**、**AWS S3** 及 **自定义 S3 Endpoint** 厂商模板；同时支持 `Virtual-Hosted` 与 `Path-Style` 寻址模式、根目录全桶列举（`ListAllMyBuckets`）、受限权限单桶直连、目录递归删除与 `UNSIGNED-PAYLOAD` 流式上传。
  - **SMB2 / SMB3 网络共享（`smb`）**：纯 Rust SMB 协议栈，支持 UNC 路径规范化、自定义端口及可选的工作组 / 域（`Domain\Username`）认证。
- **Panic Transmit 5 标准双态工作区**
  - **自定义无边框标题栏与多标签页会话**：采用 `gpui_component::TitleBar` 承载文件夹式多会话标签栏（`TabBar`）、连接状态指示灯与快捷偏好设置，同时自动同步 Windows 任务栏与 `Alt+Tab` 窗口名称。
  - **新建连接视图（左侧收藏夹画廊 + 右侧快速连接检查器）**：左侧支持按名称/IP/Bucket 实时过滤服务器卡片（单击载入配置，双击立即连接）；右侧提供协议分段选择器、参数表单与一键连接握手测试。
  - **已连接双栏文件浏览器**：支持拖拽调节宽度的 **本地文件系统 + 远程服务器** 对称双栏视图，具备可点击路径面包屑、手动输入路径跳转栏、行内新建文件夹/重命名/删除二次确认条、隐藏文件开关、多列排序（名称 / 修改时间 / 大小）与实时过滤。
  - **底部可折叠传输活动抽屉**：基于 `tokio::io::duplex` 内存管道与信号量并发控制的流式传输队列，支持文件夹递归上传/下载、实时速率与进度条展示，以及流式传输途中的 **暂停 / 继续 / 取消**。
- **系统密钥链安全防护**
  - 收藏夹配置文件 `bookmarks.json` **绝不明文落盘任何密码或私钥口令**；所有敏感凭据（密码、私钥口令、S3 `SecretAccessKey`）均通过 `keyring` 加密存入操作系统原生凭据保险箱（Windows 凭据管理器 / macOS 钥匙串 / Linux Secret Service）。
- **中英双语国际化（`简体中文` / `English`）**
  - 初次启动自动检测操作系统语言（`zh-CN` 或 `en-US`）并持久化至 `preferences.json`；支持在设置弹窗中实时切换中英文界面。
- **多主题外观（`随系统` / `浅色` / `深色`）与关于页**
  - 初次启动默认跟随系统外观（`System`），实时监听系统深浅色切换，并支持在设置弹窗中切换 **随系统 / 浅色工作室 / 深色碳黑** 主题及查看应用版本与关于信息。
- **符合各平台规范的生锈版图标（Rusty Duck）**
  - **Windows**：采用**透明背景**多尺寸图标（[rustduck.ico](assets/icons/windows/rustduck.ico)），通过 `build.rs` 直接编译嵌入 `.exe`（Win32 Resource ID `1`），文件资源管理器与任务栏原生显示透明生锈小黄鸭。
  - **macOS / iOS / Android**：保留**白色圆角矩形（Squircle）背景**（[rustduck.icns](assets/icons/macos/rustduck.icns)、[AppIcon_1024.png](assets/icons/ios/AppIcon_1024.png)、[ic_launcher_512.png](assets/icons/android/ic_launcher_512.png)）。

---

## 目录结构

```text
Rustduck/
├── assets/icons/               # 跨平台生锈版小黄鸭图标资源
│   ├── brand_logo.png          # 标题栏左上角透明背景品牌图标
│   ├── windows/                # Windows 透明背景 .ico 与高清 PNG
│   ├── macos/                  # macOS 白色圆角矩形背景 .icns 与 1024px PNG
│   ├── ios/                    # iOS AppIcon_1024.png
│   └── android/                # Android ic_launcher_512.png
├── build.rs                    # 纯 Rust Win32 图标资源编译与栈大小配置
├── src/
│   ├── main.rs                 # GPUI 应用启动入口
│   ├── lib.rs                  # 库入口
│   ├── i18n.rs                 # 中英双语国际化文案字典
│   ├── models/                 # 协议配置模型、Bookmark、FileEntry、TransferTask
│   ├── services/
│   │   ├── bookmark_store.rs   # 收藏夹 JSON 持久化 + 系统密钥链安全存储
│   │   ├── preferences.rs      # 语言与主题偏好持久化 + 系统区域自动检测
│   │   ├── runtime.rs          # 全局多线程 Tokio 异步运行时
│   │   └── transfer_manager.rs # 并发流式传输队列与暂停/继续/取消控制
│   ├── storage/
│   │   ├── mod.rs              # StorageBackend 统一异步抽象接口与工厂函数
│   │   ├── local.rs            # 本地文件系统驱动（含 Windows 盘符根目录聚合）
│   │   ├── sftp.rs             # 纯 Rust SSH/SFTP 存储后端
│   │   ├── s3.rs               # 通用 S3 / 阿里云 OSS / 腾讯云 COS / MinIO 后端
│   │   └── smb.rs              # 纯 Rust SMB2/3 网络共享后端
│   └── ui/
│       ├── mod.rs              # 深浅色主题调色板、文件类型语义图标与内置资源
│       ├── workspace.rs        # 自定义标题栏、多标签页管理与工作区状态
│       ├── new_tab_view.rs     # 收藏夹服务器画廊 + 快速连接检查器视图
│       ├── dual_pane_view.rs   # 可调宽本地/远程双栏文件浏览器视图
│       ├── file_pane.rs        # 排序、过滤与路径面包屑切分辅助函数
│       ├── settings_modal.rs   # 设置（外观主题 / 界面语言）与关于弹窗
│       └── transfer_drawer.rs  # 底部可折叠传输活动队列抽屉
```

---

## 本地构建与运行

### 环境要求

- **Rust 工具链**：Rust `1.85+`（Edition 2021）
- **Windows 环境**：MSVC 生成工具（`x86_64-pc-windows-msvc`）与 Windows 10/11 SDK（支持 GPUI 的 DirectX 硬件加速渲染）

### 编译与测试命令

```bash
# 启动开发模式运行
cargo run

# 本地构建 Debug 版本
cargo build

# 本地构建 Release 优化版本
cargo build --release

# 运行全部单元测试
cargo test --lib
```

---

## 配置文件与安全存储路径

Rustduck 将非敏感配置保存在系统标准配置目录下（Windows 为 `%APPDATA%\rustduck\`，macOS 为 `~/Library/Application Support/rustduck/`，Linux 为 `~/.config/rustduck/`）：

- **`bookmarks.json`**：收藏夹服务器列表（仅保存地址、端口、用户名、Bucket 等非敏感字段，**绝不包含密码或私钥口令**）。
- **`preferences.json`**：界面语言（`"zh-CN"` / `"en-US"`）与主题偏好（`"system"` / `"light"` / `"dark"`）。
- **操作系统安全密钥链（`service = "rustduck"`）**：以收藏夹 UUID 为键加密存储密码、私钥口令及 S3 `SecretAccessKey`。
