# APP-D 项目结构文档

## 🎯 项目定位

APP-D 是 RemoteHelper 的纯 Dioxus Desktop 实现，采用现代化的 Rust 前端架构。

## 📁 目录结构

```
App-D/
├── src/
│   ├── main.rs              # 应用入口，配置窗口和日志
│   ├── state/               # 状态管理模块 ✅
│   │   ├── mod.rs           # 导出 AppState
│   │   ├── auth.rs          # 认证状态（Signal）
│   │   ├── system.rs        # 系统监控状态
│   │   └── services.rs      # 服务管理状态
│   ├── backend/             # 后端通信模块 ✅
│   │   ├── mod.rs           # 全局连接管理
│   │   └── api.rs           # API 调用封装
│   ├── components/          # UI 组件（待迁移）
│   │   └── mod.rs
│   └── views/               # 路由页面
│       ├── mod.rs
│       └── home.rs          # 临时首页
├── assets/
│   ├── styling/
│   │   ├── main.css         # 主样式（整合）✅
│   │   ├── base.css         # 基础样式 ✅
│   │   ├── header.css       # 头部样式 ✅
│   │   ├── login.css        # 登录样式 ✅
│   │   ├── status.css       # 状态显示样式 ✅
│   │   ├── services.css     # 服务列表样式 ✅
│   │   └── chart.css        # 图表样式 ✅
│   ├── fonts/
│   │   └── LXGWWenKaiMono-Regular.woff2  ✅
│   ├── favicon.ico
│   └── tailwind.css
├── Cargo.toml               # 项目依赖 ✅
├── Dioxus.toml              # Dioxus 配置 ✅
└── README.md
```

## 🏗️ 架构设计

### 状态管理（Signal + Context 模式）

```rust
// 在 App 组件提供全局状态
use_context_provider(|| AppState::default());

// 在子组件消费状态
let mut auth = use_context::<AppState>().auth;
auth.set_authenticated();
```

#### AuthState

- `authenticated: Signal<bool>` - 认证状态
- `connected: Signal<bool>` - 连接状态
- `error_msg: Signal<Option<String>>` - 错误消息
- `server_addr: Signal<String>` - 服务器地址

#### SystemState

- `current_status: Signal<Option<SystemInfo>>` - 当前状态
- `history: Signal<VecDeque<(u64, SystemInfo)>>` - 历史数据
- `refresh_interval: Signal<u64>` - 刷新间隔

#### ServicesState

- `services: Signal<Vec<ServiceInfo>>` - 服务列表
- `show_add_modal: Signal<bool>` - 添加服务模态框
- `operating_service: Signal<Option<usize>>` - 操作中的服务

### 后端通信（全局连接池）

```rust
// 连接到服务器
backend::authenticate(password, addr).await?;

// 调用 API
let status = backend::get_status(Some(1000)).await?;
let services = backend::list_services().await?;
backend::control_service(id, "restart".to_string()).await?;
```

## 📋 待完成任务

### 1. 组件迁移（优先级：高）

从 `app/src/components/` 迁移到 `App-D/src/components/`：

- [ ] `login.rs` - 登录组件
  - 使用 `AuthState` 管理状态
  - 调用 `backend::authenticate()`
  - 错误处理和加载状态

- [ ] `header.rs` - 头部导航
  - 显示连接状态
  - 刷新间隔设置
  - 登出功能

- [ ] `system_status.rs` - 系统状态显示
  - 从 `SystemState` 读取数据
  - CPU/内存/网络/GPU 可视化
  - 响应式布局

- [ ] `trend_chart.rs` - 趋势图表
  - 使用 `SystemState.get_recent_data()`
  - Canvas 或 SVG 渲染
  - 时间轴控制

- [ ] `service_list.rs` - 服务列表
  - 从 `ServicesState` 读取
  - 启动/停止/重启按钮
  - 加载状态指示器

- [ ] `add_service_modal.rs` - 添加服务
  - 表单验证
  - 调用 `backend::add_service()`
  - 模态框控制

### 2. 路由扩展

```rust
#[derive(Routable, Clone, PartialEq)]
enum Route {
    #[layout(MainLayout)]  // 带导航栏的布局
        #[route("/")]
        Dashboard,          // 主监控面板
        
        #[route("/services")]
        Services,           // 服务管理页
    
    #[route("/login")]
    Login,                  // 登录页（无布局）
}
```

### 3. 数据刷新机制

使用 `use_resource` 实现自动刷新：

```rust
let status = use_resource(move || {
    let interval = system_state.refresh_interval();
    async move {
        loop {
            if auth_state.authenticated() {
                backend::get_status(Some(interval)).await?;
                tokio::time::sleep(Duration::from_millis(interval)).await;
            }
        }
    }
});
```

### 4. 错误处理优化

- 统一错误提示组件
- 自动重连机制
- 网络超时处理

### 5. 打包配置

```toml
# Dioxus.toml
[bundle]
identifier = "com.hughsean.remotehelper"
publisher = "Hughsean"
icon = ["assets/icon.png"]
resources = ["assets/**/*"]

# Windows 专属配置
[bundle.windows]
wix = true
```

## 🚀 开发工作流

### 启动开发服务器

```bash
cd App-D
dx serve --platform desktop --hot-reload
```

### 构建发布版本

```bash
dx build --platform desktop --release
```

### 打包应用

```bash
dx bundle --platform desktop --release
```

## 🔧 技术栈对比

| 特性 | app (Tauri + Dioxus Web) | App-D (纯 Dioxus Desktop) |
|------|--------------------------|---------------------------|
| 前端框架 | Dioxus 0.7 (WASM) | Dioxus 0.7 (Native) |
| 窗口管理 | Tauri v2 | Dioxus Desktop |
| 通信方式 | Tauri Commands | 直接 Rust 调用 |
| 打包体积 | ~15MB | ~8MB |
| 启动速度 | 较慢（WASM 加载） | 快（原生） |
| 开发体验 | 热重载 | 热重载 |
| 系统集成 | 深度（托盘等） | 基础 |

## 📚 参考资源

- [Dioxus 0.7 文档](https://dioxuslabs.com/learn/0.7/)
- [RemoteHelper 架构文档](../.github/copilot-instructions.md)
- [Dioxus 知识库](../.doc/knowledge-base/)

## ✅ 当前进度

- ✅ 项目配置（Cargo.toml, Dioxus.toml）
- ✅ 状态管理模块（state/）
- ✅ 后端通信模块（backend/）
- ✅ 资源配置（CSS, 字体）
- ⏳ UI 组件迁移（0/6）
- ⏳ 路由配置
- ⏳ 测试和优化
