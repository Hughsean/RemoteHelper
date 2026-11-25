# Changelog

All notable changes to RemoteHelper will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- 后续 GitHub Releases 不再构建或上传 Intel Mac 安装包；macOS 下载仅提供 Apple Silicon 版。Windows x64 客户端和服务端发布不变。

## [3.0.0-rc.1] - 2026-11-26

### Added

- Windows 服务端可在后台持续记录 CPU、内存、网络及可用的温度、功率等指标。历史按 UTC 日期保存，支持写入中断后恢复，并按天数或容量清理。
- 桌面端支持分页同步历史、按服务端身份隔离本地缓存，以及登录前离线回看。趋势图会区分采样缺口、时钟回拨、未同步区间和已被服务端清理的历史。
- 新增历史设置页，可查看存储状态和调整本地缓存；配置过权限的管理员还可调整服务端历史设置并确认时钟异常。

### 升级说明

1. **远程历史需同时升级客户端和 Windows 服务端。** 备份现有 `config.toml`、服务端身份文件和历史目录（如已存在）。旧服务端仍可用于实时监控，但不会提供远程历史；已固定指纹的地址不会自动降级到旧协议。
2. 若尚未初始化服务端身份，在部署目录运行 `RemoteHelperServer.exe --init-server-identity`，随后正常启动服务端。通过可信渠道核对命令输出的 SHA-256 指纹，再在客户端确认并连接。若需远程修改历史设置，将客户端公钥同时加入 `[web_panel].authorized_keys` 和 `[history].admin_keys`，再重启服务端。
3. 远程历史和实时请求分别使用 TCP 连接；如果并发上限被设为 `1`，应提高上限后再使用历史同步。服务端历史文件默认写入配置目录下的 `history/`，不加密，请限制访问权限。

### 下载

| 平台 | 文件 |
| --- | --- |
| Windows x64 桌面端 | `RemoteHelper-x86_64-pc-windows-msvc.exe` |
| Windows x64 服务端 | `RemoteHelperServer-x86_64-pc-windows-msvc.zip`（含服务端、`keygen`、配置示例和许可证） |
| macOS 15+ Intel | `RemoteHelper-x86_64-apple-darwin.dmg` |
| macOS 15+ Apple Silicon | `RemoteHelper-aarch64-apple-darwin.dmg` |

macOS 安装包使用临时签名（ad hoc），未做 Developer ID 签名或公证。

### Fixed

- macOS 客户端关闭窗口后，点击 Dock 图标会重新打开窗口，并回到关闭前的界面：已登录时恢复原来的页面，停在登录页时仍显示登录页。

### Changed

- Rust 工作区成员统一移入 `crates/`，共享依赖集中声明；桌面端与服务端包名保持不变。桌面端按入口、装配、外壳和页面分层。
- 桌面客户端随系统深浅模式切换配色。窗口使用原生模糊材质，卡片采用分层半透明背景。
- 登录页使用一体化表单，切换页面时使用短距离滑入动效。客户端消息在独立通知层显示，新增服务在悬浮弹窗中操作。
- 顶部导航使用三色圆点显示连接状态。监控页在底栏提供刷新间隔选择。
- 趋势图移除时间范围下拉菜单，以指针时刻为锚点滚轮缩放，支持 Shift + 滚轮平移和双击重置。底部滑块显示历史概览，可拖动选框和手柄调整时间范围，或点右箭头回到实时。悬停时显示该时刻各条阶梯线的读数。
- 趋势图在较宽时间范围按图宽合并密集采样，悬停读数标注平均值的时间周期。

## [2.3.1] - 2026-11-25

### Fixed

- Windows 客户端将应用图标嵌入可执行文件，修复标题栏、任务栏和文件资源管理器中的图标缺失。
- macOS 客户端在应用包中生成并嵌入 `icns` 图标，修复 Dock 和访达中的图标缺失。

### Changed

- 清理桌面客户端遗留的 Dioxus 构建配置。

## [2.3.0] - 2026-11-25

### Changed

- 桌面客户端改用 GPUI Kit，使用原生窗口、文件选择器和深色界面。
- 监控页使用自适应趋势图，按真实采样时间平滑滚动。刷新间隔和时间范围通过下拉菜单选择。
- 标签发布流程构建 Windows x64 客户端和 macOS Intel、Apple Silicon 安装映像。
- 更新用户指南、客户端开发指引和配置示例。

### Removed

- 移除旧客户端代码、样式、构建配置和开发教程。

## [2.2.0] - 2026-01-03

### 新增

- **用户模式支持**: 完整的凭据管理系统
  - 新增凭据管理模态框组件 (`credential_modal.rs`)
  - 支持多用户凭据的安全存储和管理
  - 凭据加密存储机制
- **UI/UX 全面升级**:
  - 新增登录视图 (`views/login.rs`)
  - 新增仪表板视图 (`views/home.rs`)
  - 重构应用状态管理系统 (`state/auth.rs`, `state/services.rs`, `state/system.rs`)
  - 新增顶部导航栏组件 (`components/header.rs`)
  - 新增服务添加模态框 (`components/add_service_modal.rs`)
  - 新增服务列表组件 (`components/service_list.rs`)
  - 新增系统状态组件 (`components/system_status.rs`)
- **样式系统重构**:
  - 模块化 CSS 文件结构 (`header.css`, `login.css`, `modal.css`, `services.css`, `status.css`)
  - 优化滚动条样式和行为
  - 改进状态卡片和进度条视觉效果
- **macOS 原生支持增强**:
  - 添加自定义中文菜单
  - 使用 Tauri v2 新的菜单结构和预定义菜单项
  - 更新包名称为 `RemoteHelperApp`

### 改进

- 重构路由系统，支持登录/仪表板视图切换
- 采用现代化的状态管理模式（Arc + RwLock）
- 优化窗口大小配置和标题样式
- 移除未使用的文件和引用
- 更新默认服务器地址为 `frp-try.com:53460`

### 技术细节

- 路由系统：支持 `/login` 和 `/home` 视图
- 状态管理：分离认证、服务和系统状态
- 组件化：模块化的 UI 组件设计
- 样式：CSS 模块化，提升可维护性

## [2.0.1] - 2026-01-01

### Fixed

- **Chart Rendering**: Fixed trend chart X-axis to use actual timestamps instead of data point indices
  - Chart now displays data based on real time intervals, not refresh rate
  - Changing refresh interval (0.1s → 5s) no longer affects visual scrolling speed
  - Time windows (1min/5min/30min) now accurately represent wall-clock time
- **macOS Fullscreen**: Tauri app now properly exits fullscreen mode when showing window from tray
  - Prevents window being stuck in fullscreen without title bar controls
  - Ensures window appears in normal mode with proper size and position
  - Applies to both menu item click and tray icon click events

### Improved

- Chart behavior now matches Windows Task Manager (time-based scrolling)
- More consistent user experience across different refresh rate settings

## [2.1.0] - 2026-01-01

### Changed

- **BREAKING**: Removed `nanoid` dependency and related fields from `SystemInfo` and `ServiceInfo` structures
- **BREAKING**: `SystemInfo` now uses `timestamp: u64` (Unix milliseconds) instead of `nanoid` for unique identification
- `SystemInfo` equality comparison now based on `timestamp` instead of `nanoid`
- `ServiceInfo` equality comparison now based on `id` and `pid` combination

### Added

- `SystemInfo.timestamp` field: Unix timestamp in milliseconds for accurate data collection timing
- Web client now uses time-based data retention (30-minute rolling window) instead of fixed-length buffer

### Improved

- **Performance**: Web client optimized with `VecDeque` instead of `Vec` for O(1) time-series data operations
- Historical data management now based on actual time intervals rather than arbitrary point counts
- Server generates timestamps using `SystemTime::now()` for consistent time tracking

### Removed

- `nanoid` crate dependency from workspace, server, and common library
- `common::func::nanoid_gen()` function (no longer needed)
- Client-side nanoid generation logic from Tauri commands

### Migration Guide

**For Server Operators:**

- No action required - changes are backward compatible in protocol
- Server will automatically generate timestamps for all status responses

**For Custom Client Developers:**

- Update to latest `common` library version
- Remove any code that references `SystemInfo.nanoid` or `ServiceInfo.nanoid`
- Use `SystemInfo.timestamp` for time-based operations
- Ensure proper handling of `timestamp` field (u64, milliseconds since Unix epoch)

## [2.0.2] - 2026-01-01

### Fixed

- **macOS**: Tray icon click and menu "Show/Hide" now automatically reset window size (880x600) and center position
- Window geometry is now restored to default settings when showing from hidden state on macOS

### Improved

- Better window management on macOS: prevents off-screen windows and inconsistent sizing
- Enhanced user experience with predictable window positioning

## [2.0.1] - 2026-01-01

### Changed

- **BREAKING**: Client private keys are now automatically loaded from `~/id_ed25519.json` instead of being hardcoded
- `keygen` tool now saves keys directly to `~/id_ed25519.json` in user home directory
- Login UI updated to clarify password is for the key file
- All client implementations (Tauri, Web, test tools) now use standardized key file location

### Improved

- Header UI redesign: "Powered by Hughsean" now displays as subtitle below main title
- Enhanced visual hierarchy in application header

### Migration Guide

If you have an existing `client_key.json`, migrate it to the new location:

**Windows:**

```powershell
Move-Item client_key.json $env:USERPROFILE\id_ed25519.json
```

**Linux/macOS:**

```bash
mv client_key.json ~/id_ed25519.json
```

Or generate a new keypair:

```bash
cargo run --bin keygen
```

## [2.0.0] - 2026-01-01

### Added

- 🎉 Initial public release of RemoteHelper v2.0
- End-to-end encrypted communication (X25519 + AES-256-GCM)
- Ed25519 public key authentication
- Real-time hardware monitoring (CPU, RAM, GPU, Network)
- Process management for external programs (frpc)
- Dual client support (Web + Desktop Tauri app)
- Automatic log rotation (10MB threshold)
- Configurable connection limits and timeouts
- Auto-pause monitoring when idle (10s)
- Graceful shutdown with 5s process termination timeout
- Dynamic service registration
- Challenge-response authentication with 30s expiry
- Nonce randomization to prevent replay attacks

### Technical Details

- Rust 2024 Edition
- Tokio async runtime (4 worker threads)
- Arc<RwLock<T>> for shared state
- AtomicUsize for lock-free counters
- Dioxus v0.7 for Web UI
- Tauri v2 for Desktop app
- Chart.js integration for data visualization
- WASM bindings for browser client

### Security

- X25519 Diffie-Hellman key exchange
- AES-256-GCM symmetric encryption
- Ed25519 digital signatures
- PBKDF2 key derivation for encrypted key storage
- 30-second challenge expiry
- Per-message unique nonces
- Connection limit DoS protection

### Documentation

- Comprehensive README.md
- AI-friendly copilot-instructions.md
- MIT License
- Configuration examples in config.toml

### Code Quality

- Passes `cargo clippy --workspace -- -D warnings`
- Formatted with `cargo fmt --all`
- All tests passing
- Zero compilation warnings

---

## Future Enhancements

The following features are planned for future releases:

- Persistent dynamic service storage
- TLS certificate support
- Per-client rate limiting
- Prometheus metrics endpoint
- Multi-GPU monitoring
- Cross-platform support (Linux/macOS)
- WebSocket transport option
- Service dependency management
- Scheduled task support (cron-like)

---

[Unreleased]: https://github.com/Hughsean/RemoteHelper/compare/v3.0.0-rc.1...HEAD
[3.0.0-rc.1]: https://github.com/Hughsean/RemoteHelper/compare/v2.3.1...v3.0.0-rc.1
[2.3.1]: https://github.com/Hughsean/RemoteHelper/releases/tag/v2.3.1
[2.3.0]: https://github.com/Hughsean/RemoteHelper/releases/tag/v2.3.0
[2.0.2]: https://github.com/Hughsean/RemoteHelper/releases/tag/v2.0.2
[2.0.1]: https://github.com/Hughsean/RemoteHelper/releases/tag/v2.0.1
[2.0.0]: https://github.com/Hughsean/RemoteHelper/releases/tag/v2.0.0
