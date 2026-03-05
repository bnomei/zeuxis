# Design — mcp-subprocess-capture-hard-timeout

## Overview

This spec replaces inline blocking capture execution with a subprocess worker model so timeout behavior is a true hard stop rather than best-effort `spawn_blocking` timeout.

The design keeps MCP behavior and response schemas stable while changing only the execution strategy for capture internals.

## Goals

- guarantee reliable timeout cancellation for capture jobs
- prevent stuck capture jobs from monopolizing semaphore permits
- preserve existing MCP tool request/response compatibility
- keep MCP stdio protocol output isolated from child process output

## Non-goals

- changing capture tool names or parameter schemas
- removing artifact `path`/`uri` from successful responses
- adding remote services or non-local artifact storage

## Normative Excerpt (for implementers)

- Parent process owns MCP protocol transport and response framing.
- Child process executes capture pipeline and is killable on timeout.
- Timeout is enforced by parent; child cannot extend deadline unilaterally.
- Worker IPC must be bounded, versioned, and validated before state mutation.
- Failed/timed-out workers must not mutate latest/session artifact state.

## Architecture

### 1. Parent coordinator remains authoritative

`execute_capture` in the MCP server keeps:
- input validation
- optional delay handling
- permission gate checks
- semaphore permit lifecycle
- final MCP result mapping and logging

Capture backend/encode work moves out of parent into a worker subprocess.

### 2. Worker subprocess mode

The binary adds an internal worker mode (hidden CLI/subcommand) that:
- reads one JSON request from stdin
- executes exactly one capture operation
- writes one JSON response to stdout
- exits

Worker mode does not run the MCP server.

### 3. Parent/worker IPC contract

Use versioned JSON request/response payloads.

Request fields:
- `v`
- `request_id`
- capture mode and resolved target parameters
- output settings (`format`, `jpeg_quality`, `max_dimension`)
- planned artifact file path

Response fields:
- `v`
- `request_id`
- `ok`
- success payload: artifact path + source/output dimensions + capture context metadata
- error payload: stable `error_code`, message, retryable

Validation rules:
- parent rejects mismatched version/request_id
- parent enforces max response bytes
- parent rejects malformed/partial payloads

### 4. Timeout and termination algorithm

For each capture request:
1. acquire semaphore permit with existing timeout budget logic
2. spawn worker
3. await worker completion using remaining timeout budget
4. on timeout, send terminate signal and wait up to `kill_grace_ms`
5. if still alive, force-kill and reap
6. return `storage_failed` timeout result

Permit is released by parent regardless of child behavior after reaping path completes.

### 5. Artifact adoption semantics

Current storage (`write_image`) assumes parent-encoded images. Add an adoption path for worker-produced artifacts:
- validate file existence and metadata
- compute integrity fields (sha256/hmac)
- apply retention policy
- update latest/session caches in parent

On worker failure/timeout:
- do not adopt artifact
- best-effort remove planned artifact file

### 6. MCP stdio protocol safety

Child process must never write to parent protocol stdout directly.
- spawn child with piped stdio controlled by parent (or null for non-IPC streams)
- parent writes/reads IPC via child pipes only
- parent continues to emit MCP frames on its own stdout only

### 7. Error mapping

- worker spawn failure -> `storage_failed`
- worker timeout/kill path -> `storage_failed`
- worker malformed output -> `storage_failed`
- worker known domain error -> mapped passthrough
- worker unknown error code -> `storage_failed`
- success path with missing artifact path -> `storage_failed`

## Data Contract Impact

No tool input schema change required.

Successful tool outputs remain compatible:
- include `path` and `uri`
- include capture metadata fields already exposed today

Internal-only additions:
- worker request/response schema types
- optional runtime knobs for worker timeout-kill tuning

## Observability

Add structured logs for:
- worker spawn start/complete
- timeout kill and force-kill paths
- worker exit status
- IPC decode/validation failures
- artifact adoption success/failure

Do not log raw image bytes.

## Testing Strategy

### Unit tests

- IPC schema roundtrips and version guards
- error-code mapping from worker payload to MCP errors
- timeout kill helper behavior with fake child process

### Integration tests

- capture timeout kills worker and returns `storage_failed`
- next capture succeeds after timeout (slot recovery)
- malformed worker output returns `storage_failed` without artifact adoption
- worker non-zero exit returns `storage_failed`
- successful worker response updates latest/session artifact state

### Quality gate

- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets`
