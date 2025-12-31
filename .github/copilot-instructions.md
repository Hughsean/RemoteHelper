# RemoteHelper AI Instructions

## Project Overview

RemoteHelper is a Windows-focused system utility monorepo consisting of a background service controller and multiple client implementations.

- **Server (`RemoteHelperServer`)**: A standalone Rust binary that manages child processes (e.g., `frpc`), monitors hardware (CPU/RAM/GPU), and exposes a TCP control server with encrypted communication.
- **Client Library (`client`)**: A reusable Rust library providing TCP client functionality with authentication.
- **Tauri App (`app/src-tauri`)**: A Tauri v2 desktop application with System Tray interface.
- **Dioxus Web App (`app`)**: A WebAssembly-based web interface using Dioxus framework.
- **Common (`common`)**: A shared Rust library defining the communication protocol, cryptography, and data structures.
- **HwLib (`hwlib`)**: Hardware abstraction library (placeholder, currently unused).

## Architecture & Data Flow

### Communication Protocol

- **Transport**: Custom TCP protocol over encrypted channel (X25519 ECDH + AES-256-GCM).
- **Message Format**: JSON-serialized `Request`/`Response` enums with length-prefixed framing.
- **Security Layers**:
  1. **Handshake**: X25519 key exchange to establish shared secret.
  2. **Encryption**: AES-256-GCM with unique nonce per message (random initial nonce to prevent reuse).
  3. **Authentication**: Ed25519 challenge-response with 30-second challenge expiry.

### Server Model

- **Runtime**: Multi-threaded Tokio (`flavor = "multi_thread"`, 4 worker threads).
- **Concurrency Safety**:
  - `Arc<RwLock<T>>` for shared mutable state (hardware stats, process list).
  - `AtomicUsize` for connection counter and service ID generation.
  - Optimized lock strategy: read lock for checks, write lock only when mutating.
- **Background Tasks**:
  - **Hardware Monitor**: Refreshes CPU/RAM/GPU stats based on client-requested interval (default 1s).
  - **Auto-pause**: Stops monitoring if no reads for 10 seconds to save resources.
  - **Process Management**: Tracks child process lifecycle with proper cleanup.
- **Resource Limits**:
  - Max connections: Configurable (default 100).
  - Connection timeout: Configurable (default 300s).
  - Challenge timeout: 30 seconds hardcoded.
- **Graceful Shutdown**: 5-second timeout for child process termination, then force kill.

### Client Models

- **Tauri App**:
  - System Tray only (no main window by default).
  - Cross-platform activation policy handling (macOS Accessory mode).
  - Single instance enforcement.
- **Dioxus Web App**:
  - WASM-based, runs in browser.
  - Uses `client` library via WASM bindings.
  - Real-time status updates with Chart.js visualization.

## Key Files & Directories

### Server

- `src/main.rs` - Entry point: config loading, state init, monitoring loop, TCP listener.
- `src/server.rs` - Connection handling, handshake, authentication, request routing.
- `src/state.rs` - `AppState` definition with all shared state and atomic counters.
- `src/process.rs` - Child process management, service start/stop/restart, log rotation.
- `src/config.rs` - `AppConfig` deserialization with defaults.
- `config.toml` - Runtime configuration (ports, keys, service definitions).

### Client Library

- `client/src/lib.rs` - TCP client with automatic reconnection and session management.

### Common

- `common/src/lib.rs` - Protocol enums (`Request`, `Response`, `Handshake`).
- `common/src/crypto.rs` - X25519/AES-256-GCM encryption session.
- `common/src/func.rs` - Shared utilities (nanoid generation).

### Tauri App

- `app/src-tauri/src/lib.rs` - Tauri setup, tray menu, window management.
- `app/src-tauri/src/command.rs` - Tauri commands for frontend IPC.

### Dioxus App

- `app/src/app.rs` - Main Dioxus component with routing.
- `app/src/components/` - UI components (login, status, services, charts).
- `app/src/command/mod.rs` - WASM bindings to client library.

### Documentation

- `README.md` - User documentation, quick start, features overview.
- `CONTRIBUTING.md` - Contribution guidelines, code standards, workflow.
- `CHANGELOG.md` - Version history and change logs.
- `LICENSE` - MIT License file.
- `config.example.toml` - Example configuration with detailed comments.
- `.github/copilot-instructions.md` - Developer guide and architecture (this file).

### CI/CD

- `.github/workflows/ci.yml` - Continuous integration (check, test, build).
- `.github/workflows/release.yml` - Automated release workflow.
- `.editorconfig` - Editor configuration for consistent code style.
- `.vscode/` - VS Code workspace settings and extensions.

## Configuration Schema

```toml
[web_panel]
enabled = true
local_port = 9999
frpc_exe_path = "path/to/frpc.exe"
frpc_args = ["-f", "token"]
authorized_keys = ["base64_ed25519_public_key"]
startup_delay_secs = 0          # Optional: delay before starting (default: 0)
max_connections = 100           # Optional: max concurrent connections (default: 100)
connection_timeout_secs = 300   # Optional: per-connection timeout (default: 300)

[[service]]
description = "Service Name"
exe_path = "path/to/exe"
args = ["arg1", "arg2"]
auto_start = false
allow_web_control = false
```

## Developer Workflows

### Workspace Structure

Cargo workspace with members: `common`, `client`, `app`, `app/src-tauri`, `hwlib`.

### Running Server

```bash
cargo run --bin RemoteHelperServer
# Or just: cargo run
```

### Running Tauri App

```bash
cd app
dx serve  # Dioxus web dev server
# In separate terminal:
cd app
cargo tauri dev  # Tauri desktop app
```

### Building

- **Server**: `cargo build --release` → `target/release/RemoteHelperServer.exe`
- **Tauri App**: `cd app && cargo tauri build`
- **Web App**: `cd app && dx build --release`

### Utility Binaries

- `cargo run --bin keygen` - Generate Ed25519 keypair and encrypt private key.
- `cargo run --bin gen_cert` - Generate self-signed TLS certificate (currently unused).
- `cargo run --bin test_real_ip` - Test client authentication flow.

## Conventions & Patterns

### Protocol Changes

When modifying `Request` or `Response` in `common/src/lib.rs`:

1. Update `src/server.rs` → `process_authenticated_request()`.
2. Update `client/src/lib.rs` → `send_request()` usage.
3. Update Tauri commands in `app/src-tauri/src/command.rs`.
4. Update Dioxus WASM bindings in `app/src/command/mod.rs`.

### Error Handling

- **Server**: Use `anyhow::Result` for fallible operations. Log errors with `tracing`.
- **Client Library**: Return `Result<T, String>` for Tauri/WASM compatibility.
- **Tauri Commands**: Return `Result<T, String>` (Tauri requirement).
- **Avoid Panics**: Use `?` or `.context()` instead of `.unwrap()` in production code.

### State Management

- **Server**:
  - `Arc<AppState>` cloned into each connection handler.
  - `RwLock` for read-heavy shared state.
  - `AtomicUsize` for counters (no lock needed).
- **Client**:
  - Global `LazyLock<Mutex<Option<T>>>` for signing key and connection.
  - Automatic reconnection on network errors.

### Concurrency Best Practices

- **Lock Ordering**: Always acquire locks in the same order to prevent deadlocks.
- **Minimize Lock Scope**: Drop locks before `.await` points when possible.
- **Read-then-Write Pattern**: Check with read lock, then acquire write lock if needed (see `is_service_running`).
- **Atomic Operations**: Use `AtomicUsize::fetch_add` for thread-safe counters.

### Security Considerations

- **Challenge Expiry**: Challenges expire after 30 seconds to prevent replay attacks.
- **Nonce Randomization**: Initial nonce is randomized to prevent reuse on reconnection.
- **Key Authorization**: Only pre-configured public keys in `authorized_keys` can authenticate.
- **Connection Limits**: Prevents resource exhaustion DoS attacks.
- **Sensitive Data**: Keep private keys encrypted at rest (use `keygen` tool).

### Hardware Monitoring

- **GPU Support**: Uses `nvml-wrapper` (NVIDIA only). Gracefully handles initialization failure.
- **Adaptive Refresh**: Client controls refresh interval via `GetStatus { interval_ms }`.
- **Auto-pause**: Monitoring stops if no client reads for 10 seconds.
- **First CPU Core**: CPU model string taken from first core (`sys.cpus().first()`).

### Logging

- **Server**: `tracing` with dual output (stdout + file in `logs/server.log`).
- **Client**: Uses Tauri's logging or browser console (Dioxus).
- **Service Logs**: Each service/tunnel logs to `logs/service_{id}.log` or `logs/web_tunnel.log`.
- **Log Rotation**: Automatic rotation when log files exceed 10MB.

### Process Management

- **Service IDs**: Static services (from config) have IDs 0..N-1, dynamic services start at N.
- **PID Tracking**: Store `Child` handles in `HashMap<usize, Child>`.
- **Cleanup**: On shutdown, send SIGTERM (start_kill), wait 5s, then SIGKILL if needed.
- **Status Check**: Use non-blocking `try_wait()` to avoid holding locks.

## Tech Stack

### Server

- **Rust 2024 Edition**
- **Tokio**: Async runtime (multi_thread, 4 workers).
- **Serde/serde_json**: Serialization.
- **Sysinfo**: CPU/RAM/Network monitoring.
- **nvml-wrapper**: GPU monitoring (NVIDIA).
- **Tracing/tracing-subscriber**: Structured logging.
- **ed25519-dalek**: Signature verification.
- **x25519-dalek**: Key exchange.
- **aes-gcm**: Symmetric encryption.

### Client

- **Tokio**: Async I/O (full feature set).
- **Same crypto stack**: ed25519, x25519, aes-gcm.

### Tauri App

- **Tauri v2**: Desktop app framework.
- **Dioxus**: Reactive UI framework (web target).
- **TailwindCSS**: Utility-first CSS.
- **Chart.js**: Data visualization (via WASM bindings).

### Common

- **nanoid**: Short unique ID generation (4 chars).
- **Base64**: Encoding for keys/signatures.

## Troubleshooting

### Server Won't Start

- Check `config.toml` syntax (must be valid TOML).
- Ensure port 9999 (or configured port) is not in use.
- Verify `frpc_exe_path` exists if `web_panel.enabled = true`.

### Authentication Fails

- Confirm public key in `authorized_keys` matches the one from `keygen`.
- Check challenge hasn't expired (30s limit).
- Verify keypair file is readable by client.

### GPU Monitoring Shows None

- NVIDIA drivers must be installed.
- `nvml-wrapper` initialization may fail on non-NVIDIA systems (expected).
- Server logs will show NVML init status.

### Connection Limit Reached

- Increase `max_connections` in config.
- Check for connection leaks (clients not closing).
- Consider implementing connection pooling on client side.

## Code Quality Standards

### Clippy Compliance

All code must pass `cargo clippy --workspace -- -D warnings` without warnings. Common patterns to avoid:

- Unnecessary borrows (`&value` when `value` already implements trait)
- Redundant closures (`|| String::new()` → `String::new`)
- Nested if statements (use `if let ... && let ...` for Rust 2024)
- `map().flatten()` chains (use `and_then()` instead)
- Unnecessary `return` statements
- `match` with single pattern (use `if let` instead)

### Formatting

- All code must be formatted with `cargo fmt --all`
- Use 4-space indentation
- Maximum line length: 100 characters (soft limit)
- Follow Rust 2024 edition idioms

### Testing

- Unit tests for all public APIs
- Integration tests for protocol changes
- Test coverage for error paths
- Mock external dependencies (NVML, network)

### Documentation

- Public items must have doc comments (`///`)
- Complex algorithms need inline comments
- Protocol changes documented in CHANGELOG.md
- Update README.md for user-facing changes

### Performance Considerations

- Minimize lock contention (read locks before write locks)
- Use `AtomicUsize` for simple counters
- Drop locks before `.await` points
- Profile with `tokio-console` for async bottlenecks
- Benchmark critical paths with `criterion`

## Future Enhancements

- Persistent dynamic service storage (currently lost on restart).
- TLS certificate support (infrastructure exists, not wired up).
- Rate limiting per client (currently global connection limit only).
- Metrics endpoint (Prometheus-style).
- Multi-GPU support (currently only device 0).
- Cross-platform support (Linux/macOS).
- WebSocket transport option for Web clients.
- Service dependency management.
- Scheduled task support (cron-like).
