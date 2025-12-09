# FrpcStartup - AI Coding Agent Instructions

## Project Overview
Rust application that manages multiple frpc (fast reverse proxy client) processes with email notifications. Designed to run on Windows as a startup service, monitoring frpc tunnels and alerting via SMTP when processes start or encounter errors.

## Architecture

### Core Components
- **main.rs**: Single-file architecture with three main functions:
  - `run_single_frpc()`: Spawns individual frpc processes with dedicated logging
  - `send_email()`: HTML email notifications using lettre with template rendering
  - `main()`: Thread pool manager spawning one thread per frpc configuration entry

### Data Flow
1. Load `config.toml` (SMTP credentials + array of frpc configs with description/arg pairs)
2. Spawn thread per frpc entry → each runs `frpc.exe -f <arg>` with redirected stdout/stderr
3. Send success email immediately after spawn
4. Monitor process with try_wait() loop checking for exit or Ctrl+C shutdown signal
5. On error: send alert email with last 2KB of log content

## Configuration Pattern

**config.toml** structure:
```toml
[smtp]
server = "smtp.qq.com"
username = "user@domain.com"
password = "app-password"
from = "user@domain.com"
to = "recipient@domain.com"

[[frpc]]
description = "Remote Desktop Access"
arg = "token:port"
```

- Each `[[frpc]]` entry spawns separate thread/process
- `arg` passed directly to `frpc.exe -f <arg>` (frpc protocol-specific format)

## Logging Strategy

- **Application logs**: `logs/application.log` - startup/shutdown/config events
- **Process logs**: `logs/frpc_{index}_{timestamp}.log` - individual frpc stdout/stderr
- Logs created per-process with datetime stamp for debugging tunnel issues
- Error emails include last 2048 bytes from process log

## Email Template System

`email_template.html` uses token replacement for dynamic content:
- `{{COLOR}}`, `{{BG_COLOR}}`, `{{TEXT_COLOR}}`: Conditional styling (red for errors, green for success)
- `{{SUBJECT}}`, `{{INTRO}}`, `{{CONTENT}}`: Message content
- Template is read from disk on each send (allows hot-reload without recompile)

## Development Workflows

### Build & Deploy
```powershell
.\deploy.ps1  # Compiles release build → copies to E:\UserData\.Exe\StartUp
```
- Script preserves existing `config.toml` at target (avoids overwriting credentials)
- Expects `frpc.exe` in target directory or system PATH

### Testing
- No automated tests (manual testing workflow)
- Test by running with sample frpc args and verifying email delivery
- Check `logs/application.log` for startup sequence

### Local Development
```powershell
cargo build --release
.\target\release\FrpcStartup.exe
```

## Project-Specific Conventions

1. **No error recovery**: Process errors trigger email and thread termination (fail-fast for monitoring)
2. **Blocking email sends**: Email delivery happens synchronously in each thread (acceptable for low-frequency notifications)
3. **Chinese UI strings**: User-facing messages (emails, logs) use Chinese (target audience consideration)
4. **Windows-specific paths**: Hardcoded `E:\UserData\.Exe` in deploy.ps1 reflects single-user deployment model
5. **Shutdown handling**: ctrlc crate for graceful termination with Arc<AtomicBool> shared across threads

## Dependencies Note

- **lettre**: SMTP relay mode (not direct connection) - note `SmtpTransport::relay()` usage
- **chrono**: Log timestamps in `%Y-%m-%d %H:%M:%S` format
- **ctrlc**: System signal handling for Windows service compatibility

## Key Files
- `src/main.rs`: All application logic (309 lines)
- `config.toml`: Runtime configuration (credentials + tunnel definitions)
- `email_template.html`: HTML email styling with token placeholders
- `deploy.ps1`: Deployment automation for Windows target environment
