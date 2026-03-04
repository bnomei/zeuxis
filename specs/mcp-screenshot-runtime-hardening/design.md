# Design — mcp-screenshot-runtime-hardening

## Overview

This spec captures runtime hardening behavior implemented after the base screenshot server scope, with emphasis on timeout budgeting, diagnostics clarity, fallback observability, and regression coverage.

It extends [specs/mcp-screenshot-server/design.md](/Users/bnomei/Sites/zeuxis/specs/mcp-screenshot-server/design.md) and does not replace base tool contracts.

## Goals

- enforce deterministic timeout behavior under queue contention
- remove duplicated blocking timeout/join error mapping logic
- expose explicit diagnostics metadata for permission-check semantics
- make runtime config fallback behavior observable in logs
- make retention prune failure behavior observable in logs
- keep regression tests aligned with these behaviors

## Non-goals

- adding new MCP tools
- changing existing stable error-code names
- changing capture output format contracts
- introducing remote telemetry or external logging dependencies

## Normative Excerpt (for implementers)

- Capture execution uses a bounded semaphore for parallelism control.
- Capture execution uses blocking worker tasks for backend capture and storage write.
- Diagnostics payload already includes permission and component status fields.
- Runtime config values are loaded from environment variables with min/max policy bounds.
- Artifact retention is best effort and should not fail successful capture responses.

## Architecture Additions

### 1. Timeout budget split for capture execution

Capture execution now treats timeout as a single budget split across two phases:

1. `capture_slots.acquire_owned()` phase
2. blocking worker phase (`backend capture` + optional `downscale` + `storage write`)

Algorithm:

- Start with configured timeout `T`.
- Attempt slot acquisition with timeout `T`.
- If acquisition times out, return `storage_failed`.
- If acquisition succeeds after elapsed duration `E`, compute `remaining = T - E`.
- If `remaining == 0`, return `storage_failed` immediately.
- Run blocking worker with timeout `remaining`.

Result:

- queued calls do not wait forever behind long-running captures
- timeout semantics remain bounded and predictable per invocation

### 2. Shared blocking timeout helper

A single helper encapsulates:

- `tokio::task::spawn_blocking`
- `tokio::time::timeout`
- join-error mapping to `storage_failed`
- timeout mapping to `storage_failed`

Applied to:

- monitor listing tool
- diagnostics monitor probe
- latest artifact lookup
- capture worker execution

Result:

- uniform timeout/join error behavior
- lower drift risk across handlers

### 3. Diagnostics metadata fields

`diagnose_runtime` structured payload includes:

- `permission_checked: bool`
- `permission_check_mode: string`

Platform mapping:

- Linux: `false`, `"best_effort_unchecked"`
- macOS: `true`, `"macos_preflight"`
- non-macOS/non-Linux: `true`, `"unsupported_platform"`

Result:

- clients can distinguish “permission is okay” from “permission was not preflighted”

### 4. Runtime config fallback warnings

Environment-lookup parsing behavior:

- valid in-range values are accepted
- invalid or out-of-range values fall back to defaults
- warning logs include env var name, provided value, bounds, and fallback

Result:

- safer operations when env config is malformed
- better explainability in logs

### 5. Constructor timeout clamping

Constructors that accept explicit timeout values normalize to policy bounds:

- min clamp
- max clamp

Result:

- consistent timeout behavior regardless of constructor entrypoint

### 6. Retention prune warning path

When a prune candidate cannot be removed:

- continue processing
- emit warning log containing file path and error

Result:

- preserves best-effort retention semantics
- surfaces disk/permission cleanup issues for operators

## Data Contract Delta

### `diagnose_runtime` (`structuredContent`)

Added fields:

```json
{
  "permission_checked": false,
  "permission_check_mode": "best_effort_unchecked"
}
```

No existing fields are removed.

## Failure Behavior

- Slot acquisition timeout -> `storage_failed`
- Blocking worker timeout -> `storage_failed`
- Blocking worker join failure -> `storage_failed`
- Retention prune delete failure -> warning log only; capture result remains successful if artifact write succeeded

## Testing Strategy

### New regression tests

- capture slot acquisition timeout under contention
- capture slot coordination failure path (closed semaphore)
- runtime config parse-fallback path
- retention prune delete-failure path

### Quality gate

- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets`
- `cargo llvm-cov --summary-only`
