# FrpcStartup 项目内协作指南（面向 AI 代理）

- **项目目的与入口**：Windows 常驻辅助程序，用于自启动/管理多个进程（主要是 frpc.exe），可选内置 Web 控制台。主入口在 [src/main.rs](src/main.rs)。
- **配置契约**：运行时配置读取 [config.toml](config.toml)，解析逻辑在 [src/config.rs](src/config.rs)。`[[service]]` 默认补全：`auto_start=false`、`exe_path="frpc.exe"`、`args` 优先且由旧字段 `arg` 推导、`allow_web_control=true`；请保持向后兼容，新增字段时注意默认值与旧字段兼容。
- **ServiceManager 机制**：核心状态位于 [src/frpc.rs](src/frpc.rs)，`processes: Vec<Option<Child>>` 顺序与 `config.service` 一一对应。必须通过 `start_service`/`stop_service` 统一管理，确保日志与进程句柄一致；健康检查与未来重启逻辑集中在 `check_health`。每次启动为该服务生成日志文件 `logs/service_<idx>_timestamp.log`，stdout/stderr 均重定向其中。
- **启动流程**：主进程启动时先写分隔日志并等待 25 秒确保网络/DNS 就绪，再加载配置。如果 `web_control.enabled=false` 或配置缺失，则直接 `start_all()`；否则仅启动 Web 控制台，业务服务由 Web 触发。主循环每 5 秒做健康检查，监听 Ctrl+C 置 `shutdown`，若 Web 线程退出则整体退出。
- **Web 控制台协议**：实现于 [src/web.rs](src/web.rs)，自建 TCP HTTP 服务器并先拉起隧道进程 `frpc.exe -f <frpc_arg>` 监听 `0.0.0.0:<local_port>`。接口：POST `/api/action`，`action` 支持 `start_all`/`stop_all`/`start_<idx>`/`stop_<idx>`，`secret` 必须匹配 `auth_secret`。仅 `allow_web_control=true` 的服务可被操作，其他服务直接拒绝。
- **Web 安全策略**：请求头+体超限 4KB 即丢弃；读取超时 5 秒防慢读；同 IP 密码错误累计 5 次锁定 5 分钟；错误密钥固定延迟 1 秒响应以缓解时序侧信道。日志会记录失败尝试但不要泄露密钥。
- **仪表板渲染**：GET `/` 读取模板 [web/dashboard.html](web/dashboard.html) 并替换 `{{ROWS}}`，缺模板时回退内置 HTML。按钮仅对 `allow_web_control=true` 展示；状态显示依赖 `ServiceManager::get_all_statuses`。
- **日志规范**：所有诊断使用 [src/logger.rs](src/logger.rs) 的 `write_app_log`，内部带全局互斥并附调用位置，输出到 `logs/application.log`。服务子进程日志单独文件，目录会自动创建。
- **邮件通知（已弃用）**：API 仍在 [src/email.rs](src/email.rs)，配置段保留兼容（[email_template.html](email_template.html)）。如非大版本请勿移除接口，但默认 `smtp.enabled=false`。
- **工作目录与可执行路径**：`ServiceConfig.working_dir` 通过 `Command::current_dir` 设置，保留以兼容依赖绝对路径的现有配置；`exe_path` 缺省指向工作目录下 frpc.exe。
- **构建/运行**：标准 Cargo 流程（如 `cargo build`、`cargo run`）。运行时需确保 frpc.exe 在当前工作目录或由 `exe_path` 指定；日志路径相对进程工作目录。
- **已知限制与注意**：Web 服务器 `accept()` 无优雅关停，修改为非阻塞/可中断需谨慎；共享密钥为明文存储，避免在日志或错误输出中打印。

如有不清晰或缺失的知识点，请告知以便补充完善。
