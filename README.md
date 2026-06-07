# RemoteHelper

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-orange.svg)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-v2-blue.svg)](https://tauri.app/)
[![Dioxus](https://img.shields.io/badge/Dioxus-v0.7-green.svg)](https://dioxuslabs.com/)
[![License](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

一个基于 Rust 的远程系统监控与进程管理工具，支持 Windows 和 macOS。提供 Web 控制面板和桌面客户端。

## ✨ 特性

- 🔒 **端到端加密通信**：基于 X25519 + AES-256-GCM 的安全通信协议
- 🔐 **Ed25519 身份认证**：使用公钥加密技术的强身份验证
- 📊 **实时硬件监控**：CPU、内存、GPU（NVIDIA）、网络流量监控
- 🚀 **进程管理**：启动、停止、重启外部程序（如 frpc 隧道）
- 🌐 **双客户端支持**：
  - **Web 客户端**：基于 WebAssembly 的浏览器应用
  - **桌面客户端**：Tauri v2 系统托盘应用
- 📝 **日志管理**：服务/隧道日志在超过 10MB 时轮转；主服务器日志 `logs/server.log` 默认不自动轮转
- 🧪 **Mock 调试模式**：无需连接服务器即可在本地模拟硬件数据，加速前端开发
- ⚡ **高性能**：异步 Tokio 运行时，支持并发连接

## 🏗️ 架构

项目采用 Cargo Workspace 单体仓库架构：

```
RemoteHelper/
├── src/                    # 服务器主程序
│   ├── main.rs            # 入口点
│   ├── server.rs          # TCP 连接处理
│   ├── state.rs           # 全局状态管理
│   ├── process.rs         # 进程管理
│   └── config.rs          # 配置解析
├── common/                # 共享库（协议定义）
│   └── src/
│       ├── lib.rs         # Request/Response 枚举
│       ├── crypto.rs      # 加密会话
│       └── func.rs        # 工具函数
├── client/                # 客户端库
│   └── src/lib.rs         # TCP 客户端实现（含 Mock 模式）
├── app/                   # Dioxus 前端应用 (Web + 桌面)
│   ├── src/
│   │   ├── main.rs        # 路由与入口
│   │   ├── views/         # 页面：登录、首页、服务管理
│   │   └── widgets/       # 自定义组件：图表、卡片、对话框
│   └── assets/            # CSS 样式资源
├── PawnIO/                # PawnIO 子模块
└── hwlib/                 # 硬件抽象层（未使用）
```

## 🚀 快速开始

### 前置要求

- **Rust** 1.83+ (2024 Edition)
- **Node.js** 18+ (可选，用于前端开发)
- **Dioxus CLI** (可选): `cargo install dioxus-cli`
- **NVIDIA 驱动** (可选，用于 GPU 监控)

### 1. 生成密钥对

首先为客户端认证生成 Ed25519 密钥对：

```bash
cargo run --bin keygen
```

这将在用户目录生成 `~/id_ed25519.json` 密钥文件：

- **公钥**（Base64 编码）：添加到服务器配置的 `authorized_keys`
- **私钥文件**（加密存储在 `~/id_ed25519.json`）：客户端自动读取

### 2. 配置服务器

编辑 `config.toml`：

```toml
[web_panel]
enabled = true
local_port = 9999
frpc_exe_path = "frpc.exe"
frpc_args = ["-f", "your_token"]
authorized_keys = ["YOUR_PUBLIC_KEY_FROM_KEYGEN"]
startup_delay_secs = 0
max_connections = 100
connection_timeout_secs = 300

[[service]]
description = "Remote Desktop Access"
exe_path = "frpc.exe"
args = ["-f", "token:port"]
# 服务启动模式："none" | "oneshot" | "continuous"
# - "none"      : 不自动启动
# - "oneshot"   : 启动后会立即结束（不保留 PID）
# - "continuous": 启动后会持续运行（保留 PID 以便后续销毁）
auto_start = "none"
allow_web_control = true
```

### 3. 启动服务器

#### 开发模式

```bash
# 开发模式
cargo run

# 生产模式
cargo build --release
./target/release/RemoteHelperServer.exe
```

#### 生产部署

```bash
# 编译 Release 版本
cargo build --release

# 将编译产物和 config.toml 部署到目标目录
cp target/release/RemoteHelperServer.exe /path/to/deploy/
cp config.toml /path/to/deploy/

# 如需注册为系统服务，可使用 NSSM 等工具
# nssm install RemoteHelperServer /path/to/deploy/RemoteHelperServer.exe
```

### 4. 运行客户端

#### Web 客户端（开发模式）

```bash
cd app
dx serve
# 访问 http://localhost:8080
```

#### 桌面客户端（开发模式）

```bash
cd app
cargo tauri dev
```

#### 构建发布版本

```bash
# Web 客户端
cd app
dx build --release

# 桌面客户端
cd app
cargo tauri build
```

## 📖 使用说明

### 服务器命令

```bash
# 运行服务器
cargo run --bin RemoteHelperServer

# 生成密钥对
cargo run --bin keygen
```

### 客户端连接

1. 启动服务器（默认监听端口 9999）
2. 打开 Web 客户端或桌面客户端
3. 输入服务器地址（如 `localhost:9999`）
4. 输入 `~/id_ed25519.json` 文件的密码短语进行认证
5. 查看系统状态和管理服务

> **注意**：客户端会自动从用户目录读取 `~/id_ed25519.json` 密钥文件

## 🔧 配置详解

### 服务器配置 (`config.toml`)

| 字段 | 说明 | 默认值 |
|------|------|--------|
| `enabled` | 是否启用 Web 面板 | `true` |
| `local_port` | TCP 监听端口 | `9999` |
| `frpc_exe_path` | frpc 可执行文件路径 | `"frpc.exe"` |
| `authorized_keys` | 授权的公钥列表（Base64） | `[]` |
| `startup_delay_secs` | 启动延迟（秒） | `0` |
| `max_connections` | 最大并发连接数 | `100` |
| `connection_timeout_secs` | 连接超时时间（秒） | `300` |

### 服务配置

```toml
[[service]]
description = "服务描述"
exe_path = "可执行文件路径"
args = ["参数1", "参数2"]
auto_start = "none"           # 服务启动模式："none" | "oneshot" | "continuous"
allow_web_control = true     # 允许 Web 控制
```

## 🔒 安全特性

### 加密通信

1. **握手阶段**：
   - 客户端生成临时 X25519 密钥对
   - 服务器生成临时 X25519 密钥对
   - 双方交换公钥并计算共享密钥

2. **加密层**：
   - 算法：AES-256-GCM
   - 密钥派生：SHA256(X25519 共享密钥)
   - 每条消息使用唯一 nonce（避免重放攻击）

3. **消息格式**：

   ```
   [4 字节长度][加密的 JSON 载荷][16 字节 GCM 标签]
   ```

### 身份认证

1. 服务器发送随机挑战（30 秒有效期）
2. 客户端使用 Ed25519 私钥签名挑战
3. 服务器验证签名和公钥授权
4. 认证成功后建立会话

### 密钥管理

- 私钥使用 PBKDF2 + AES-256-GCM 加密存储
- 公钥使用 Base64 编码配置
- 挑战 30 秒后自动失效

## 📊 监控指标

### 系统信息

- **CPU**：使用率（%）、型号
- **内存**：已用/总量（GB）、使用率（%）
- **GPU**（NVIDIA）：使用率（%）、显存使用量、型号
- **网络**：接收/发送字节数、实时速率
- **运行时间**：系统正常运行时间

### 进程管理

- 服务状态（运行中/已停止）
- 进程 PID
- 启动/停止/重启操作
- 日志查看（服务/隧道日志实现简单轮转）

## 🛠️ 开发

### 代码质量

```bash
# 格式化代码
cargo fmt --all

# 代码检查
cargo clippy --workspace -- -D warnings

# 运行测试
cargo test --workspace

# 编译检查
cargo check --workspace
```

### 项目约定

- **Rust Edition**: 2024
- **错误处理**：服务器使用 `anyhow::Result`，客户端使用 `Result<T, String>`
- **日志**：服务器使用 `tracing`，客户端使用 Tauri/Console 日志
- **并发安全**：`Arc<RwLock<T>>` + `AtomicUsize`
- **异步运行时**：Tokio multi_thread（4 线程）

### 修改协议

修改 `common/src/lib.rs` 中的 `Request`/`Response` 后，需要同步更新：

1. `src/server.rs` → `process_authenticated_request()`
2. `client/src/lib.rs` → `send_request()` 调用
3. `app/src/views/` → 页面中对应的请求处理

## 🐛 故障排除

### 服务器无法启动

- ✅ 检查 `config.toml` 语法是否正确
- ✅ 确认端口 9999 未被占用
- ✅ 验证 `frpc_exe_path` 文件存在

### 认证失败

- ✅ 确认 `~/id_ed25519.json` 文件存在且格式正确
- ✅ 公钥与 `keygen` 生成的匹配
- ✅ 密码短语正确
- ✅ 挑战未过期（30 秒内）
- ✅ 密钥文件权限正确

### GPU 监控显示 None

- ✅ 安装 NVIDIA 驱动
- ✅ 仅支持 NVIDIA GPU
- ✅ 查看服务器日志了解 NVML 初始化状态

### 连接数限制

- ✅ 增加配置中的 `max_connections`
- ✅ 检查客户端是否正确关闭连接
- ✅ 考虑实现连接池

## 📝 日志

### 服务器日志

- **位置**：`logs/server.log`
- **轮转**：**不自动轮转**（由 tracing_appender 写入）。如需轮转，请使用系统日志轮转工具或部署脚本（例如通过 NSSM 或外部轮转工具）。
- **输出**：同时输出到 stdout 和文件

### 服务日志

- **静态服务**：`logs/service_{id}.log`
- **Web 隧道**：`logs/web_tunnel.log`
- **轮转**：超过 10MB 自动轮转（服务与 Web 隧道日志由程序实现简单轮转）

## 🔮 未来计划

- [ ] 动态服务持久化存储
- [ ] TLS 证书支持
- [ ] 每客户端速率限制
- [ ] Prometheus 指标端点
- [ ] 多 GPU 支持
- [ ] Linux 平台支持

## 📄 许可证

本项目采用 MIT 许可证 - 详见 [LICENSE](LICENSE) 文件

## 🙏 致谢

- [Tauri](https://tauri.app/) - 跨平台桌面应用框架
- [Dioxus](https://dioxuslabs.com/) - Rust 响应式 UI 框架
- [Tokio](https://tokio.rs/) - 异步运行时
- [sysinfo](https://github.com/GuillaumeGomez/sysinfo) - 系统信息库
- [nvml-wrapper](https://github.com/Cldfire/nvml-wrapper) - NVIDIA GPU 监控

## 📧 联系方式

- **作者**: Hughsean
- **邮箱**: <Hughsean.F@outlook.com>
- **项目**: [GitHub Repository](#)

---

**注意**：本项目当前支持 Windows 和 macOS，未来可能扩展更多平台支持。
