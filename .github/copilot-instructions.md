# RemoteHelper - AI Coding Assistant Instructions

## Project Overview

RemoteHelper 是一个基于 Rust 的远程系统监控与进程管理工具，专为 Windows 设计。采用**客户端-服务器架构**，使用自定义 TCP 加密协议通信。

### 核心架构

```
┌─────────────────┐                    ┌──────────────────┐
│  Web/Desktop    │ ←──encrypted───→  │  Server (此机)   │
│  Client (A/)    │   TCP (X25519+AES) │  (src/)          │
└─────────────────┘                    └──────────────────┘
        ↓ 使用                                 ↓ 使用
┌─────────────────┐                    ┌──────────────────┐
│   client/       │                    │   common/        │
│  (TCP客户端库)   │                    │ (协议+加密共享库) │
└─────────────────┘                    └──────────────────┘
```

**项目组成 (Cargo Workspace)**:
- **`src/`** - 服务器主程序 (`RemoteHelperServer` binary)，监听 TCP 连接，管理系统监控和进程
- **`A/`** - Dioxus 0.7 全栈应用，支持 Web (WASM) 和桌面 (Tauri v2) 双客户端
- **`common/`** - 共享库，定义 `Request/Response` 枚举和 `CryptoSession` 加密会话
- **`client/`** - TCP 客户端库，封装连接逻辑和 Ed25519 认证，被 `A/` 使用
- **`hwlib/`** - 硬件抽象层（当前未激活使用）

## 关键技术约定

### 加密与认证协议

**强制使用的安全机制** (见 [common/src/crypto.rs](common/src/crypto.rs)):
1. **握手阶段**: X25519 ECDH 密钥交换 (每次连接生成临时密钥对)
2. **会话加密**: AES-256-GCM，密钥从 ECDH 共享密钥派生 (`SHA256(shared_secret)`)
3. **身份认证**: Ed25519 数字签名 (客户端用私钥签名随机挑战，服务器验证公钥)
4. **Nonce 管理**: 64位计数器 + MSB 方向位 (服务器/客户端分离)

**重要**:
- 服务器在 `config.toml` 的 `authorized_keys` 字段维护白名单公钥
- 客户端密钥存储在 `~/id_ed25519.json` (AES-256加密的 PBKDF2-HMAC-SHA256 保护)
- 生成密钥: `cargo run --bin keygen`

### Dioxus 0.7 前端开发规范

**⚠️ 重要：所有 Dioxus 代码开发必须严格参考 `.doc/` 目录下的官方文档**

撰写 Dioxus 代码时，务必参考以下文档（位于项目根目录的 `.doc/` 文件夹）：
- **[.doc/dioxus-0.7-essentials.md](.doc/dioxus-0.7-essentials.md)** - 核心概念和基础知识
- **[.doc/dioxus-0.7-tutorial.md](.doc/dioxus-0.7-tutorial.md)** - 官方教程和示例
- **[.doc/dioxus-0.7-guides.md](.doc/dioxus-0.7-guides.md)** - 实践指南和最佳实践
- **[.doc/dioxus-0.7-reference.md](.doc/dioxus-0.7-reference.md)** - API 参考手册
- **[.doc/knowledge-base/](.doc/knowledge-base/)** - 知识库（动态样式等专题）

同时遵循 [A/AGENTS.md](A/AGENTS.md) 中的项目特定约定。

**关键差异 (Dioxus 0.7 vs 旧版本)**:
- ❌ **已移除**: `cx`, `Scope`, `use_state`, `use_ref`
- ✅ **现在使用**: `use_signal`, `use_memo`, `Signal<T>`, `ReadOnlySignal<T>`
- 组件必须标注 `#[component]`，参数为函数参数（不再使用 `Props` struct）
- RSX 中使用 `for` 循环优于 `.map()` 迭代器
- 事件处理器直接使用闭包，不需要 `move |_|` 前缀（除非需要捕获外部变量）

**样式与资源**:
- 使用 `asset!("/assets/path")` 宏引用静态资源（相对项目根目录）
- CSS 通过 `document::Stylesheet { href: asset!(...) }` 注入
- 主 CSS 位于 [A/assets/styling/](A/assets/styling/)，组件级 CSS 在各 `component/style.css`

**状态管理模式** (见 [A/src/main.rs](A/src/main.rs)):
- 全局状态用 `use_context_provider` / `use_context` 传递
- 异步操作用 `use_resource`（自动重新运行依赖的 signal）
- 与服务器通信使用 `client` 库的 `CONNECTION` 全局状态

### 配置文件结构

[config.toml](config.toml) 控制所有服务器行为:
```toml
[web_panel]
enabled = true
local_port = 9999                  # 监听端口
frpc_exe_path = "frpc.exe"         # Web 面板内网穿透程序
frpc_args = ["-f", "token"]        # frpc 参数
authorized_keys = ["base64_pubkey"] # Ed25519 公钥白名单
startup_delay_secs = 0             # 启动前等待网络
max_connections = 100
connection_timeout_secs = 300
health_check_url = null            # 可选的网络检测 URL

[[service]]                        # 可管理的外部服务
description = "描述"
exe_path = "path/to/exe"
args = ["arg1", "arg2"]
auto_start = false                 # 服务器启动时自动运行
allow_web_control = true           # 允许客户端控制
```

## 开发工作流

### 常用命令

| 命令 | 用途 | 目录 |
|------|------|------|
| `cargo run` | 启动服务器 (开发模式) | 根目录 |
| `cargo run --bin keygen` | 生成 Ed25519 密钥对 | 根目录 |
| `dx serve` | 启动 Dioxus Web 客户端 | `A/` |
| `dx serve --platform desktop` | 桌面模式 (Tauri) | `A/` |
| `cargo build --release` | 构建生产版本 | 根目录 |

**首次设置**:
1. 安装 `dioxus-cli`: `cargo install dioxus-cli`
2. 生成密钥: `cargo run --bin keygen`
3. 复制公钥到 `config.toml` 的 `authorized_keys`
4. 启动服务器: `cargo run`
5. 新终端启动客户端: `cd A && dx serve`

### 服务器核心机制

**并发模型** ([src/main.rs](src/main.rs)):
- Tokio 异步运行时 (4 线程 `multi_thread`)
- 后台监控任务每 N 毫秒刷新系统指标 (可动态调整)
- **智能暂停**: 10秒无客户端读取时自动暂停刷新
- 连接限制: `max_connections` 原子计数器控制

**连接处理** ([src/server.rs](src/server.rs)):
- 每个连接独立 `handle_connection` 任务
- 读写分离: 响应队列 (`mpsc::channel`) + 专用发送任务避免阻塞
- 超时控制: `connection_timeout_secs` 包裹整个会话

**进程管理** ([src/process.rs](src/process.rs)):
- `Mutex<HashMap<usize, Child>>` 管理子进程
- 支持普通启动和 `runas` (Windows 用户身份运行)
- 自动启动: `auto_start = true` 的服务在 `main()` 中启动

## 编码模式

### 添加新的 Request/Response 类型

1. 在 [common/src/lib.rs](common/src/lib.rs) 添加枚举变体:
   ```rust
   #[derive(Debug, Serialize, Deserialize, Clone)]
   pub enum Request {
       YourNewRequest { param: String },
   }
   
   pub enum Response {
       YourNewResponse(YourData),
   }
   ```

2. 在 [src/server.rs](src/server.rs) 的 `handle_request()` 添加 match 分支

3. 在 [client/src/lib.rs](client/src/lib.rs) 添加发送方法

4. 在 [A/src/](A/src/) 前端组件中调用

### 添加新的监控指标

在 [src/state.rs](src/state.rs) 的 `AppState` 添加缓存字段，然后在 [src/main.rs](src/main.rs) 的后台监控循环更新，最后在 `SystemInfo` struct 添加对应字段。

### 添加新的前端组件

**必须严格遵循** `.doc/` 目录下的 Dioxus 0.7 官方文档规范，特别是：
- **组件定义**: 参考 [.doc/dioxus-0.7-essentials.md](.doc/dioxus-0.7-essentials.md) 的组件章节
- **状态管理**: 参考 [.doc/dioxus-0.7-guides.md](.doc/dioxus-0.7-guides.md) 的 Signals 使用指南
- **动态样式**: 参考 [.doc/knowledge-base/dioxus-dynamic-styles.md](.doc/knowledge-base/dioxus-dynamic-styles.md)

示例代码（符合 Dioxus 0.7 规范）:
```rust
#[component]
fn MyComponent(name: String, count: ReadOnlySignal<i32>) -> Element {
    let mut local_state = use_signal(|| 0);
    
    rsx! {
        div { class: "my-class",
            "你好 {name}，计数: {count}"
            button { 
                onclick: move |_| *local_state.write() += 1,
                "本地值: {local_state}"
            }
        }
    }
}
```

**开发前必读文档**:
1. 查阅 `.doc/dioxus-0.7-tutorial.md` 了解基础用法
2. 参考 `.doc/dioxus-0.7-reference.md` 查找 API 详情
3. 遵循 `.doc/dioxus-0.7-guides.md` 的最佳实践

## 常见陷阱

1. **Dioxus 0.7 迁移**: 永远不要使用 `use_state` 或 `cx`，用 `use_signal` 和移除 `Scope` 参数
2. **加密 Nonce**: 不要手动管理 nonce，使用 `CryptoSession` 的自动计数器
3. **异步锁**: 避免在 `async` 块中持有 `std::sync::Mutex`，使用 `tokio::sync::Mutex`
4. **配置修改**: 修改 `config.toml` 后需重启服务器（无热重载）
5. **跨平台路径**: Windows 路径用反斜杠 `\`，但 Rust 字符串字面量需转义 `\\` 或用原始字符串 `r"C:\path"`

## 测试与调试

- 日志输出: 控制台 + `logs/server.log` (不自动轮转)
- 环境变量 `RUST_LOG=trace` 开启详细日志
- 客户端控制台查看 `gloo-console` 输出
- GPU 监控需 NVIDIA 驱动 (`nvml-wrapper` 依赖)
