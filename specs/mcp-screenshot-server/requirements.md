# Requirements — mcp-screenshot-server

## Scope

Implement a local, read-only MCP screenshot server in Rust that supports screen, window, and region capture and returns local image artifacts through MCP tool results.

## System Name

`ZeuxisScreenshotServer`

## EARS Requirements

### Core protocol and safety

- R001: The ZeuxisScreenshotServer shall expose MCP tools over a local stdio transport.
- R001a: The ZeuxisScreenshotServer shall implement MCP server functionality using the Rust `rmcp` crate.
- R002: The ZeuxisScreenshotServer shall expose only read-only observational tools and no system-control tools.
- R003: The ZeuxisScreenshotServer shall validate all tool input arguments before attempting capture.
- R004: The ZeuxisScreenshotServer shall return tool execution failures as MCP tool results with `isError=true`.
- R004a: The ZeuxisScreenshotServer shall declare tool annotations as `readOnlyHint=true`, `destructiveHint=false`, and `idempotentHint=false`.

### Tool availability and behavior

- R005: The ZeuxisScreenshotServer shall provide `capture_screen`.
- R006: The ZeuxisScreenshotServer shall provide `capture_active_window`.
- R007: The ZeuxisScreenshotServer shall provide `capture_window_at_cursor`.
- R008: The ZeuxisScreenshotServer shall provide `capture_cursor_region`.
- R009: The ZeuxisScreenshotServer shall provide `capture_rect`.
- R010: When `capture_screen` is called, the ZeuxisScreenshotServer shall capture the primary monitor.
- R011: When `capture_active_window` is called, the ZeuxisScreenshotServer shall capture the focused non-minimized window.
- R012: When `capture_window_at_cursor` is called, the ZeuxisScreenshotServer shall select a best-effort non-minimized window containing the cursor point using backend-supported ordering.
- R013: When `capture_cursor_region` is called, the ZeuxisScreenshotServer shall capture a square region centered on the cursor.
- R014: When `capture_rect` is called, the ZeuxisScreenshotServer shall capture the exact validated rectangle.

### Delay and feedback

- R015: Where `delay_seconds` is provided, the ZeuxisScreenshotServer shall wait that duration before capture.
- R016: Where `play_sound=true`, the ZeuxisScreenshotServer shall emit capture feedback after a successful capture.

### Output contract

- R017: The ZeuxisScreenshotServer shall encode capture artifacts as local image files (`png`, `jpeg`, or `webp`) based on validated output settings.
- R018: The ZeuxisScreenshotServer shall include machine-readable output in `structuredContent`.
- R019: The ZeuxisScreenshotServer shall include a `resource_link` to a `file://` URI for the produced artifact.
- R020: The ZeuxisScreenshotServer shall include `path`, `width`, `height`, and `capture_mode` in successful structured output.
- R020a: The ZeuxisScreenshotServer shall include `captured_at_utc` as the original artifact capture timestamp in successful structured output.

### Coordinates and bounds

- R021: When a global region is mapped to a monitor-local capture API, the ZeuxisScreenshotServer shall apply coordinate conversion before capture.
- R022: If a requested region falls outside supported bounds, then the ZeuxisScreenshotServer shall reject the request with `invalid_region`.

### Permissions and platform constraints

- R023: While running on macOS, the ZeuxisScreenshotServer shall preflight screen-capture permission before capture.
- R023a: If macOS preflight indicates missing permission, then the ZeuxisScreenshotServer shall attempt `CGRequestScreenCaptureAccess` before returning an error.
- R023b: After attempting `CGRequestScreenCaptureAccess`, the ZeuxisScreenshotServer shall not retry capture in the same tool invocation.
- R024: If macOS screen-capture permission is not usable for the current invocation after the request attempt, then the ZeuxisScreenshotServer shall return `permission_denied` with remediation guidance.
- R025: While running on Linux in v1, the ZeuxisScreenshotServer shall operate in best-effort mode and surface backend capability/permission errors with stable error codes.
- R025a: While running on non-macOS and non-Linux platforms in v1, the ZeuxisScreenshotServer shall return `capture_unsupported_on_platform`.

### Reliability and observability

- R026: The ZeuxisScreenshotServer shall emit structured logs for tool invocation start, completion, and error paths.
- R027: If capture backend operations fail, then the ZeuxisScreenshotServer shall preserve the causal error category in the returned error code.
- R028: If temporary file creation or write fails, then the ZeuxisScreenshotServer shall return `storage_failed`.
- R028a: The ZeuxisScreenshotServer shall apply a configured timeout to blocking backend/storage worker tasks and return `storage_failed` on timeout.
- R028b: Artifact retention pruning shall be best effort and shall never delete the current artifact being returned.

### Privacy and non-goals

- R029: The ZeuxisScreenshotServer shall not upload screenshots to remote services.
- R030: The ZeuxisScreenshotServer shall not persist screenshots outside configured temporary storage unless explicitly configured in a future extension.
- R031: The ZeuxisScreenshotServer shall not perform OCR, UI element detection, or automated interaction in v1.
