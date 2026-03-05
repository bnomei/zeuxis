# zeuxis

[![Crates.io Version](https://img.shields.io/crates/v/zeuxis)](https://crates.io/crates/zeuxis)
[![CI](https://img.shields.io/github/actions/workflow/status/bnomei/zeuxis/ci.yml?branch=main)](https://github.com/bnomei/zeuxis/actions/workflows/ci.yml)
[![Crates.io Downloads](https://img.shields.io/crates/d/zeuxis)](https://crates.io/crates/zeuxis)
[![License](https://img.shields.io/crates/l/zeuxis)](https://crates.io/crates/zeuxis)
[![Discord](https://flat.badgen.net/badge/discord/bnomei?color=7289da&icon=discord&label)](https://discordapp.com/users/bnomei)
[![Buymecoffee](https://flat.badgen.net/badge/icon/donate?icon=buymeacoffee&color=FF813F&label)](https://www.buymeacoffee.com/bnomei)

Zeuxis is a local, read-only MCP screenshot server that lets AI agents capture screenshots themselves.

When your MCP client connects to Zeuxis, the agent can call screenshot tools directly (full screen, active window, cursor region, or exact rect), then immediately use the returned `file://` image link and metadata in its next reasoning step. No manual screenshot/upload handoff is required.

Zeuxis is:
- CLI-first: one local binary, stdio MCP transport by default.
- MCP-first: explicit tool schemas and stable error codes.
- Safety-first: strict input validation, bounded capture concurrency, and temp-file retention limits.

Supported platforms in v1:
- macOS (first-class)
- Linux (best-effort; backend/compositor dependent)

## Installation

### Cargo (crates.io)
```bash
cargo install zeuxis
```

### Homebrew
```bash
brew install bnomei/zeuxis/zeuxis
```

### GitHub Releases
Download a prebuilt archive from GitHub Releases, extract it, and place `zeuxis` on your `PATH`.

### From source
```bash
git clone https://github.com/bnomei/zeuxis.git
cd zeuxis
cargo build --release
```

## Quickstart

### Run MCP over stdio
```bash
zeuxis
# or
cargo run --quiet
```

### Show CLI options
```bash
zeuxis --help
zeuxis --version
```

### MCP client configuration example
```json
{
  "mcpServers": {
    "zeuxis": {
      "command": "zeuxis",
      "args": []
    }
  }
}
```

### Add Zeuxis via CLI commands

If you use Codex CLI:
```bash
codex mcp add zeuxis -- zeuxis
codex mcp list
```

If you use Amp CLI:
```bash
amp mcp add zeuxis -- zeuxis
amp mcp list
```

## MCP Tools

Shared params for `capture_*` tools:
- `delay_ms` or `delay_seconds` (use one), `play_sound`, and optional `output` settings.
- `output` accepts either a preset string (`"analysis"|"exact"|"compact"`) or an object (`{ "mode":"preset|custom", ... }`).

| Tool | Params | Description |
| --- | --- | --- |
| `list_monitors` | none | Lists available monitors and IDs. Use IDs with monitor-specific capture tools. |
| `list_windows` | `focused_only?`, `include_system_windows?`, `app_contains?`, `title_contains?` | Lists windows with deterministic IDs plus snapshot metadata. Use its `snapshot_id` + `window_id` with `capture_window`. |
| `get_runtime_diagnostics` | none | Reports runtime capture readiness (permission, monitor discovery, cursor availability). Useful as a first troubleshooting step. |
| `get_latest_capture` | none | Returns the latest artifact captured in the current Zeuxis session without taking a new screenshot. |
| `list_session_artifacts` | none | Lists all artifacts created in the current Zeuxis session, including `is_latest`. |
| `clear_session_artifacts` | none | Deletes artifacts from the current Zeuxis session and resets latest-capture state. |
| `capture_screen` | `monitor_id?` + shared capture params | Captures a full monitor (primary monitor if `monitor_id` is omitted). Good default when you need overall context. |
| `capture_active_window` | shared capture params | Captures the currently focused window. |
| `capture_cursor_window` | `include_system_windows?` + shared capture params | Captures the window under the cursor. By default, system/menu surfaces are excluded. |
| `capture_window` | `snapshot_id`, `window_id` + shared capture params | Captures a specific window deterministically from a `list_windows` snapshot. |
| `capture_cursor_region` | `size` + shared capture params | Captures a square region centered on the cursor. |
| `capture_rect` | `x`, `y`, `width`, `height` + shared capture params | Captures an exact global desktop rectangle (logical points). |
| `capture_monitor_region` | `monitor_id`, `x`, `y`, `width`, `height` + shared capture params | Captures a monitor-local rectangle (logical points). Best for multi-monitor workflows. |

Notes:
- Coordinate inputs are logical desktop points; resulting image dimensions are source pixels.
- Capture results include local `file://` artifact links plus metadata like `capture_mode`, `artifact_capture_mode`, dimensions, hashes, and `source_scale_factor`.
- Errors are structured with `error_code`, `message`, and `retryable`.

## Runtime Safety Limits

- `delay_ms` max: `30000`
- `delay_seconds` max: `30`
- capture dimension max: `16384 x 16384`
- capture area max: `40,000,000` pixels
- capture work runs in a dedicated subprocess worker and is gated by a semaphore
- capture timeout is enforced in the parent process; timed-out workers are terminated and reaped before returning
- capture timeout: configurable (`100..=300000` ms, default `15000` ms)

Temp artifact retention:
- managed files use prefix `zeuxis-` and suffix `.png`, `.jpg`, or `.webp`
- older artifacts are pruned on each successful write
- pruning is best effort and never deletes the current artifact being returned

## Configuration

Precedence is `CLI flag > env var > default` (no config files).

| CLI Flag | Env Var | Default | Meaning |
| --- | --- | --- | --- |
| `--max-concurrent-captures` | `ZEUXIS_MAX_CONCURRENT_CAPTURES` | `2` | Max parallel capture workers (`1..=16`). |
| `--max-artifacts` | `ZEUXIS_MAX_ARTIFACTS` | `64` | Max retained Zeuxis temp image files (`1..=10000`). |
| `--max-artifact-bytes` | `ZEUXIS_MAX_ARTIFACT_BYTES` | `536870912` | Max retained Zeuxis temp image bytes (`1024..=10737418240`). |
| `--artifact-dir` | `ZEUXIS_ARTIFACT_DIR` | system temp dir | Directory for managed capture artifacts. |
| `--blocking-task-timeout-ms` | `ZEUXIS_BLOCKING_TASK_TIMEOUT_MS` | `15000` | Overall capture deadline before timeout/worker termination (`100..=300000`). |
| `--worker-kill-grace-ms` | `ZEUXIS_WORKER_KILL_GRACE_MS` | `250` | Grace period to wait after soft terminate before hard-kill (`10..=30000`). |
| `--max-worker-stdout-bytes` | `ZEUXIS_MAX_WORKER_STDOUT_BYTES` | `65536` | Max worker IPC stdout bytes accepted by parent (`1024..=4194304`). |
| `--capture-sound-file` | `ZEUXIS_CAPTURE_SOUND_FILE` | platform default | Optional custom sound file for capture-complete feedback when `play_sound=true`. |
| n/a | `ZEUXIS_ARTIFACT_HMAC_KEY` | unset | Optional HMAC key; when set, `artifact_hmac_sha256` is included in capture results. |
| n/a | `RUST_LOG` | unset | Standard Rust logging filter for runtime diagnostics. |

## macOS Permissions

On macOS, Zeuxis preflights Screen Recording permission before each capture.

If permission is missing, Zeuxis requests access via CoreGraphics and returns `permission_denied` for that same invocation (no same-call retry). After granting access, call the tool again.

Cursor-dependent tools may also require Accessibility permission because they read global cursor position.

## Linux Notes

On Linux, Zeuxis does not run a dedicated permission preflight gate in v1. Capture behavior depends on your desktop environment/compositor and what the underlying capture backend permits.

If cursor-dependent tools fail (especially on Wayland), run `get_runtime_diagnostics` first and fall back to `capture_screen` or `capture_rect` as needed.

## Development

```bash
cargo check
cargo fmt --all --check
cargo clippy --all-targets --all-features
cargo test --all-targets
cargo audit
```

See:
- [operations](docs/operations.md)
- [homebrew notes](docs/homebrew.md)
- [overview](docs/overview.md)
- [specs](specs/mcp-screenshot-server)
