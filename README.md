# zeuxis

[![Crates.io Version](https://img.shields.io/crates/v/zeuxis)](https://crates.io/crates/zeuxis)
[![CI](https://img.shields.io/github/actions/workflow/status/bnomei/zeuxis/ci.yml?branch=main)](https://github.com/bnomei/zeuxis/actions/workflows/ci.yml)
[![Crates.io Downloads](https://img.shields.io/crates/d/zeuxis)](https://crates.io/crates/zeuxis)
[![License](https://img.shields.io/crates/l/zeuxis)](https://crates.io/crates/zeuxis)

Zeuxis is a local, read-only MCP screenshot server that lets AI agents capture screenshots themselves.

When your MCP client connects to Zeuxis, the agent can call screenshot tools directly (full screen, active window, cursor region, or exact rect), then immediately use the returned `file://` image link and metadata in its next reasoning step. No manual screenshot/upload handoff is required.

Zeuxis is:
- CLI-first: one local binary, stdio MCP transport by default.
- MCP-first: explicit tool schemas and stable error codes.
- Safety-first: strict input validation, bounded capture concurrency, and temp-file retention limits.

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
- `capture_screen`
- `capture_active_window`
- `capture_window_at_cursor`
- `capture_cursor_region`
- `capture_rect`

Successful tool results include:
- `content` text summary
- `content` `resource_link` to local `file://` PNG
- `structuredContent` with `path`, `uri`, `width`, `height`, `capture_mode`, `captured_at_utc`

Error results use `isError=true` with structured fields:
- `error_code`
- `message`
- `retryable`

## Runtime Safety Limits

- `delay_seconds` max: `30`
- capture dimension max: `16384 x 16384`
- capture area max: `40,000,000` pixels
- capture work runs on blocking workers and is gated by a semaphore

Temp artifact retention:
- managed files use prefix `zeuxis-` and suffix `.png`
- older artifacts are pruned on each successful write

## Configuration

| Variable | Default | Meaning |
| --- | --- | --- |
| `ZEUXIS_MAX_CONCURRENT_CAPTURES` | `2` | Max parallel capture workers (`1..=16`). |
| `ZEUXIS_MAX_ARTIFACTS` | `64` | Max retained Zeuxis temp PNG files (`1..=10000`). |
| `ZEUXIS_MAX_ARTIFACT_BYTES` | `536870912` | Max retained Zeuxis temp PNG bytes (`1024..=10737418240`). |
| `RUST_LOG` | unset | Standard Rust logging filter for runtime diagnostics. |

## macOS Permissions

On macOS, Zeuxis preflights Screen Recording permission before each capture.

If permission is missing, Zeuxis requests access via CoreGraphics and returns `permission_denied` for that same invocation (no same-call retry). After granting access, call the tool again.

Cursor-dependent tools may also require Accessibility permission because they read global cursor position.

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
