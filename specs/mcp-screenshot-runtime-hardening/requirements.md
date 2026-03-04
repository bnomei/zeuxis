# Requirements — mcp-screenshot-runtime-hardening

## Scope

Define runtime hardening and diagnostics additions for `ZeuxisScreenshotServer` that extend the base screenshot server specification.

This spec is additive to [specs/mcp-screenshot-server/requirements.md](/Users/bnomei/Sites/zeuxis/specs/mcp-screenshot-server/requirements.md).

## System Name

`ZeuxisScreenshotServer`

## EARS Requirements

### Blocking timeout budget and coordination

- H001: While executing capture tools, the ZeuxisScreenshotServer shall enforce the configured blocking-task timeout across both slot acquisition and blocking worker execution.
- H001a: If capture slot acquisition does not complete within the configured timeout, then the ZeuxisScreenshotServer shall return `storage_failed`.
- H001b: When capture slot acquisition consumes part of the timeout budget, the ZeuxisScreenshotServer shall apply only the remaining budget to the blocking worker phase.
- H001c: If no timeout budget remains after slot acquisition, then the ZeuxisScreenshotServer shall return `storage_failed` without executing backend capture or storage write.
- H002: Where tool handlers execute blocking backend or storage jobs (`list_monitors`, `diagnose_runtime` monitor probe, `get_latest_screenshot`, and capture worker execution), the ZeuxisScreenshotServer shall map timeout and join failures using a shared timeout/join error policy.

### Runtime diagnostics payload clarity

- H003: When `diagnose_runtime` returns structured diagnostics, the ZeuxisScreenshotServer shall include `permission_checked` as a boolean field.
- H003a: When `diagnose_runtime` returns structured diagnostics, the ZeuxisScreenshotServer shall include `permission_check_mode` as a string field.
- H003b: While running on Linux, the ZeuxisScreenshotServer shall report `permission_checked=false` and `permission_check_mode="best_effort_unchecked"`.
- H003c: While running on macOS, the ZeuxisScreenshotServer shall report `permission_checked=true` and `permission_check_mode="macos_preflight"`.
- H003d: While running on non-macOS and non-Linux platforms, the ZeuxisScreenshotServer shall report `permission_checked=true` and `permission_check_mode="unsupported_platform"`.

### Runtime configuration hardening

- H004: If a configured runtime environment variable is syntactically invalid or outside allowed bounds, then the ZeuxisScreenshotServer shall use default values and emit a warning log containing the variable name and fallback behavior.
- H005: When constructing ZeuxisScreenshotServer with explicit blocking-timeout values, the ZeuxisScreenshotServer shall clamp the timeout to the configured minimum and maximum policy bounds.

### Storage retention observability

- H006: If retention pruning cannot delete an artifact candidate, then the ZeuxisScreenshotServer shall keep processing and emit a warning log containing the candidate path and deletion error.

### Regression coverage expectations

- H007: The ZeuxisScreenshotServer test suite shall include automated tests for capture slot coordination failure and capture slot acquisition timeout behavior.
- H007a: The ZeuxisScreenshotServer test suite shall include automated tests for runtime configuration parse-fallback behavior.
- H007b: The ZeuxisScreenshotServer test suite shall include automated tests for retention-prune delete-failure behavior.
