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

## MCP Tools

Zeuxis exposes:
- `list_monitors`: discover monitors and ids.
- `diagnose_runtime`: report capture readiness, cursor status, and session context.
- `get_latest_screenshot`: return the latest artifact from this server session without taking a new capture.
- `capture_screen`: full-monitor capture (best default when user says “show me what you see”).
- `capture_active_window`: focused app window only.
- `capture_window_at_cursor`: window under cursor.
- `capture_cursor_region`: square around cursor.
- `capture_rect`: exact global rectangle.

Monitor-aware capture:
- `capture_screen` accepts optional `monitor_id` (from `list_monitors`).
- If `monitor_id` is omitted, `capture_screen` uses the primary monitor.

Output presets for capture tools (`capture_*`):
- default `output_preset: "analysis"` => `png` + `max_dimension=2560`
- `output_preset: "exact"` => `png` + no resize
- `output_preset: "compact"` => `jpeg` + `jpeg_quality=82` + `max_dimension=1600`

Optional expert overrides:
- `output_format`: `png | jpeg | webp`
- `jpeg_quality`: `40..95` (only when resolved output format is `jpeg`)
- `max_dimension`: `256..8192` (long-edge cap, aspect ratio preserved)

Override precedence:
- `output_format`/`jpeg_quality`/`max_dimension` override preset defaults when provided.

Successful tool results include:
- `content` text summary
- `content` `resource_link` to local `file://` image (`png`, `jpeg`, or `webp`)
- `structuredContent` with `path`, `uri`, `output_format`, `mime_type`, `artifact_sha256`, `artifact_hmac_sha256`, `width`, `height`, `capture_mode`, `captured_at_utc` (original capture timestamp)

Error results use `isError=true` with structured fields:
- `error_code`
- `message`
- `retryable`

`get_latest_screenshot` returns `no_capture_yet` until at least one successful `capture_*` call has occurred in the current Zeuxis process.

## Runtime Safety Limits

- `delay_seconds` max: `30`
- capture dimension max: `16384 x 16384`
- capture area max: `40,000,000` pixels
- capture work runs on blocking workers and is gated by a semaphore
- blocking worker timeout: configurable (`100..=300000` ms, default `15000` ms)

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
| `--blocking-task-timeout-ms` | `ZEUXIS_BLOCKING_TASK_TIMEOUT_MS` | `15000` | Timeout for blocking backend/storage workers (`100..=300000`). |
| n/a | `ZEUXIS_ARTIFACT_HMAC_KEY` | unset | Optional HMAC key; when set, `artifact_hmac_sha256` is included in capture results. |
| n/a | `RUST_LOG` | unset | Standard Rust logging filter for runtime diagnostics. |

## macOS Permissions

On macOS, Zeuxis preflights Screen Recording permission before each capture.

If permission is missing, Zeuxis requests access via CoreGraphics and returns `permission_denied` for that same invocation (no same-call retry). After granting access, call the tool again.

Cursor-dependent tools may also require Accessibility permission because they read global cursor position.

## Linux Notes

On Linux, Zeuxis does not run a dedicated permission preflight gate in v1. Capture behavior depends on your desktop environment/compositor and what the underlying capture backend permits.

If cursor-dependent tools fail (especially on Wayland), run `diagnose_runtime` first and fall back to `capture_screen` or `capture_rect` as needed.

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
