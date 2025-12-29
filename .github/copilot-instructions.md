# FrpcStartup/RemoteHelper 协作指南（面向 AI 代理）

- **项目目的与入口**：Windows 常驻辅助程序，用于自启动/管理多个进程（主要是 frpc.exe，也可带 Telegraf 等代理），可选内置 Web 控制台。主入口在 [src/main.rs](src/main.rs)。
- **包/产物命名**：Cargo 包名 `RemoteHelper`，部署脚本目标目录/服务名也使用 RemoteHelper（见 [deploy.ps1](deploy.ps1)、[move_and_restart.ps1](move_and_restart.ps1)）。
- **配置契约**：运行时读取 [config.toml](config.toml)，解析逻辑在 [src/config.rs](src/config.rs)。`[[service]]` 默认补全：`auto_start=false`、`exe_path="frpc.exe"`、`args` 优先且由旧字段 `arg` 推导、`allow_web_control=true`；新增字段需保持旧配置兼容。
- **ServiceManager 机制**：核心状态在 [src/frpc.rs](src/frpc.rs)，`processes: Vec<Option<Child>>` 与 `config.service` 顺序一致。必须经 `start_service`/`stop_service` 管理，日志与句柄才能对应；健康检查与后续重启逻辑集中在 `check_health`。每次启动生成 `logs/service_<idx>_timestamp.log`。
- **启动流程**：主进程先写分隔日志并等待 25 秒保证网络/DNS，再加载配置。若 `web_control.enabled=false` 或缺省，直接 `start_all()`；否则仅启 Web，业务服务由 Web 触发。主循环每 5 秒健康检查，Ctrl+C 置 `shutdown`，Web 线程死亡则退出。
- **Web 控制台协议**：实现于 [src/web.rs](src/web.rs)，自建 TCP HTTP，先拉起隧道进程 `frpc.exe -f <frpc_arg>` 监听 `0.0.0.0:<local_port>`。POST `/api/action`，`action` 支持 `start_all`/`stop_all`/`start_<idx>`/`stop_<idx>`，`secret` 匹配 `auth_secret` 才执行；仅 `allow_web_control=true` 的服务可操作。
- **Web 安全策略**：请求头+体超限 4KB 丢弃；读取超时 5 秒防慢读；同 IP 密码错误 5 次锁 5 分钟；错误密钥固定延迟 1 秒响应；日志记录失败但不泄露密钥。
- **仪表板渲染**：GET `/` 渲染 [web/dashboard.html](web/dashboard.html) 模板（`{{ROWS}}` 替换）；缺模板回退内置 HTML；按钮仅对 `allow_web_control=true` 展示；状态来自 `ServiceManager::get_all_statuses`。
- **Telegraf 集成**：Web 新增 GET `/metrics` 透传本机 `127.0.0.1:9273/metrics`（需在 telegraf.conf 启用 `outputs.prometheus_client`）。典型服务配置：`exe_path = "D:\\Trusted\\.Apps\\apps\\Telegraf\\current\\telegraf.exe"`，`args = ["--config", "D:\\Trusted\\.Apps\\persist\\Telegraf\\telegraf.conf"]`，`auto_start=true`，`allow_web_control=false` 防误停。
- **日志规范**：所有诊断使用 [src/logger.rs](src/logger.rs) 的 `write_app_log`（全局互斥，附调用位置），输出 `logs/application.log`；子进程日志独立文件，目录自动创建。
- **邮件通知（已弃用）**：API 保留在 [src/email.rs](src/email.rs)；配置段兼容，默认 `smtp.enabled=false`。
- **工作目录与可执行路径**：`ServiceConfig.working_dir` 通过 `Command::current_dir` 设置，需保持以兼容绝对路径依赖；`exe_path` 缺省指向工作目录下 frpc.exe。
- **构建/运行**：标准 Cargo (`cargo build`/`cargo run`)；运行需确保 frpc.exe 或其它目标可执行在工作目录或由 `exe_path` 指定；日志相对进程工作目录生成。
- **已知限制**：Web 服务器 `accept()` 无优雅关停，改为非阻塞/可中断需谨慎；共享密钥明文存储，避免在日志或错误输出中打印。

如有不清晰或缺失的知识点，请告知以便补充完善。
