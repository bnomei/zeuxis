# Design — mcp-screenshot-server

## Overview

`mcp-screenshot-server` is a Rust MCP server that provides local screenshot capabilities for AI agents while remaining strictly read-only and local-first.

The design extends [docs/overview.md](/Users/bnomei/Sites/zeuxis/docs/overview.md) with implementation-level contracts, platform behavior, and fallback strategy.

## Goals

- minimal moving parts
- deterministic tool contract
- explicit platform and permission failure behavior
- local file artifact outputs that clients can consume reliably
- clear v1 scope on macOS first-class support with Linux best-effort support

## Non-goals

- OCR and object detection
- video streaming and frame pipelines
- any mouse/keyboard/window control
- cloud upload or remote transport in v1
- Windows implementation in v1

## Normative Excerpt (copied for implementers)

These constraints are the implementation source-of-truth for this spec:

- MCP tool results may include `content` items (`text`, `image`, `resource_link`, `resource`) and optional `structuredContent`.
- Tool execution failures should be returned as tool results with `isError=true`.
- `xcap` monitor API exposes `all`, `from_point`, `capture_image`, and `capture_region`.
- `xcap` window API exposes `all`, `is_focused`, geometry, and `capture_image`.
- `device_query` exposes current mouse coordinates through `DeviceState::get_mouse`.
- macOS screen-capture permission can be preflighted and requested through CoreGraphics access APIs.
- `NamedTempFile::keep` preserves a temp artifact without path move; `persist`/`persist_noclobber` are path-based operations with caveats.

## High-Level Architecture

```text
MCP Transport (stdio, rmcp)
  -> Tool Router
    -> Input Validation
    -> Capture Orchestrator
      -> Platform Permission Gate
      -> CaptureBackend trait
        -> XcapBackend (v1)
      -> PNG Storage Writer
    -> MCP Result Mapper (structuredContent + resource_link)
```

Implementation lock:

- use `rmcp` 1.x as the MCP crate in v1

## Module Design

### `mcp/tools.rs`

- declares tool schemas and handlers
- applies uniform pre/post handling:
  - delay
  - permission gate
  - capture dispatch
  - result mapping

### `capture/backend.rs`

- defines `CaptureBackend` trait for monitor/window/region operations
- enables deterministic unit tests with mock backend

### `capture/xcap_backend.rs`

- concrete `xcap` implementation
- handles:
  - monitor enumeration
  - focused window selection
  - window-under-cursor selection
  - region capture

### `cursor/device_query.rs`

- global cursor position lookup
- isolates platform-specific cursor failure mapping

### `platform/permissions.rs`

- macOS preflight/request helpers and denial mapping
- v1 platform guard (`cfg(target_os = "macos")` path + unsupported mapping elsewhere)
- shared typed permission errors

### `storage/temp_png.rs`

- secure temp file creation
- PNG encoding and write
- lifecycle policy (`keep` for return-to-client path)

### `mcp/result.rs`

- canonical success/error result shape
- helper for `resource_link` and `structuredContent` emission

## Tool Contracts

### Tool annotations

Each tool should expose:

- `readOnlyHint=true`
- `destructiveHint=false`
- `idempotentHint=false`

## Inputs

Common optional:

- `delay_seconds: number >= 0`
- `play_sound: boolean`

Specific:

- `capture_cursor_region(size: integer > 0)`
- `capture_rect(x: integer, y: integer, width: integer > 0, height: integer > 0)`

## Success output shape (`structuredContent`)

```json
{
  "path": "/tmp/...",
  "uri": "file:///tmp/...",
  "width": 1920,
  "height": 1080,
  "capture_mode": "capture_screen",
  "captured_at_utc": "2026-03-04T15:00:00Z"
}
```

`content` includes:

- text summary
- `resource_link` entry to the same `file://` URI

## Error output shape (`structuredContent`)

```json
{
  "error_code": "permission_denied",
  "message": "Screen capture permission was denied on macOS.",
  "retryable": true
}
```

`isError` is set to `true`.

## Coordinate Handling

- cursor coordinates are global
- monitor region APIs may be monitor-local
- conversion algorithm:
  1. identify target monitor by point intersection
  2. translate global `(x, y)` to monitor-local `(x - monitor.x, y - monitor.y)`
  3. clamp or reject based on requested mode and bounds policy
- `capture_screen` targets the primary monitor in v1
- `capture_window_at_cursor` uses backend-order best-effort selection when multiple windows overlap

## Platform Behavior

### macOS

- preflight before each capture attempt
- if preflight fails, attempt `CGRequestScreenCaptureAccess`
- do not auto-retry capture in the same invocation after request attempt (safer default)
- explicit actionable error on denial after request attempt

### Linux (v1 best effort)

- no dedicated permission preflight gate in v1
- behavior depends on compositor/backend availability and session state
- map backend failures into stable error codes with actionable remediation where possible

### Non-macOS/non-Linux platforms (v1)

- treated as explicitly unsupported in v1
- return `capture_unsupported_on_platform` with remediation text

## Temp File Policy

- write to secure temp file
- keep file after tool return so client can read it
- do not write outside temp locations in v1
- do not log raw image payloads
- retention pruning is best effort and never deletes the current artifact being returned

## Error Model

Internal error categories map to stable external codes:

- `PermissionDenied` -> `permission_denied`
- `Unsupported` -> `capture_unsupported_on_platform`
- `WindowNotFound` -> `window_not_found`
- `MonitorNotFound` -> `monitor_not_found`
- `InvalidRegion` -> `invalid_region`
- `CursorUnavailable` -> `cursor_unavailable`
- `EncodeFailed` -> `encode_failed`
- `StorageFailed` -> `storage_failed`

## Testing Strategy

### Unit tests

- region and coordinate conversions
- focused-window and window-under-cursor selection logic
- validation and error-code mapping
- result serialization conformance

### Integration tests

- tool handlers with mock backend
- smoke tests for all five tools in deterministic mode

### Manual checks

- macOS permission denied -> request prompt -> no same-call retry -> remediation/restart -> success on next call
- non-macOS returns `capture_unsupported_on_platform`

## Risks And Mitigations

- cross-platform expansion risk:
  - Mitigation: isolate backend behind trait boundary and keep unsupported mapping explicit in v1
- temp artifact leakage risk:
  - Mitigation: temp-only writes, documented cleanup behavior
- backend churn risk (`xcap` evolving quickly):
  - Mitigation: trait boundary and lockfile pinning

## Primary Sources

- MCP tools spec: <https://modelcontextprotocol.io/specification/draft/server/tools>
- MCP schema: <https://modelcontextprotocol.io/specification/2025-11-25/schema>
- RMCP docs: <https://docs.rs/crate/rmcp/latest>
- XCap monitor source: <https://raw.githubusercontent.com/nashaofu/xcap/master/src/monitor.rs>
- XCap window source: <https://raw.githubusercontent.com/nashaofu/xcap/master/src/window.rs>
- Device Query docs: <https://docs.rs/device_query>
- Apple CoreGraphics permission API: <https://developer.apple.com/documentation/coregraphics/cgrequestscreencaptureaccess()>
- Tempfile docs: <https://docs.rs/tempfile/latest/tempfile/struct.NamedTempFile.html>
