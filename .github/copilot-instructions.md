# RemoteHelper 开发指引

RemoteHelper 使用 Cargo Workspace。根包 `RemoteHelperServer` 提供 Windows 服务端和 `keygen`。桌面包 `App` 使用 Rust 与 GPUI Kit，面向 Windows 和 macOS。

| 路径 | 职责 |
| --- | --- |
| `app/src/` | 登录、监控仪表板、趋势图和服务管理 |
| `client/src/` | Tokio TCP 连接、认证、请求封装和模拟数据 |
| `common/src/` | 协议类型、消息帧、会话加密和密钥文件格式 |
| `src/` | 服务端连接、认证、请求分派、监控和进程管理 |
| `hwlib/`、`PawnIO/` | Windows 硬件监控和驱动模块资源 |

## 开发命令

在仓库根目录执行。涉及服务端和硬件驱动时，在 Windows 上检查整个工作区。

```sh
cargo run -p App
cargo test -p App
cargo clippy -p App --no-deps --all-targets -- -D warnings
cargo run --bin RemoteHelperServer
cargo check --workspace
```

调试版客户端的登录页提供模拟数据。真实连接流程和构建前置条件见[根目录 README](../README.md)和[客户端指南](../app/README.md)。

## 桌面客户端

- 窗口、颜色和资源集中在 `app/src/main.rs`。新增图标时确认 `AppAssets` 能加载对应路径。可复用的控件见 [GPUI Kit 组件文档](https://gpui-kit.com/zh-CN/component/)。
- 界面状态通过 GPUI `Entity` 和 `Context` 更新，更新后调用 `cx.notify()`。后台网络请求在 Tokio 运行时执行，由 GPUI 前台任务接收结果并更新视图。
- 登录使用原生文件选择器和现有的客户端认证流程。登录设置只保存服务器地址和密钥文件路径。
- 仪表板轮询与图表动画是独立过程。保持实际采样时间、缺失指标的处理、暂停冻结和窗口高度自适应行为。
- 路径补全针对远程主机文件系统，继续复用 `client::query_path`。

## 服务端和协议

- 服务端先完成 X25519 密钥交换，再验证客户端对挑战的 Ed25519 签名及公钥白名单。通过 [`CryptoSession`](../common/src/crypto.rs) 管理 AES-256-GCM 会话和 nonce。
- 增加协议消息时，先修改 [`Request` 与 `Response`](../common/src/lib.rs)，再检查 [`src/handlers/`](../src/handlers/)、[`client/src/requests.rs`](../client/src/requests.rs) 和对应视图。
- 监控采样入口在 [`src/monitor.rs`](../src/monitor.rs)。添加指标时同时检查采样、状态缓存、协议字段及界面显示。
- 服务端配置格式以 [`src/config.rs`](../src/config.rs) 为准。修改配置时同步更新 [配置示例](../config.example.toml)。`web_panel` 和 `allow_web_control` 是供现有部署使用的字段名。
- 不跨 `.await` 持有同步锁。运行时日志使用 `tracing`。
