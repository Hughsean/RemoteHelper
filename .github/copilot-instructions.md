# RemoteHelper AI Instructions

## Project Overview
RemoteHelper is a Windows-focused system utility monorepo consisting of a background service controller and a Tauri-based GUI client.
- **Server (`RemoteHelperServer`)**: A standalone Rust binary that manages child processes (e.g., `frpc`), monitors hardware (CPU/RAM/GPU), and exposes a TCP control server.
- **Client (`RemoteHelper`)**: A Tauri v2 application providing a GUI to interact with the server.
- **Common (`common`)**: A shared Rust library defining the communication protocol and data structures.

## Architecture & Data Flow
- **Communication**: The Client and Server communicate via a custom TCP protocol using JSON-serialized messages defined in `common::Request` and `common::Response`.
- **Security**: Authentication uses a challenge-response mechanism with Ed25519 signatures (`ed25519-dalek`).
- **Server Model**: 
  - Single-threaded Tokio runtime (`flavor = "current_thread"`).
  - Background tasks for hardware monitoring (`sysinfo`, `nvml-wrapper`) and process management.
  - `config.toml` drives service configuration and auto-start behavior.
- **Client Model**:
  - Tauri v2 with a System Tray interface.
  - Frontend: Vite + TypeScript + TailwindCSS (Vanilla/No-Framework).
  - Backend: Rust (`client/src-tauri`) handles IPC and TCP communication with the Server.

## Key Files & Directories
- **Server Entry**: `src/main.rs` - Initializes config, state, monitoring loop, and TCP listener.
- **Client Entry**: `client/src-tauri/src/lib.rs` - Tauri setup, tray menu, and command handlers.
- **Protocol**: `common/src/lib.rs` - `Request`/`Response` enums and shared structs (`StatusData`, `ServiceData`).
- **Config**: `config.toml` (Server) and `client/src-tauri/tauri.conf.json` (Client).

## Developer Workflows
- **Workspace**: This is a Cargo workspace. `Cargo.toml` in the root defines members `common` and `client/src-tauri`.
- **Running Server**:
  ```bash
  cargo run
  ```
- **Running Client**:
  ```bash
  cd client
  npm install # or pnpm
  npm run tauri dev
  ```
- **Building**:
  - Server: `cargo build --release`
  - Client: `cd client && npm run tauri build`

## Conventions & Patterns
- **Protocol Changes**: When modifying `Request` or `Response` in `common`, you MUST update both the Server (`src/server.rs`) and Client (`client/src-tauri/src/client.rs`) handlers.
- **Error Handling**: Use `anyhow::Result` for top-level server logic. Client commands return `Result<T, String>` for Tauri compatibility.
- **State Management**: 
  - Server uses `Arc<AppState>` with `RwLock` for shared mutable state (processes, hardware stats).
  - Client uses Tauri's managed state for connection handling.
- **Hardware Monitoring**: GPU monitoring relies on `nvml-wrapper`. Ensure NvOptimus/CUDA environment is considered if testing on non-NVIDIA hardware (graceful fallback required).

## Tech Stack
- **Rust**: Tokio, Serde, Sysinfo, Nvml-wrapper, Tracing.
- **Tauri**: v2, tauri-plugin-log.
- **Frontend**: TypeScript, TailwindCSS, Chart.js.
