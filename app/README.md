# RemoteHelper 桌面客户端

客户端使用 GPUI Kit 0.11，通过 `client` 和 `common` 连接 RemoteHelper 服务端。界面采用深色主题，Windows 11 窗口启用系统 Mica 背景。

## 构建和运行

- Windows 10 或更新版本需要 MSVC Rust 工具链、Visual Studio C++ Build Tools 和 `PATH` 上的 CMake。
- macOS 15 或更新版本需要 Xcode Command Line Tools。

工具链的完整要求见 [GPUI Kit 安装指南](https://gpui-kit.com/zh-CN/docs/installation/)。

在仓库根目录运行以下命令。

```sh
cargo run -p App
cargo build --release -p App
cargo test -p App
cargo clippy -p App --no-deps --all-targets -- -D warnings
```

调试构建的登录页提供「使用模拟数据」。连接远程主机时，可手动选择密钥文件，或使用用户主目录中的 `id_ed25519.json`。登录成功后，客户端会在系统配置目录的 `remotehelper/login_data.json` 中保存服务器地址和密钥文件路径。每次连接都需输入密钥密码。

## 界面和数据流

代码分四层：`main` 只做进程入口，`bootstrap` 装配 GPUI 应用，`shell` 是根视图（配 `theme`、`ui`、`assets` 三个支撑模块），页面都在 `views/` 下。

| 文件 | 职责 |
| --- | --- |
| [src/main.rs](src/main.rs) | 进程入口：日志和 Tokio 运行时 |
| [src/bootstrap.rs](src/bootstrap.rs) | GPUI 应用装配：资源、主题和开窗 |
| [src/shell.rs](src/shell.rs) | 根视图：登录页与会话页切换 |
| [src/theme.rs](src/theme.rs) | 主题初始化与配色 |
| [src/ui.rs](src/ui.rs) | 面板、字段、提示文字和导航按钮等共用构件 |
| [src/assets.rs](src/assets.rs) | 图标资源和资源加载测试 |
| [src/views/login.rs](src/views/login.rs) | 原生文件选择、登录设置和连接流程 |
| [src/views/dashboard.rs](src/views/dashboard.rs) | 监控数据、刷新下拉菜单、时间范围下拉菜单和自适应布局 |
| [src/views/trend.rs](src/views/trend.rs) | GPUI Canvas 绘图、历史缓冲区、曲线计算及动画 |
| [src/views/services.rs](src/views/services.rs) | 服务列表、启停重启、添加服务和远程路径补全 |

按钮、输入框、下拉菜单和图标复用 [GPUI Kit 组件库](https://gpui-kit.com/zh-CN/component/)。网络任务交给 Tokio，结果返回 GPUI 视图后更新状态。趋势图按采样时间逐帧推进，支持 1 分钟、5 分钟、30 分钟和 3 小时范围。暂停时冻结曲线，图表高度随窗口调整。

## 发布

[标签发布流程](../.github/workflows/release.yml)构建三个客户端产物。

| 平台 | 产物 |
| --- | --- |
| Windows x64 | `RemoteHelper-x86_64-pc-windows-msvc.exe` |
| macOS Intel | `RemoteHelper-x86_64-apple-darwin.dmg` |
| macOS Apple Silicon | `RemoteHelper-aarch64-apple-darwin.dmg` |

macOS 应用包使用 ad hoc 签名，尚未进行 Developer ID 签名或公证。详见[服务端启动与部署](../README.md)。
