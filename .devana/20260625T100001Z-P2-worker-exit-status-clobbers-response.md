DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: open
Location: src/worker/parent.rs:96 | Slug: worker-exit-status-clobbers-response

# Non-zero worker exit overrides an already-parsed valid response

## Finding

In `run_capture_worker` the parent first parses and validates the worker's stdout into a
complete `WorkerResponse` (`src/worker/parent.rs:82-87`) and checks the `request_id`
matches (`:89-94`). It then unconditionally checks `if !status.success()`
(`src/worker/parent.rs:96-100`) and, on any non-zero exit, returns a generic
`ServerError::storage_failed("capture worker exited with status ...")` — discarding the
structured outcome the worker already produced (the `ok=true` `result`, or the `ok=false`
`error` with its real `error_code` and `retryable` flag). The `response.ok` dispatch at
`:102-113` is never reached in that case.

## Violated Invariant Or Contract

The worker protocol communicates outcome through the JSON response, not the exit code:
`handle_request` (`src/worker/child.rs`) encodes every capture error as a valid
`ok=false` response with the correct `error_code`/`retryable` and returns `Ok(())`, so a
clean worker exits 0 even for capture errors. A non-zero exit is meant to signal "no
usable response / crash." Therefore, when a valid, id-matched response is present, the
parent must return that response's outcome; the exit status should only decide the
outcome when no valid response exists (which is already handled by the parse failure at
`:82` and the missing-payload branches at `:103`/`:108`).

## Oracle

`src/worker/child.rs` (capture errors are serialized as structured `ok=false`
responses with correct `error_code`/`retryable`, then `Ok(())`) and `src/main.rs:97-99`
(a worker error propagates to a non-zero process exit). Together these establish that a
valid response can coexist with a non-zero exit, and that the response carries the true
semantics.

## Counterexample

1. Worker handles `capture_window` for a missing window. It builds and writes a complete
   `ok=false` response `{error_code:"window_not_found", retryable:false}` to stdout
   (`src/worker/child.rs`), then reaches the final `flush()`.
2. The final stdout `flush()`/write returns a transient `io::Error`, so
   `run_stdio_worker` returns `Err`, and `main` propagates it → the process exits
   non-zero, while stdout already holds the full valid response.
3. Parent: `parse_response_json` succeeds, `request_id` matches, then `!status.success()`
   is true (`:96`) → parent returns `storage_failed(...)` with `retryable=true`.
4. The client receives `error_code="storage_failed", retryable=true` instead of the true
   `window_not_found, retryable=false`.

Symmetric success direction: worker writes a complete `ok=true` result (artifact already
on disk) then exits non-zero for the same reason → parent returns `storage_failed`, so a
capture that actually succeeded is reported as a retryable storage failure and its
artifact is discarded.

## Why It Might Matter

A deterministic, non-retryable error (`window_not_found`, `permission_denied`,
`invalid_argument`, ...) is masked as a transient retryable `storage_failed`, inviting
pointless retries and losing the real diagnostic. In the success direction, a good
capture is reported as a failure. Both require a late post-serialize failure or abnormal
exit, but such exits are exactly what this branch is meant to handle.

## Proof

Control-flow ordering within `run_capture_worker`:
- `src/worker/parent.rs:82-87`: full response parsed first.
- `src/worker/parent.rs:89-94`: request_id verified.
- `src/worker/parent.rs:96-100`: `!status.success()` returns generic `storage_failed`,
  short-circuiting before the `response.ok` dispatch.
- `src/worker/parent.rs:102-113`: structured outcome (real result / real error) never
  reached when the exit is non-zero.

## Counterevidence Checked

- Normal capture-error path: worker exits 0, so `:96` passes and the real error is
  returned at `:113` — the bug only fires when a valid response coexists with a non-zero
  exit, which is reachable via a post-serialize flush/write failure
  (`src/worker/child.rs`) or any late abort after stdout is fully written.
- Crash with empty/partial stdout is handled earlier: `parse_response_json` fails at
  `:82` → `storage_failed("response was invalid")`, so the status check is not the
  mechanism there.
- The timeout and `child.wait()` I/O-error paths return before parsing (`:64-72`,
  `:57-62`), so this is specifically the valid-response + non-zero-exit case.

## Suggested Next Step

When a valid, id-matched response is present, dispatch on `response.ok`/`response.error`
first and only fall back to the exit-status error when no usable response was parsed
(i.e. move the `!status.success()` check ahead of parsing, or only apply it when the
response is absent).

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...`
and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`,
`duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.

DEVANA-KEY: src/worker/parent.rs:96 | P2 | worker-exit-status-clobbers-response
DEVANA-SUMMARY: Status=open | P2 high src/worker/parent.rs:96 - A non-zero worker exit replaces an already-parsed valid response with generic retryable storage_failed, so a real non-retryable error (or a successful capture) is misreported when the worker aborts after writing stdout.
