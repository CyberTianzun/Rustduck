<div align="center">

<img src="assets/icons/brand_logo.png" width="108" alt="Rustduck Logo" />

# Rustduck

**让服务器、对象存储桶与个人云盘，拥有如本地文件系统般纯粹的原生体验。**

基于 **100% 纯正 Rust** 与 **GPUI** 打造的现代化硬件加速跨协议文件传输工作台。

[**English**](./README.md) · [**MIT 开源协议**](./LICENSE)

</div>

---

## 为什么做 Rustduck？

今天开发者的基础设施早已分布在各个角落：Linux 生产服务器（`SSH / SFTP`）、云厂商对象存储（`S3 / 阿里云 OSS / 腾讯云 COS / Cloudflare R2 / MinIO`）、内网共享存储（`SMB / NAS`、`FTP / FTPS`、`WebDAV`），以及个人云盘（`Google Drive`、`OneDrive`）。

然而，我们每天用来在这些存储之间搬运文件的工具，却往往困在两个极端：
- **传统老牌客户端**：诞生于二十年前的界面与交互范式，要么充斥着割裂的弹窗，要么依赖沉重的 Java 运行时。
- **套壳 Electron 应用**：仅仅为了渲染一个目录列表就要吞噬数百 MB 内存，面对成千上万个文件时滚动掉帧、拖拽迟滞。

**Rustduck** 的理念非常纯粹：**跨协议的文件传输，本该如原生文件管理器一样轻快、统一、直觉且克制。**

---

## 核心理念

### 1. 万物皆目录：统一的多协议双栏工作台
无论你是在通过 **SSH / SFTP** 排查线上日志、向 **S3** 存储桶发布带有自定义 Canned ACL 权限的构建产物、挂载局域网内的 **SMB** NAS，还是通过代理与 **Google Drive** / **OneDrive** 同步归档，所有存储后端在 Rustduck 中都遵循同一套直观的左右双栏交互语言。

### 2. GPU 现代工作台 + 操作系统真原生列表
工作台框架应当具备现代 GPU 渲染的细腻质感，而承载十万级文件的目录列表则必须拥有操作系统的原生肌肉记忆。Rustduck 独创了 **GPUI 硬件加速工作台** 与 **OS 原生虚拟化列表控件**（Windows 端采用 **Win32 `SysListView32` (`LVS_OWNERDATA`)**，macOS 端采用 **AppKit `NSTableView`**）的混合架构——兼顾丝滑滚动、系统原生文件图标、框选多选，以及左右工作区与系统文件管理器之间的直接拖拽传输。

### 3. 纯血 Rust，零外部 C/OpenSSL 负担
从 SSH2、S3 SigV4、SMB2/3 到 FTPS、WebDAV 与 OAuth 云盘，Rustduck 的所有协议引擎均采用内存安全的异步 Rust（`tokio` + `rustls`）实现。告别 OpenSSL 动态库冲突，告别 WebView 运行时，编译产物仅为一个干净、极速的原生可执行文件。

### 4. 默认安全与自由迁移
站点收藏夹的非敏感配置以清晰的 JSON 存储，支持随时导出备份与跨设备迁移；而密码、SSH 私钥口令、S3 SecretKey 与 OAuth Refresh Token 则默认交由**操作系统原生安全密钥链**（**Windows 凭据管理器** / **macOS 钥匙串**）加密托管，绝不明文落盘。

---

## 支持协议生态

| 协议类型 | 核心能力支持 |
| :--- | :--- |
| **SSH / SFTP** | 支持密码与 Ed25519 / RSA 私钥认证（自动展开 `~/.ssh/`）、流式目录浏览与递归文件夹传输 |
| **FTP / FTPS** | 支持标准明文 FTP 与显式/隐式 TLS 加密（`rustls`），支持被动模式流式上传与下载 |
| **S3 通用对象存储** | 内置 **阿里云 OSS**、**腾讯云 COS**、**AWS S3**、**MinIO** 及自定义 Endpoint 预设；支持 Virtual-Hosted 与 Path-Style 寻址；支持配置全局默认上传权限（Canned ACL / `x-amz-acl`）；按文件体积自动切换单次流式 PUT 与 8 MiB 并发分片上传 |
| **SMB (v2 / v3)** | 纯 Rust 实现的 SMB2/3 客户端，轻松连接 Windows 共享文件夹、Samba 及群晖 / 威联通 NAS |
| **WebDAV** | 支持 Nextcloud、ownCloud、群晖 WebDAV 及标准 HTTP/HTTPS `PROPFIND` / `PUT` / `MKCOL` 服务 |
| **Google Drive** | 一键拉起系统浏览器完成 OAuth 2.0 本地回环授权，内置 HTTP / HTTPS / SOCKS5 代理配置 |
| **OneDrive** | 基于 Microsoft Graph API v1.0，支持浏览器 OAuth 2.0 PKCE 授权与代理配置 |

---

## 快速构建与运行

### 环境要求
- **Rust** `1.85+`（`edition = "2021"`）
- **Windows 10 / 11** 或 **macOS 12+**

### 编译与启动

```bash
# 开发模式启动
cargo run

# 编译 Release 高性能版本
cargo build --release
```

---

## 开源协议

本项目基于 [MIT License](./LICENSE) 开源发布。
