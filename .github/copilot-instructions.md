# Copilot Instructions for FrpcStartup

## Project Overview
FrpcStartup is a Rust-based utility service designed to manage multiple `frpc` (Fast Reverse Proxy Client) processes. It runs as a startup application, ensuring configured tunnels are active, and provides a lightweight web interface for remote management.

## Architecture & Core Components

### 1. Service Management (`src/frpc.rs`)
- **`FrpcManager`**: The core struct that manages child processes.
- **Process Isolation**: Each `frpc` tunnel runs as a separate child process (`std::process::Command`).
- **State Tracking**: Keeps track of child process handles to allow starting/stopping individual tunnels.

### 2. Web Control Interface (`src/web.rs`)
- **Custom HTTP Server**: Implemented using raw `std::net::TcpListener` without external web frameworks (like Actix or Axum).
- **Security**:
  - **IP Lockout**: Tracks failed authentication attempts and temporarily bans IPs (`IpStatus`).
  - **Auth**: Simple secret-based authentication (`auth_secret` in config).
  - **Timeouts**: Implements read timeouts to prevent Slowloris attacks.
- **Tunneling**: The web interface itself is exposed via a dedicated `frpc` tunnel.

### 3. Configuration (`src/config.rs`)
- **Format**: TOML (`config.toml`).
- **Structure**:
  - `[frpc]`: Array of tunnel configurations.
  - `[web_control]`: Settings for the management interface.
  - `[smtp]`: **Deprecated** email notification settings.

### 4. Lifecycle (`src/main.rs`)
- **Startup Delay**: Intentionally waits 25 seconds (`thread::sleep`) to ensure network services are ready on the host machine.
- **Graceful Shutdown**: Uses `ctrlc` crate to handle termination signals and clean up child processes.

## Developer Workflows

### Building & Deployment
- **Script**: Use `deploy.ps1` for the complete build-and-deploy cycle.
  - Builds release binary (`cargo build --release`).
  - Copies artifacts to `E:\UserData\.Exe\StartUp`.
  - Manages config file updates (preserves existing configs).
- **Prerequisites**: `frpc.exe` must be present in the target directory or system PATH.

### Logging
- **Mechanism**: Custom logging via `crate::logger::write_app_log`.
- **Pattern**: Always log significant state changes (startup, shutdown, process spawn/death).

## Coding Conventions

- **Web Server**: Do **not** introduce heavy web frameworks. Maintain the raw TCP implementation for minimal footprint.
- **Error Handling**: Prefer logging errors via `write_app_log` over panicking.
- **Concurrency**: Use `Arc<Mutex<T>>` for shared state between the main loop and the web server thread.
- **External Commands**: When spawning `frpc`, ensure arguments are passed correctly as separate strings to `Command::arg()`.

## Critical Files
- `src/web.rs`: Contains the manual HTTP request parsing and security logic.
- `deploy.ps1`: The source of truth for deployment paths and file handling.
- `config.toml`: Defines the schema for runtime configuration.
