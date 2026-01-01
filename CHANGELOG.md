# Changelog

All notable changes to RemoteHelper will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.1] - 2026-01-01

### Changed

- **BREAKING**: Client private keys are now automatically loaded from `~/id_ed25519.json` instead of being hardcoded
- `keygen` tool now saves keys directly to `~/id_ed25519.json` in user home directory
- Login UI updated to clarify password is for the key file
- All client implementations (Tauri, Web, test tools) now use standardized key file location

### Improved

- Header UI redesign: "Powered by Hughsean" now displays as subtitle below main title
- Enhanced visual hierarchy in application header

### Migration Guide

If you have an existing `client_key.json`, migrate it to the new location:

**Windows:**

```powershell
Move-Item client_key.json $env:USERPROFILE\id_ed25519.json
```

**Linux/macOS:**

```bash
mv client_key.json ~/id_ed25519.json
```

Or generate a new keypair:

```bash
cargo run --bin keygen
```

## [2.0.0] - 2026-01-01

### Added

- 🎉 Initial public release of RemoteHelper v2.0
- End-to-end encrypted communication (X25519 + AES-256-GCM)
- Ed25519 public key authentication
- Real-time hardware monitoring (CPU, RAM, GPU, Network)
- Process management for external programs (frpc)
- Dual client support (Web + Desktop Tauri app)
- Automatic log rotation (10MB threshold)
- Configurable connection limits and timeouts
- Auto-pause monitoring when idle (10s)
- Graceful shutdown with 5s process termination timeout
- Dynamic service registration
- Challenge-response authentication with 30s expiry
- Nonce randomization to prevent replay attacks

### Technical Details

- Rust 2024 Edition
- Tokio async runtime (4 worker threads)
- Arc<RwLock<T>> for shared state
- AtomicUsize for lock-free counters
- Dioxus v0.7 for Web UI
- Tauri v2 for Desktop app
- Chart.js integration for data visualization
- WASM bindings for browser client

### Security

- X25519 Diffie-Hellman key exchange
- AES-256-GCM symmetric encryption
- Ed25519 digital signatures
- PBKDF2 key derivation for encrypted key storage
- 30-second challenge expiry
- Per-message unique nonces
- Connection limit DoS protection

### Documentation

- Comprehensive README.md
- AI-friendly copilot-instructions.md
- MIT License
- Configuration examples in config.toml

### Code Quality

- Passes `cargo clippy --workspace -- -D warnings`
- Formatted with `cargo fmt --all`
- All tests passing
- Zero compilation warnings

---

## Future Enhancements

The following features are planned for future releases:

- Persistent dynamic service storage
- TLS certificate support
- Per-client rate limiting
- Prometheus metrics endpoint
- Multi-GPU monitoring
- Cross-platform support (Linux/macOS)
- WebSocket transport option
- Service dependency management
- Scheduled task support (cron-like)

---

[2.0.1]: https://github.com/Hughsean/RemoteHelper/releases/tag/v2.0.1
[2.0.0]: https://github.com/Hughsean/RemoteHelper/releases/tag/v2.0.0
