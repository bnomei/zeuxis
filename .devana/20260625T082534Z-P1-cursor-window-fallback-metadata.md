DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: open
Location: src/mcp/tools.rs:1415 | Slug: cursor-window-fallback-metadata

# capture_cursor_window reports resolved window metadata after unfiltered fallback capture

## Finding

`CaptureCursorWindow` resolves a window using filtered `list_windows` semantics (`include_system_windows`), then tries `capture_window(resolved.id)`. On failure it falls back to `capture_window_at_cursor`, which scans **unfiltered** backend window order. The success payload always binds `target.window_id`, `input_width`, and `input_height` to the originally resolved window, even when the fallback captured a different window.

## Violated Invariant Or Contract

`CaptureSuccessPayload.target.window_id` and input dimensions must describe the window that was actually captured. Skill/docs expect clients to use `target.window_id` to confirm which surface was resolved.

## Oracle

`resolve_window_at_cursor_with_filter` applies `filter_windows` with `include_system_windows` (`src/mcp/tools.rs:1685–1712`). `capture_window_at_cursor` in `src/capture/xcap_backend.rs:254–264` calls `all_windows()` with no system-window filter and picks the first backend-order hit via `select_window_at_cursor_index`. Both inline (`src/mcp/tools.rs:1415–1433`) and worker (`src/worker/child.rs:141–159`) share the same pattern.

## Counterexample

`include_system_windows=false`. Cursor sits inside filtered window W1 (id=100) that overlaps a system overlay W0 listed earlier by the backend. `resolve_window_at_cursor_with_filter` selects W1. `capture_window(100)` fails (transient backend error or capture restriction). `capture_window_at_cursor` succeeds on W0. Response reports `target.window_id = 100`, `input_width/height` from W1, but the image is W0.

## Why It Might Matter

Agents acting on `target.window_id` or input dimensions will reason about the wrong window. Follow-up `capture_window` calls using the returned metadata will target a different surface than the screenshot shows.

## Proof

**Dataflow trace:** `resolve_window_at_cursor_with_filter` (filtered) → `capture_window(resolved.id)` fails → `capture_window_at_cursor` (unfiltered backend order) succeeds → `CaptureWorkOutput.target` still bound to `resolved_window` (`src/mcp/tools.rs:1421–1432`).

**Cross-entry mismatch:** Window selection in resolve path uses `filter_windows`; fallback capture path uses raw `all_windows()` ordering (`src/capture/xcap_backend.rs:255–264`).

## Counterevidence Checked

- Backend-order overlap selection is documented and tested (`capture_select_window_at_cursor_uses_backend_order`); that behavior does not justify reporting window A while capturing window B.
- No test covers `capture_window` failure followed by fallback success.
- `include_system_windows=true` reduces but does not eliminate mismatch when `capture_window(id)` fails for other reasons.

## Suggested Next Step

On fallback success, re-resolve the captured window from the image path (or capture only via one consistent selection path) and update `target`/`input_*` to match the window actually captured.

DEVANA-KEY: src/mcp/tools.rs:1415 | P1 | cursor-window-fallback-metadata
DEVANA-SUMMARY: P1 high src/mcp/tools.rs:1415 - capture_cursor_window fallback can capture a different window than target.window_id reports when capture_window(id) fails.