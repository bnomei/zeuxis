DEVANA-FINDING: v1
Priority: P2 | Confidence: medium | Security-sensitive: no | Status: open
Location: src/worker/parent.rs:161 | Slug: terminate-worker-wait-io-error

# terminate_worker treats child.wait() I/O error as successful termination

## Finding

After sending SIGTERM, `terminate_worker` checks `tokio::time::timeout(kill_grace, child.wait()).await.is_ok()`. The outer `Result::is_ok()` is true for both `Ok(Ok(status))` and `Ok(Err(io_error))`. On wait I/O failure, the function returns `Ok(())` without escalating to `child.kill()` or confirming reap.

## Violated Invariant Or Contract

C003/C003a: on timeout, parent shall terminate the child and reap it before returning. Design §4 steps 4–5: graceful terminate, then force-kill if still alive.

## Oracle

`specs/mcp-subprocess-capture-hard-timeout/requirements.md` C003, C003a; `terminate_worker` implementation at `src/worker/parent.rs:141–177`.

## Counterexample

Capture worker times out. Parent sends SIGTERM. `child.wait()` returns an I/O error inside the grace window (e.g. transient wait failure). `timeout(...).await` is `Ok(Err(e))`; `.is_ok()` is true; function returns success. Hard-kill path (lines 165–176) is skipped. Child may remain running until `Child` drop.

## Why It Might Matter

Timed-out workers may not be explicitly reaped, leaving zombie or lingering capture processes that hold resources or continue screen-capture work past the declared deadline.

## Proof

**Control-flow trace:** `Err(_)` timeout branch in `run_worker_capture` (64–71) → `terminate_worker` → `if timeout(kill_grace, child.wait()).await.is_ok() { return Ok(()); }` (161–163) — wait I/O error matches this branch.

**Contract mismatch:** Intended "reaped successfully" vs actual "wait returned without elapsed timeout".

## Counterevidence Checked

- `Child` drop on Unix typically sends SIGKILL as a backstop, mitigating but not satisfying explicit reap semantics.
- Wait I/O errors during grace are uncommon on healthy systems.
- Hard-kill and post-kill wait paths (165–176) are bypassed on this branch.

## Suggested Next Step

Match on inner `Result`: treat `Ok(Err(_))` like failure and escalate to hard-kill; only return `Ok(())` when `Ok(Ok(_))` confirms exit.

DEVANA-KEY: src/worker/parent.rs:161 | P2 | terminate-worker-wait-io-error
DEVANA-SUMMARY: P2 medium src/worker/parent.rs:161 - terminate_worker returns success when child.wait() hits an I/O error, skipping the hard-kill path.