# zeuxis

[![Crates.io Version](https://img.shields.io/crates/v/zeuxis)](https://crates.io/crates/zeuxis)
[![CI](https://img.shields.io/github/actions/workflow/status/bnomei/zeuxis/ci.yml?branch=main)](https://github.com/bnomei/zeuxis/actions/workflows/ci.yml)
[![Crates.io Downloads](https://img.shields.io/crates/d/zeuxis)](https://crates.io/crates/zeuxis)
[![License](https://img.shields.io/crates/l/zeuxis)](https://crates.io/crates/zeuxis)
[![Discord](https://flat.badgen.net/badge/discord/bnomei?color=7289da&icon=discord&label)](https://discordapp.com/users/bnomei)
[![Buymecoffee](https://flat.badgen.net/badge/icon/donate?icon=buymeacoffee&color=FF813F&label)](https://www.buymeacoffee.com/bnomei)

Zeuxis is a local MCP screenshot server that lets AI agents capture the current desktop, windows, cursor regions, and exact rectangles through MCP tools.

It runs as one local binary over stdio by default. Capture results stay on the machine as managed image artifacts and are returned to the MCP client as `file://` resource links plus structured metadata. Zeuxis does not upload screenshots, perform OCR, drive the UI, or expose system-control tools.

## Supported platforms

| Platform | Status | Notes |
| --- | --- | --- |
| macOS | First-class | Zeuxis preflights Screen Recording permission before capture. Cursor-based tools may also need Accessibility permission. |
| Linux | Best effort | Behavior depends on the desktop environment, compositor, session type, and backend support. |
| Other platforms | Unsupported in v1 | Tools return `capture_unsupported_on_platform`. |

## Install

Use one of the following install paths.

### Cargo

Requires Rust `1.88` or newer.

```bash
cargo install zeuxis
zeuxis --version
```

### Homebrew

```bash
brew install bnomei/zeuxis/zeuxis
zeuxis --version
```

### GitHub Releases

Download a prebuilt archive from [GitHub Releases](https://github.com/bnomei/zeuxis/releases), extract it, and place `zeuxis` on your `PATH`.

Verify the binary:

```bash
zeuxis --help
```

### From source

```bash
git clone https://github.com/bnomei/zeuxis.git
cd zeuxis
cargo build --release
./target/release/zeuxis --version
```

## Quickstart

Add Zeuxis to an MCP client as a stdio server:

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

After the client connects, call `get_runtime_diagnostics` first. A healthy result reports `permission_ok=true` and `monitors_ok=true`. Then call `capture_screen` for the first screenshot.

Successful capture tools return:

- a short text summary,
- a `file://` resource link to the local artifact,
- structured fields such as `path`, `uri`, `output_format`, `mime_type`, `artifact_sha256`, `width`, `height`, `capture_mode`, `captured_at_utc`, `source_scale_factor`, and `target`.

## Choose a capture tool

| User intent | Tool |
| --- | --- |
| See the whole screen or get first-pass context | `capture_screen` |
| Capture the focused app window | `capture_active_window` |
| Capture the window under the cursor | `capture_cursor_window` |
| Capture a specific window from a window listing | `list_windows`, then `capture_window` |
| Capture a tooltip, menu, or small cursor-adjacent area | `capture_cursor_region` |
| Capture exact global desktop coordinates | `capture_rect` |
| Capture exact monitor-local coordinates | `capture_monitor_region` |
| Reuse the last screenshot from this server session | `get_latest_capture` |
| Inspect or delete Zeuxis artifacts from this session | `list_session_artifacts`, `clear_session_artifacts` |

For deterministic window capture, call `list_windows` and pass both `snapshot_id` and `window_id` from that same response to `capture_window`. Window IDs are scoped to the snapshot, not durable across listings.

## MCP tools

Tool schemas are defined in [`src/mcp/tools.rs`](src/mcp/tools.rs). Result payloads are built in [`src/mcp/result.rs`](src/mcp/result.rs), and stable errors are defined in [`src/mcp/errors.rs`](src/mcp/errors.rs).

| Tool | Parameters | Description |
| --- | --- | --- |
| `list_monitors` | none | Lists monitors with IDs, names, logical bounds, and primary/built-in flags. |
| `list_windows` | `focused_only?`, `include_system_windows?`, `app_contains?`, `title_contains?` | Lists windows and records a snapshot for `capture_window`. System UI surfaces are excluded unless requested. |
| `get_runtime_diagnostics` | none | Reports OS/session context, permission status, monitor discovery, and cursor availability. |
| `get_latest_capture` | none | Returns the latest artifact from the current server session without taking a new screenshot. |
| `list_session_artifacts` | none | Lists artifacts created in the current server session and marks the latest one. |
| `clear_session_artifacts` | none | Deletes artifacts created in the current server session and resets latest-capture state. |
| `capture_screen` | `monitor_id?` plus shared capture parameters | Captures a full monitor. Omitting `monitor_id` selects the primary monitor. |
| `capture_active_window` | shared capture parameters | Captures the focused, non-minimized window. |
| `capture_cursor_window` | `include_system_windows?` plus shared capture parameters | Captures the non-system window under the cursor by default. |
| `capture_window` | `snapshot_id`, `window_id` plus shared capture parameters | Captures a window selected from a `list_windows` snapshot. |
| `capture_cursor_region` | `size` plus shared capture parameters | Captures a square region centered on the cursor. |
| `capture_rect` | `x`, `y`, `width`, `height` plus shared capture parameters | Captures a global desktop rectangle in logical points. |
| `capture_monitor_region` | `monitor_id`, `x`, `y`, `width`, `height` plus shared capture parameters | Captures a monitor-local rectangle in logical points. |

Shared capture parameters:

| Parameter | Type | Default | Notes |
| --- | --- | --- | --- |
| `delay_ms` | integer | unset | Optional pre-capture delay in milliseconds. Range: `0..=30000`. Do not combine with `delay_seconds`. |
| `delay_seconds` | number | unset | Optional pre-capture delay in seconds. Range: `0..=30`. Do not combine with `delay_ms`. |
| `play_sound` | boolean | `false` | Plays capture-complete feedback after a successful capture. |
| `output` | string or object | `"analysis"` | Controls artifact format, downscaling, and JPEG quality. |

Examples:

```json
{ "delay_ms": 800, "play_sound": true }
```

```json
{ "output": "compact" }
```

```json
{
  "output": {
    "mode": "custom",
    "format": "webp",
    "max_dimension": 2048
  }
}
```

## Output options

Preset output modes:

| Preset | Format | Max dimension | JPEG quality | Use when |
| --- | --- | --- | --- | --- |
| `analysis` | PNG | `2560` | n/a | Default LLM analysis with moderate downscaling. |
| `exact` | PNG | original size | n/a | You need original pixels and lossless output. |
| `compact` | JPEG | `1600` | `85` | You want smaller artifacts for faster transfer. |

Custom output mode:

| Field | Required | Constraints |
| --- | --- | --- |
| `mode` | yes | Must be `"custom"`. |
| `format` | yes | `"png"`, `"jpeg"`, or `"webp"`. |
| `max_dimension` | no | Longest output side in pixels, `256..=8192`. |
| `jpeg_quality` | only for JPEG | `40..=95`. Rejected for PNG and WebP. |

If `ZEUXIS_ARTIFACT_HMAC_KEY` is set, capture results also include `artifact_hmac_sha256`.

## Coordinates and limits

Coordinate inputs use logical desktop points. Captured image dimensions use source pixels. Use the returned `input_units`, `source_units`, and `source_scale_factor` fields to reason about HiDPI scaling.

Runtime limits:

| Limit | Value |
| --- | --- |
| `delay_ms` | `0..=30000` |
| `delay_seconds` | `0..=30` |
| Capture width or height | `1..=16384` |
| Capture area | `<= 40000000` pixels |
| Custom output `max_dimension` | `256..=8192` |
| JPEG quality | `40..=95` |

Requested delays run before capture work and are additive to the capture timeout. For example, a request with `delay_ms=30000` and the default `--blocking-task-timeout-ms=15000` can take up to about 45 seconds before the client receives a timeout or result.

## Configuration

Configuration is resolved as `CLI flag > environment variable > default`. Zeuxis does not read config files.

Runtime configuration lives in [`src/runtime_config.rs`](src/runtime_config.rs).

| CLI flag | Environment variable | Default | Range | Description |
| --- | --- | --- | --- | --- |
| `--max-concurrent-captures` | `ZEUXIS_MAX_CONCURRENT_CAPTURES` | `2` | `1..=16` | Maximum concurrent capture workers. |
| `--max-artifacts` | `ZEUXIS_MAX_ARTIFACTS` | `64` | `1..=10000` | Maximum retained Zeuxis temp image files. |
| `--max-artifact-bytes` | `ZEUXIS_MAX_ARTIFACT_BYTES` | `536870912` | `1024..=10737418240` | Maximum retained artifact bytes. |
| `--artifact-dir` | `ZEUXIS_ARTIFACT_DIR` | system temp dir | path | Directory for managed capture artifacts. |
| `--blocking-task-timeout-ms` | `ZEUXIS_BLOCKING_TASK_TIMEOUT_MS` | `15000` | `100..=300000` | Timeout for capture, listing, and storage work. Delays run before this timeout. |
| `--worker-kill-grace-ms` | `ZEUXIS_WORKER_KILL_GRACE_MS` | `250` | `10..=30000` | Grace period between soft worker termination and hard kill. |
| `--max-worker-stdout-bytes` | `ZEUXIS_MAX_WORKER_STDOUT_BYTES` | `65536` | `1024..=4194304` | Maximum worker IPC stdout bytes accepted by the parent. |
| `--capture-sound-file` | `ZEUXIS_CAPTURE_SOUND_FILE` | platform default | path | Optional custom sound file for `play_sound=true`. |
| n/a | `ZEUXIS_ARTIFACT_HMAC_KEY` | unset | non-empty string | Optional HMAC key for artifact integrity metadata. |
| n/a | `RUST_LOG` | `info` | tracing filter | Runtime logging filter. Logs go to stderr to keep MCP stdout clean. |

Example:

```bash
ZEUXIS_MAX_CONCURRENT_CAPTURES=4 \
ZEUXIS_MAX_ARTIFACTS=128 \
zeuxis --blocking-task-timeout-ms 30000
```

## Platform permissions

### macOS

Zeuxis checks Screen Recording permission before capture. If permission is missing, Zeuxis asks macOS for access and returns `permission_denied` for that same tool call. Grant Screen Recording permission to the terminal or host app that starts Zeuxis, then retry the tool call.

Cursor-dependent tools read the global cursor position and may also require Accessibility permission. If those fail, try `capture_screen` or `capture_rect` while you update permissions.

### Linux

Linux capture support depends on the graphical session and backend capabilities. If capture fails, call `get_runtime_diagnostics` and check `xdg_session_type`, `display`, `wayland_display`, `monitors_ok`, and `cursor_ok`.

On Wayland, cursor and window capture behavior can be more limited than full-screen capture. Prefer `capture_screen` first, then narrow to regions if the compositor allows it.

## Troubleshooting

### `permission_denied`

Cause: The OS denied screen capture permission.

Fix:

1. On macOS, grant Screen Recording permission to the terminal or MCP host app.
2. Retry the same tool call after granting permission.

Verify:

1. Call `get_runtime_diagnostics`.
2. Confirm `permission_ok=true`.

### `cursor_unavailable`

Cause: Zeuxis could not read the global cursor position.

Fix:

1. Grant Accessibility permission if your platform requires it.
2. Use `capture_screen`, `capture_active_window`, or `capture_rect` when cursor position is unavailable.

### `window_not_found`

Cause: The focused window, cursor window, or requested snapshot window is no longer available.

Fix:

1. Call `list_windows` again.
2. Retry with a fresh `snapshot_id` and `window_id`, or fall back to `capture_screen`.

### `invalid_region`

Cause: The requested rectangle is outside supported bounds or exceeds the size limits.

Fix:

1. Check monitor bounds with `list_monitors`.
2. Reduce `width` and `height`.
3. Keep the capture area at or below `40000000` pixels.

### `no_capture_yet`

Cause: `get_latest_capture` was called before this server session captured an artifact.

Fix:

1. Call a `capture_*` tool first.
2. Retry `get_latest_capture`.

### `storage_failed`

Cause: Artifact write, retention cleanup, worker IPC, or timeout handling failed.

Fix:

1. Check that `ZEUXIS_ARTIFACT_DIR` is writable, if set.
2. Increase `--blocking-task-timeout-ms` for slow captures.
3. Retry the capture. Timed-out worker processes are terminated and reaped before Zeuxis returns.

## Privacy and safety

Zeuxis is designed for local observation:

- It serves MCP over local stdio.
- It returns local `file://` artifact links.
- It does not upload screenshots to remote services.
- It does not perform OCR, UI element detection, input automation, shell execution, or window control.
- It validates tool parameters before capture.
- Capture work runs in a subprocess worker with parent-enforced timeout and termination.
- `clear_session_artifacts` deletes only Zeuxis-managed artifacts from the current session.

Managed artifact files use the `zeuxis-` prefix and `.png`, `.jpg`, or `.webp` suffix. Retention pruning is best effort and never deletes the artifact currently being returned.

## Development

Useful source entrypoints:

| File | Purpose |
| --- | --- |
| [`src/main.rs`](src/main.rs) | CLI parsing, stdio server startup, hidden worker mode, tracing setup. |
| [`src/runtime_config.rs`](src/runtime_config.rs) | CLI/env defaults, ranges, and runtime settings. |
| [`src/mcp/tools.rs`](src/mcp/tools.rs) | MCP tool schemas, validation, capture execution, output settings. |
| [`src/mcp/result.rs`](src/mcp/result.rs) | MCP result payloads and resource links. |
| [`src/mcp/errors.rs`](src/mcp/errors.rs) | Stable error codes and retryability. |
| [`src/capture/backend.rs`](src/capture/backend.rs) | Capture backend trait and monitor/window metadata. |
| [`src/worker/contract.rs`](src/worker/contract.rs) | Parent/worker JSON contract. |
| [`skills/capturing-ui-with-zeuxis/SKILL.md`](skills/capturing-ui-with-zeuxis/SKILL.md) | Codex skill guidance for using Zeuxis proactively. |
| [`specs/`](specs/) | Historical design and requirements specs. |

Run local checks:

```bash
cargo check
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

On Ubuntu/Linux CI-like environments, install the capture backend build dependencies first:

```bash
sudo apt-get update
sudo apt-get install -y \
  pkg-config \
  libclang-dev \
  libxcb1-dev \
  libxrandr-dev \
  libdbus-1-dev \
  libpipewire-0.3-dev \
  libwayland-dev \
  libegl-dev \
  libdrm-dev \
  libgbm-dev
```

This repo also ships a `prek.toml` for lightweight local commit gates:

```bash
prek validate-config
prek run --all-files
prek install
```

The configured hooks run `cargo fmt --all -- --check` and `cargo clippy --all-targets --all-features -- -D warnings`.

## License

Zeuxis is licensed under the [MIT License](LICENSE).
