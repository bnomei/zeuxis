# Requirements — mcp-subprocess-capture-hard-timeout

## Scope

Define a reliable hard-timeout execution model for all `capture_*` tools so timed-out capture work is forcibly terminated and cannot hold capture slots indefinitely.

This spec is additive to:
- [specs/mcp-screenshot-server/requirements.md](/Users/bnomei/Sites/zeuxis/specs/mcp-screenshot-server/requirements.md)
- [specs/mcp-screenshot-runtime-hardening/requirements.md](/Users/bnomei/Sites/zeuxis/specs/mcp-screenshot-runtime-hardening/requirements.md)

## System Name

`ZeuxisScreenshotServer`

## EARS Requirements

### Hard-timeout execution and cancellation

- C001: When any `capture_*` tool executes capture work, the ZeuxisScreenshotServer shall run that capture work in a dedicated child process.
- C002: While a capture request is running, the ZeuxisScreenshotServer shall enforce the configured timeout budget in the parent process.
- C003: If a child process exceeds the timeout budget, then the ZeuxisScreenshotServer shall terminate the child process and reap it before returning the tool result.
- C003a: If graceful termination does not complete within a bounded grace period, then the ZeuxisScreenshotServer shall force-kill the child process and reap it.
- C004: If a capture request times out, then the ZeuxisScreenshotServer shall return `storage_failed` and shall release the capture slot for subsequent requests.

### MCP stdio safety and transport compatibility

- C005: While serving MCP over stdio, the ZeuxisScreenshotServer shall keep parent `stdout` reserved for MCP protocol frames only.
- C005a: When spawning capture workers, the ZeuxisScreenshotServer shall not allow worker output to inherit MCP protocol stdout.
- C006: Where the server is hosted behind different transports (including stdio and HTTP wrappers), the ZeuxisScreenshotServer shall use the same capture timeout and worker-kill semantics.

### Artifact integrity and state mutation rules

- C007: When a capture worker reports success, the ZeuxisScreenshotServer shall validate worker output before committing artifact metadata into session state.
- C008: If a capture worker times out, exits non-zero, or returns malformed output, then the ZeuxisScreenshotServer shall not publish the artifact in `latest` or session artifact lists.
- C008a: If a capture worker fails after creating a partial artifact file, then the ZeuxisScreenshotServer shall attempt cleanup and shall not return that path to the client.
- C009: When capture succeeds, the ZeuxisScreenshotServer shall preserve existing successful tool response semantics, including artifact path/uri availability.

### Error compatibility and diagnostics

- C010: If worker startup, IPC decoding, or worker termination fails, then the ZeuxisScreenshotServer shall return `storage_failed` with actionable diagnostic text.
- C011: When a worker returns a known domain error code, the ZeuxisScreenshotServer shall preserve the mapped external error code in the tool result.
- C012: The ZeuxisScreenshotServer test suite shall include deterministic coverage for timeout kill, slot recovery, malformed worker response, non-zero worker exit, and successful artifact adoption.
