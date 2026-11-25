# RemoteHelper

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-orange.svg)](https://www.rust-lang.org/)
[![GPUI Kit](https://img.shields.io/badge/UI-GPUI_Kit-blue.svg)](https://gpui-kit.com/)
[![License](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

RemoteHelper 用于监控远程 Windows 主机和管理主机上的服务。Windows 和 macOS 桌面客户端使用 Rust 与 GPUI Kit，通过加密 TCP 连接服务端。

- **系统监控**显示 CPU、内存和 NVIDIA GPU 的实时指标及历史阶梯图，支持滚轮缩放和拖动时间滑块。温度和功率取决于远程主机的硬件与驱动。
- **服务管理**支持查看状态与 PID、启动、停止、重启和添加远程服务。新增服务通过独立弹窗填写。
- **认证**使用 Ed25519 密钥。服务端通过公钥白名单授权客户端，会话使用 X25519 密钥交换和 AES-256-GCM 加密。
- **离线预览**可在调试版客户端中使用模拟数据查看仪表板。

## 快速开始

### 只体验客户端

在仓库根目录运行以下命令，在登录页点击「使用模拟数据」。

```sh
cargo run -p App
```

Windows 需要 MSVC Rust 工具链、Visual Studio C++ Build Tools 和 CMake。macOS 需要 macOS 15 或更新版本及 Xcode Command Line Tools。各平台的准备步骤见[客户端开发指南](crates/app/README.md)。

在监控页底栏可调整刷新间隔。顶部标签栏右侧的圆点显示连接状态，鼠标悬停在「资源使用率趋势」标题上可查看图表操作。

### 连接远程主机

1. 在 Windows 上生成 Ed25519 密钥并设置密码。命令会在终端显示公钥，并把加密密钥文件写入用户主目录的 `id_ed25519.json`。如果该文件已存在，请先备份，生成命令会覆盖它。

   ```sh
   cargo run --bin keygen
   ```

2. 在 Windows 服务端将 [config.example.toml](config.example.toml) 复制为工作目录下的 `config.toml`。把生成的公钥添加到 `[web_panel].authorized_keys`。客户端保管加密私钥文件。使用 macOS 客户端时，将加密密钥文件复制到 Mac。
3. 在包含 `config.toml` 的目录中启动服务端。首次从源码启动时，在仓库根目录运行以下命令。

   ```sh
   cargo run --bin RemoteHelperServer
   ```

4. 在桌面客户端输入 `主机地址:端口`。同机调试可用 `127.0.0.1:9999`。选择密钥文件，输入密码后连接。用户主目录下的 `id_ed25519.json` 可自动选用。

### 构建发布版本

```sh
# 桌面客户端
cargo build --release -p App

# Windows 服务端
cargo build --release --bin RemoteHelperServer
```

Windows 部署使用 `target/release/RemoteHelperServer.exe`，从包含 `config.toml` 的工作目录启动。如果设置了 Cargo 输出目录，以实际目录为准。桌面客户端的打包产物和发布流程见[客户端开发指南](crates/app/README.md)。

## 服务端配置

`[web_panel]` 和 `allow_web_control` 是现有配置使用的兼容名称。修改 `config.toml` 后重启服务端。

| 选项 | 用途 |
| --- | --- |
| `[web_panel].enabled` | 是否启动可选的 frpc 隧道。关闭后服务端仍监听 TCP 端口 |
| `[web_panel].local_port` | TCP 监听端口。示例使用 `9999` |
| `[web_panel].frpc_exe_path` / `frpc_args` | frpc 程序路径和启动参数。示例直接使用局域网连接 |
| `[web_panel].authorized_keys` | 获准连接的 Base64 公钥列表 |
| `[web_panel].startup_delay_secs` / `health_check_url` | 启动前等待时间和可选的网络检测地址 |
| `[web_panel].max_connections` | 服务端同时接受的 TCP 连接数，默认 `100`。实时请求、历史同步和历史管理连接均计入 |
| `[web_panel].connection_timeout_secs` | 单条 TCP 连接的最长寿命，默认 `300` 秒，包含握手时间；持续轮询也不会延长它 |
| `[[service]].allow_web_control` | 是否允许远程客户端控制该服务 |
| `[[service]].auto_start` | 启动策略，支持 `none`、`oneshot` 和 `continuous` |

客户端添加的动态服务保存在服务端内存中。需要重启后继续可用的服务，请写入服务端配置。

## 开发

| 路径 | 职责 |
| --- | --- |
| [crates/app/](crates/app/) | GPUI Kit 桌面界面和本地登录设置 |
| [crates/client/](crates/client/) | TCP 连接、认证、请求和本地历史同步 |
| [crates/common/](crates/common/) | 请求与响应类型、加密会话和消息帧 |
| [crates/history/](crates/history/) | 服务端和客户端共用的历史文件格式与编解码 |
| [crates/hwlib/](crates/hwlib/) 和 [PawnIO/](PawnIO/) | Windows 硬件监控及驱动模块资源 |
| [crates/server/](crates/server/) | Windows 服务端、监控任务、历史记录、请求分派和进程管理 |

在 Windows 上检查整个工作区。只开发桌面客户端时，也可单独检查 `App`。

```sh
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
cargo clippy -p App --no-deps --all-targets -- -D warnings
```

协议变更从[共享协议定义](crates/common/src/lib.rs)的 `Request` 和 `Response` 开始，同步检查[服务端请求处理器](crates/server/src/handlers/)、[客户端请求入口](crates/client/src/requests.rs)及对应的界面调用方。参见[贡献指南](CONTRIBUTING.md)和[变更记录](CHANGELOG.md)。

## 故障排查

- **无法连接**时，检查服务端进程、监听端口和客户端输入的地址。使用 frpc 时，同时检查隧道进程。
- **连接约每五分钟断开**时，先检查 `[web_panel].connection_timeout_secs`。客户端遇到连接读写错误时，会对只读请求重连并重试一次；若请求本身等满 5 秒，则直接报超时，下一次轮询再连接。写操作不会自动重放；报错后先刷新服务列表或相关设置，确认操作是否已生效。
- **实时监控正常但历史连不上**时，先确认服务端身份已初始化且指纹已核对，再检查 `[web_panel].max_connections`。实时连接已占用一个名额时，至少还要为历史同步预留一个名额。
- **只有大服务列表等普通响应反复失败**时，检查是否超过客户端的 64 KiB 加密帧体接收上限。历史连接的 1 MiB 上限不适用于普通请求；重连不能解决帧过大的问题。
- **认证失败**时，检查服务端白名单是否包含密钥文件对应的公钥，并核对密码。
- **GPU 指标缺失**时，检查远程主机的 NVIDIA 驱动和服务端 NVML 日志。
- **部分传感器指标缺失**时，查看服务端硬件驱动初始化日志。驱动失败时，服务端仍可提供其他可用指标。

服务端向控制台和 `logs/server.YYYY-MM-DD.log` 输出日志，主日志按日轮转。服务和隧道分别写入 `logs/service_{id}.log` 和 `logs/web_tunnel.log`，打开日志文件时如果发现超过 10 MiB，会先轮转。

## 后续计划

- 动态服务持久化
- TLS 证书支持
- 每客户端速率限制
- Prometheus 指标端点
- 多 GPU 支持
- Linux 平台支持

## 许可证与致谢

项目采用 [MIT 许可证](LICENSE)。主要依赖包括 [GPUI Kit](https://gpui-kit.com/)、[Tokio](https://tokio.rs/)、[sysinfo](https://github.com/GuillaumeGomez/sysinfo) 和 [nvml-wrapper](https://github.com/Cldfire/nvml-wrapper)。

欢迎通过 [GitHub Issues](https://github.com/Hughsean/RemoteHelper/issues) 提交问题。
