# 设计文档：Windows 主机服务控制器 (Host Service Controller)

| 项目 | 内容 |
| --- | --- |
| **版本** | v1.0 |
| **日期** | 2025-12-04 |
| **核心框架** | Rust + Axum + Tokio |
| **监控组件** | sysinfo + nvml-wrapper |

## 1. 项目概述 (Overview)

### 1.1 背景

本项目旨在构建一个轻量级的 Windows 后台驻留程序，用于管理特定的外部进程（如 `frpc` 隧道），并提供主机硬件状态监控。该程序不依赖 Windows 系统服务（SCM），而是作为一个独立的进程管理器，根据 `config.toml` 配置文件拉起、停止和监控子进程。

### 1.2 核心功能

1. **进程管理 (Process Control)**: 解析配置文件，作为父进程启动指定的 exe 文件（主要是 `frpc`），支持通过 API 随时启停子进程。
2. **硬件监控 (Hardware Monitor)**: 实时监控宿主机的 CPU 使用率、内存占用、GPU（NVIDIA）的使用率和温度。
3. **自托管面板隧道**: 程序启动时，自动根据配置拉起一条专门用于 Web 控制台访问的 `frpc` 隧道。
4. **HTTP API**: 基于 Axum 提供 RESTful API，用于前端面板交互。
5. **安全鉴权**: 基于配置文件中的 `auth_secret` 进行简单的 Header 鉴权。

---

## 2. 系统架构 (Architecture)

系统采用单体架构，运行在 Tokio 异步运行时之上。

```mermaid
graph TD
    User[用户/Web前端] -->|HTTP Request + Auth| AxumServer[Axum HTTP Server]
    
    AxumServer -->|Read| AppState[Global AppState (Arc/Mutex)]
    
    AppState -->|管理| ProcMgr[进程管理器 (Process Manager)]
    AppState -->|读取| SysMon[硬件监控器 (System Monitor)]
    
    ProcMgr -->|Spawn/Kill| Child1[子进程: FRPC (Remote Desktop)]
    ProcMgr -->|Spawn/Kill| Child2[子进程: FRPC (SSH)]
    ProcMgr -->|Auto Start| ChildSys[子进程: Web Panel Tunnel]
    
    SysMon -->|Refresh| SysInfo[crate: sysinfo (CPU/RAM)]
    SysMon -->|Query| NVML[crate: nvml-wrapper (GPU)]
    
    ConfigFile[config.toml] -->|Load at startup| AppState

```

---

## 3. 数据结构设计 (Data Structures)

### 3.1 配置映射 (Config Mapping)

对应 `config.toml` 的序列化结构。

### 3.2 运行时状态 (Runtime State)

全局共享状态，用于在 HTTP Handler 中访问进程句柄和硬件信息。


---

## 4. API 接口规范 (API Specification)

所有接口均需携带 Header: `Authorization: <auth_secret>`。

### 4.1 监控接口

#### `GET /api/status`

获取当前服务器硬件状态。

* **Response (200 OK):**
```json
{
  "cpu": {
    "usage_percent": 15.2,
    "cores": 16
  },
  "memory": {
    "total_mb": 32768,
    "used_mb": 8192,
    "usage_percent": 25.0
  },
  "gpu": [
    {
      "name": "NVIDIA GeForce RTX 4060",
      "usage_percent": 45,
      "memory_used_mb": 4096,
      "temperature_c": 62
    }
  ]
}

```



### 4.2 服务控制接口

#### `GET /api/services`

获取配置文件中定义的所有服务的当前运行状态。

* **Response (200 OK):**
```json
[
  {
    "id": 0,
    "description": "Remote Desktop Access",
    "running": true,
    "pid": 12345
  },
  {
    "id": 1,
    "description": "SSH Access",
    "running": false,
    "pid": null
  }
]

```



#### `POST /api/services/{id}/action`

控制指定 ID (配置列表索引) 服务的启停。

* **Path Param**: `id` (integer) - 对应 `config.services` 数组的下标。
* **Body**:
```json
{ "action": "start" } // 或 "stop", "restart"

```


* **Response**:
* `200 OK`: 操作成功。
* `400 Bad Request`: 服务已在运行或已停止。
* `404 Not Found`: ID 不存在。
* `500 Internal Error`: 启动进程失败。



---

## 5. 核心逻辑详解 (Implementation Details)

### 5.1 进程启动逻辑 (Process Spawning)

使用 `tokio::process::Command` 避免阻塞主线程。

**关键点**:

1. **路径处理**: Windows 下路径可能包含空格，Rust 的 `Command` 会自动处理参数转义，但需确保 `exe_path` 是绝对路径或在 PATH 中。
2. **Kill on Drop**: 必须确保当主控程序崩溃或关闭时，子进程也会被关闭，防止僵尸进程。虽然 `Command` 有 `kill_on_drop`，但要在 `App` 层实现 Graceful Shutdown 逻辑。


### 5.2 硬件监控逻辑

* **CPU**: `sysinfo` 需要两次测量之间有时间间隔。
* *策略*: 每次 API 请求时，先调用 `sys.refresh_cpu()`。注意，如果请求非常频繁，计算出的 Usage 可能为 0 或不准。建议在 `main` 函数启动一个后台 Tokio Task，每 2 秒刷新一次 `sys`，API 只负责读取。


* **GPU**: 使用 `nvml-wrapper`。
* *初始化*: 在 `main` 中初始化一次 `Nvml`，存入 `AppState`。如果初始化失败（如无显卡），记录日志但不要 panic，相关 API 返回空数据。



### 5.3 Web 面板自启 (Self-Hosting Tunnel)

在 `main` 函数启动 Axum 服务之前：

1. 检查 `config.web_panel.enabled`。
2. 若为 true，读取 `frpc_exe_path` 和 `frpc_arg`。
3. 启动该进程，并将句柄存入 `state.web_tunnel_process`。
4. 该进程不受 `/api/services` 接口控制，属于系统级进程。

---

## 6. 异常处理与日志 (Error & Logging)

1. **日志**: 使用 `tracing` 和 `tracing-subscriber`。
* 记录所有子进程的 stdout/stderr（可选，防止日志过大）。
* 记录 API 的访问日志。


2. **错误定义**:




---

## 7. 开发路线图 (Roadmap)

1. **Phase 1: 基础骨架**
* 实现 TOML 配置读取。
* 搭建 Axum Hello World。
* 实现 `AppState` 结构。


2. **Phase 2: 监控集成**
* 集成 `sysinfo`，实现 `/api/status` (CPU/RAM)。
* 集成 `nvml-wrapper`，补充 GPU 数据。


3. **Phase 3: 进程管理核心**
* 实现 `tokio::process` 的启动与停止逻辑。
* 实现 `/api/services` 列表与控制接口。
* 集成 Web 面板自启逻辑。


4. **Phase 4: 安全与优化**
* 添加 Auth 中间件。
* 添加 Graceful Shutdown (Ctrl+C 信号捕获，清理所有子进程)。
* 编译 Release 版本测试。

