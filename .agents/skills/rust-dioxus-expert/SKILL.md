---
name: rust-dioxus-expert
description: Rust 和 Dioxus 开发超级专家。当需要编写或审查 Rust 后端代码、Dioxus 0.7 前端组件、加密协议实现、Tokio 异步并发、以及 RemoteHelper 项目特有的客户端-服务器架构时使用此技能。
---

# Rust & Dioxus 开发超级专家

你是 Rust 和 Dioxus 0.7 的超级专家，专精于 RemoteHelper 项目的全栈开发。在编写任何代码前，先通读相关源文件，理解现有模式后再动手。

## 项目架构理解

RemoteHelper 是一个 Cargo Workspace，由以下成员组成：

| 成员 | 路径 | 角色 |
|------|------|------|
| 服务器主程序 | `src/` | Tokio TCP 服务器，监控系统指标，管理子进程 |
| 共享协议库 | `common/` | `Request/Response` 枚举、`CryptoSession` 加密会话 |
| TCP 客户端库 | `client/` | 连接管理、Ed25519 认证、全局 `CONNECTION` 状态 |
| Dioxus 前端 | `app/` (Cargo.toml 中名为 `app`) | Web (WASM) + 桌面 (Tauri v2) 双平台 UI |
| 硬件抽象层 | `hwlib/` | 未激活使用 |

**数据流**: 前端组件 → `client::CONNECTION` → 加密 TCP → `src/server.rs` → `handle_request()` → `AppState`

## Rust 编码规范

### 1. 错误处理

- **库代码**（`common/`, `client/`）：返回 `Result<T, E>`，不使用 `unwrap()`。使用 `anyhow::Result` 进行快速原型，但关键库函数应定义明确的错误类型。
- **服务器代码**（`src/`）：使用 `anyhow::Result<T>` 搭配 `?` 操作符，在边界处使用 `.context("...")` 添加上下文。
- **前端代码**（`app/`）：网络错误使用 `String` 作为错误类型，通过 `use_resource` 的 `Result` 变体处理。

```rust
// ✅ 好的库代码
pub fn parse_config(path: &str) -> Result<AppConfig, ConfigError> { ... }

// ✅ 好的服务器代码
let data = fetch_data().context("获取系统指标失败")?;

// ✅ 好的前端代码
let status: Resource<Result<SystemInfo, String>> = use_resource(move || async move {
    client::get_status(interval).await.map_err(|e| e.to_string())
});
```

### 2. 并发与异步

**Tokio 模式（服务器端）**：
```rust
// 后台任务生成
tokio::spawn(async move {
    loop {
        // 定期刷新数据
        let data = refresh_data(&state).await;
        *state.cache.write().await = data;
        tokio::time::sleep(Duration::from_millis(interval)).await;
    }
});

// 读写锁模式（读多写少场景）
let current = *state.config_cache.read().await;
// 需要修改时再获取写锁
*state.config_cache.write().await = new_value;
```

**关键规则**：
- 使用 `tokio::sync::Mutex` 而非 `std::sync::Mutex` 在 `async` 上下文中
- `Arc<RwLock<T>>` 用于读多写少的共享状态
- `mpsc::channel` 用于解耦读写操作（见 `src/server.rs` 的响应队列模式）
- `Notify` 用于唤醒等待任务（见 `state.update_notify` 模式）

### 3. 模式匹配与枚举

项目使用丰富的枚举驱动设计。在 `common/src/lib.rs` 的 `Request`/`Response` 枚举是协议核心：

```rust
// 添加新请求类型的完整流程：
// 1. common/src/lib.rs → 添加 Request/Response 变体
// 2. src/server.rs → handle_request() 添加匹配分支
// 3. client/src/lib.rs → 添加发送函数
// 4. app/src/ → 在前端组件中调用
```

### 4. 内存与性能

- 大型数据结构（`SystemInfo`）使用 `Clone`，小数据用引用或 `Copy`
- Dioxus 中使用 `ReadOnlySignal<T>` 而非克隆整个值
- 使用 `Arc` 而非深拷贝共享配置
- 注意 `Drop` 实现中的零化敏感数据（见 `CryptoSession` 的 `EphemeralSecret` 和 `SharedSecret`）

## Dioxus 0.7 前端开发规范

### ⚠️ 强制规则

**永远不要使用以下已废弃的 API**：
- ❌ `use_state`, `use_ref`, `cx: Scope`
- ❌ `Props` struct 模式
- ❌ `move |_|` 无必要的闭包前缀（仅在需要捕获外部变量时使用）

**必须使用的 API**：
- ✅ `use_signal(|| initial_value)` — 本地可变状态
- ✅ `use_memo(|| derived_value)` — 派生计算结果
- ✅ `use_resource(|| async { ... })` — 异步数据获取
- ✅ `use_context_provider::<T>()` / `use_context::<T>()` — 全局状态
- ✅ `Signal<T>` / `ReadOnlySignal<T>` / `ReadSignal<T>`

### 组件定义规范

```rust
#[component]
pub fn MyComponent(
    /// 文档注释作为 props 文档
    name: String,                          // 静态 props（非响应式）
    count: ReadOnlySignal<i32>,            // 响应式 props
    on_action: EventHandler<()>,           // 回调 props
    children: Element,                     // 子元素
) -> Element {
    let mut local_state = use_signal(|| 0);

    rsx! {
        div { class: "my-component",
            "Hello {name}, count: {count}"
            button {
                onclick: move |_| {
                    *local_state.write() += 1;
                    on_action.call(());
                },
                "本地值: {local_state}"
            }
            {children}
        }
    }
}
```

### RSX 规范

1. **循环使用 `for` 而非 `.map()`**（Dioxus 0.7 最佳实践）：
```rust
// ✅ 推荐
for item in items.iter() {
    div { key: "{item.id}", "{item.name}" }
}

// ❌ 避免
{items.iter().map(|item| rsx! { div { "{item.name}" } })}
```

2. **条件渲染**：
```rust
// if 表达式
if let Some(value) = optional_data {
    span { "{value}" }
}

// match 表达式
match status {
    Status::Loading => rsx! { Spinner {} },
    Status::Error(e) => rsx! { ErrorBanner { message: e } },
    Status::Ready(data) => rsx! { DataView { data } },
}
```

3. **样式资源引用**：
```rust
const COMPONENT_CSS: Asset = asset!("/assets/components/my_component/style.css");

// 在 rsx! 中的第一个元素内注入
rsx! {
    div { class: "my-wrapper",
        document::Link { rel: "stylesheet", href: COMPONENT_CSS }
        // ... 其余内容
    }
}
```

### 状态管理模式

**模式 1：本地状态（组件内）**
```rust
let mut count = use_signal(|| 0);
// 读取：count()
// 写入：*count.write() += 1;
```

**模式 2：全局上下文状态（跨组件共享）**
```rust
// 在根组件提供
use_context_provider(|| Signal::new(AppConfig::default()));

// 在子组件消费
let config: Signal<AppConfig> = use_context();
```

**模式 3：异步资源（自动依赖追踪）**
```rust
let interval = use_signal(|| 1000);
let status = use_resource(move || async move {
    // 当 interval 变化时自动重新执行
    client::get_status(Some(interval())).await
});
```

**模式 4：与服务器通信**
```rust
// 使用 client 库的全局 CONNECTION
use client::CONNECTION;

let mut data = use_signal(|| None::<SystemInfo>);
let fetch_task = use_resource(move || async move {
    let conn = CONNECTION.lock().await;
    if let Some(conn) = conn.as_ref() {
        // 发送请求并获取响应
    }
});
```

## 加密协议规范

当涉及加密相关代码时，必须遵循以下规范：

### CryptoSession 使用

```rust
// 握手阶段：X25519 ECDH
let (secret, public) = common::crypto::generate_ephemeral();
// 交换公钥后派生共享密钥
let shared_secret = secret.diffie_hellman(&their_public);

// 创建加密会话
let mut session = CryptoSession::new(shared_secret.to_bytes(), is_server);

// 加密/解密（自动管理 nonce）
let ciphertext = session.encrypt(plaintext)?;
let plaintext = session.decrypt(&ciphertext)?;
```

### 认证流程（基于 client/src/lib.rs 模式）

1. 客户端发送 `ClientHello` → 服务器响应 `ServerHello`（X25519 公钥交换）
2. 客户端发送 `GetChallenge` → 服务器返回随机 `Challenge(challenge_bytes)`
3. 客户端用 Ed25519 私钥签名挑战 → 发送 `Login { public_key, signature }`
4. 服务器用白名单公钥验证签名 → 建立加密会话

### 安全检查清单

- [ ] Nonce 是否通过 `CryptoSession` 自动管理（切勿手动构造）
- [ ] 敏感数据是否在 `Drop` 中零化（`fill(0)`）
- [ ] 是否每次连接生成新的临时密钥对
- [ ] Ed25519 公钥是否经过白名单验证

## 配置管理规范

### config.toml 结构（src/config.rs 解析）

```toml
[web_panel]
enabled = true
local_port = 9999
authorized_keys = ["base64_encoded_public_key"]
max_connections = 100
connection_timeout_secs = 300
startup_delay_secs = 0
health_check_url = "http://example.com/health"

[[service]]
description = "My Service"
exe_path = "C:\\path\\to\\service.exe"
args = ["--flag", "value"]
auto_start = false
allow_web_control = true
```

### 配置修改规范

- 永远在 `AppConfig` struct 中添加对应的 serde 字段
- 使用 `#[serde(default)]` 保证向后兼容
- 所有可配置值通过 `state.config` 访问，不要硬编码
- 修改配置后需要重启服务器，没有热加载机制

## 项目文件组织

### 服务器端（src/）

```
src/
├── main.rs          # 入口点、后台监控循环、main()
├── server.rs        # TCP 连接处理、握手、请求分发
├── config.rs        # 配置文件解析 (AppConfig, ServiceConfig)
├── state.rs         # 全局状态 (AppState, GpuCache, ServiceProcess)
├── process.rs       # 子进程管理 (启动/停止/重启/runas)
└── utils.rs         # 工具函数 (网络检测等)
```

### 前端（app/）

```
app/
├── src/
│   ├── main.rs      # App 组件、路由定义
│   ├── views/       # 页面级组件
│   │   ├── mod.rs
│   │   ├── login.rs
│   │   ├── home.rs
│   │   ├── services.rs
│   │   └── test.rs
│   └── widgets/     # 可复用组件
│       ├── mod.rs
│       ├── card.rs
│       ├── metric_card.rs
│       ├── trend_chart.rs
│       └── add_service_dialog.rs
├── assets/
│   ├── styling/     # 全局 CSS
│   └── components/  # 组件级 CSS
├── Cargo.toml
└── Dioxus.toml
```

### 添加新组件的步骤

1. 在 `app/src/widgets/` 创建 `my_component.rs`
2. 在 `app/src/widgets/mod.rs` 注册：`pub mod my_component;`
3. 在 `app/assets/components/my_component/` 创建 `style.css`
4. 在组件中使用 `asset!("/assets/components/my_component/style.css")` 引用
5. 在需要使用的页面中导入并使用

## 常见反模式与陷阱

### 1. 在 async 块中持有同步锁
```rust
// ❌ 危险 - std::sync::Mutex 跨越 .await
async fn bad(state: &AppState) {
    let guard = state.std_mutex.lock().unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await; // 锁在此处仍被持有！
}

// ✅ 正确 - 在 .await 前释放锁
async fn good(state: &AppState) {
    {
        let guard = state.std_mutex.lock().unwrap();
        // 仅在同步代码中使用
    } // 锁在此处释放
    tokio::time::sleep(Duration::from_secs(1)).await;
}
```

### 2. Dioxus 中的信号误用
```rust
// ❌ 在闭包外读取信号值
let value = my_signal();  // 获取当前快照，不会响应式更新
rsx! { span { "{value}" } }

// ✅ 在 RSX 中直接读取（自动追踪）
rsx! { span { "{my_signal}" } }

// ❌ 在 use_resource 中遗漏信号依赖
let resource = use_resource(move || async move {
    // interval 不被追踪，变化时不会重新执行
    fetch_data(1000).await
});

// ✅ 在闭包中读取信号建立依赖
let resource = use_resource(move || async move {
    let ms = interval(); // 建立响应式依赖
    fetch_data(ms).await
});
```

### 3. 配置硬编码
```rust
// ❌ 魔法数字
tokio::time::sleep(Duration::from_secs(300)).await;

// ✅ 从配置读取
tokio::time::sleep(Duration::from_secs(state.config.web_panel.connection_timeout_secs)).await;
```

### 4. 不完整的 match 分支
在 `handle_request()` 中添加新 `Request` 变体时，确保在所有 match 分支中处理：
- `src/server.rs` 的 `handle_request()` — 服务器端处理
- `client/src/lib.rs` — 客户端发送逻辑
- 前端组件的响应处理

## 测试与调试

### 服务器调试
```bash
# 开启详细日志
RUST_LOG=trace cargo run

# 日志输出位置
# - 控制台 stdout
# - logs/server.log
```

### 前端调试
```bash
# Web 模式
cd app && dx serve

# 桌面模式
cd app && dx serve --platform desktop

# 浏览器控制台使用 gloo-console
gloo_console::log!("debug message");
```

### 常用验证命令
```bash
cargo check                    # 快速语法检查（全 workspace）
cargo check -p common          # 仅检查 common 库
cargo run --bin keygen         # 生成测试密钥
cargo build --release          # 生产构建（LTO 优化）
```

## 编码前检查清单

在编写任何代码之前，问自己：

1. **协议变更**：是否需要修改 `common/src/lib.rs` 的 `Request`/`Response`？
2. **状态变更**：是否需要修改 `AppState`（`src/state.rs`）？
3. **配置变更**：是否需要修改 `AppConfig`（`src/config.rs`）和 `config.toml` 示例？
4. **前端影响**：新功能需要在 `app/` 中显示吗？
5. **加密涉及**：是否涉及密钥、签名或加密操作？如果是，回顾加密协议规范。
6. **并发考虑**：新代码是否跨越 `.await` 持有锁？是否使用了正确的 Tokio 锁类型？
7. **Dioxus 版本**：是否使用了 `use_signal`（非 `use_state`）？是否使用 `#[component]` 注解？
