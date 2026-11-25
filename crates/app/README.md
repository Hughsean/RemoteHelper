# RemoteHelper 桌面客户端

客户端使用 GPUI Kit 0.11，通过 `client` 和 `common` 连接 RemoteHelper 服务端。界面跟随系统深浅模式，窗口与卡片使用分层半透明底色。Windows 11 22H2 起优先使用 DWM 桌面亚克力，较早的受支持版本使用 GPUI 原生模糊。macOS 使用原生模糊。材质效果取决于系统的透明度和合成设置。

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

开发趋势图时，可运行离线预览，查看两小时的合成历史。在 Windows PowerShell 中运行 `$env:REMOTEHELPER_PREVIEW="1"; cargo run -p App`。在 macOS 终端中运行 `REMOTEHELPER_PREVIEW=1 cargo run -p App`。可选的 `REMOTEHELPER_PREVIEW_FROZEN=1` 会自动缩放到历史窗口，`REMOTEHELPER_PREVIEW_HOVER=1` 会显示读数卡。

## 界面和数据流

`main` 启动进程和网络运行时，`bootstrap` 装配 GPUI 应用，`shell` 管理根视图、导航和底栏。页面在 `views/` 下。

| 文件 | 职责 |
| --- | --- |
| [src/main.rs](src/main.rs) | 进程入口：日志和 Tokio 运行时 |
| [src/bootstrap.rs](src/bootstrap.rs) | GPUI 应用装配：资源、主题、窗口重开（Dock 图标还原）和开窗 |
| [src/shell.rs](src/shell.rs) | 页面切换动效、顶部导航、底栏和关窗前页签记忆 |
| [src/theme.rs](src/theme.rs) | 系统主题、配色和原生窗口材质 |
| [src/notifications.rs](src/notifications.rs) | 窗口级提示、警告和错误通知 |
| [src/ui.rs](src/ui.rs) | 面板、字段、提示文字和导航按钮等共用构件 |
| [src/assets.rs](src/assets.rs) | 图标资源和资源加载测试 |
| [src/preview.rs](src/preview.rs) | 调试版离线趋势预览 |
| [src/views/login.rs](src/views/login.rs) | 原生文件选择、登录设置和连接流程 |
| [src/views/dashboard.rs](src/views/dashboard.rs) | 监控指标、底栏刷新间隔菜单和连接状态更新 |
| [src/views/trend/](src/views/trend/mod.rs) | 阶梯图、采样缓冲区、时间桶均值、滚轮缩放和历史滑块 |
| [src/views/services.rs](src/views/services.rs) | 服务列表、启停重启、新增弹窗和远程路径补全 |

按钮、输入框、通知和弹窗复用 [GPUI Kit 组件库](https://gpui-kit.com/zh-CN/component/)。网络任务交给 Tokio，结果返回 GPUI 视图后更新状态。提示、警告和错误在窗口右下角展示。新增服务在独立弹窗中填写。页面滑入动效遵循系统的减少动画设置。

顶部标签栏右侧用绿、黄、红三色圆点表示在线、连接中和已断开。监控页底栏右侧可将刷新间隔设为 0.5 秒、1 秒、3 秒或暂停。暂停时，连接圆点显示最近一次确认的状态。

趋势图以直角阶梯显示采样值。拉宽时间范围时，图表根据图宽按时间段合并密集采样，并在悬停卡片中注明均值周期。缩回细节范围后显示原始采样。悬停在「资源使用率趋势」标题上可查看交互说明。滚轮以指针时刻为锚点缩放，Shift + 滚轮平移。底部滑块显示整段历史，可拖动选框平移、拖两侧手柄调整范围，或点击框外跳转。点击滑块右侧箭头回到实时，双击图表或滑块恢复默认五分钟窗口。交互参考 [Argus](https://github.com/Ray-lea/Argus) 的历史页。

## 发布

[标签发布流程](../../.github/workflows/release.yml)构建 Windows x64 和 macOS Apple Silicon 客户端，并另外发布 Windows x64 服务端压缩包。不再自动发布 Intel Mac 安装包。

| 平台 | 产物 |
| --- | --- |
| Windows x64 | `RemoteHelper-x86_64-pc-windows-msvc.exe` |
| macOS Apple Silicon | `RemoteHelper-aarch64-apple-darwin.dmg` |

macOS 应用包使用 ad hoc 签名，尚未进行 Developer ID 签名或公证。详见[服务端启动与部署](../../README.md)。
