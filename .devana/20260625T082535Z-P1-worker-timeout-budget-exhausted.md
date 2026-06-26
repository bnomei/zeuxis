DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: open
Location: src/mcp/tools.rs:1078 | Slug: worker-timeout-budget-exhausted

# Subprocess capture timeout budget is consumed by spawn/IPC overhead, rejecting near-limit successes

## Finding

In `SubprocessWorker` mode, `worker_timeout` is assigned entirely to `run_worker_capture`, but the outer `deadline` is set to `now + worker_timeout` before spawn/IPC work begins. `run_worker_capture` only bounds `child.wait()`; spawn, stdin write, and stdout drain occur outside that inner timeout yet count against the outer deadline. A worker that succeeds after using most of the wait budget is rejected at the post-worker `now > deadline` check, or starves adoption with ~0 remaining time.

## Violated Invariant Or Contract

C002: parent shall enforce the configured timeout budget. C009: successful capture shall return artifact path/uri. Design §4 step 3: "await worker completion using remaining timeout budget" — the budget must cover worker **and** adoption end-to-end.

## Oracle

`specs/mcp-subprocess-capture-hard-timeout/requirements.md` C002, C009; design §4–5 (`specs/mcp-subprocess-capture-hard-timeout/design.md:76–94`). README: "capture timeout is enforced in the parent process" with default 15000 ms.

## Counterexample

`blocking_task_timeout_ms = 15000`. Permit acquire is fast, so `worker_timeout ≈ 15s`. `deadline = T₀ + 15s`. Worker spawn/IPC takes 100 ms; child runs 14.9 s and exits 0 with a valid artifact. Total elapsed from deadline anchor ≈ 15.0s + drain > `deadline` → line 1174 rejects with `storage_failed` and deletes the artifact. Even when post-check barely passes, `remaining = deadline.saturating_duration_since(now) ≈ 0` causes `adopt_artifact` to time out immediately (lines 1180–1193).

## Why It Might Matter

Slow captures (large monitors, busy systems, low timeout settings) fail despite a successful worker, returning `storage_failed` and deleting valid screenshots. Violates the hard-timeout spec's goal of reliable success semantics within the budget.

## Proof

**Control-flow trace:** `deadline = Instant::now() + worker_timeout` (1078) → `run_worker_capture(..., worker_timeout)` bounds only `child.wait()` (`src/worker/parent.rs:54`) → post-check `if now > deadline` (1173–1178) → else `remaining = deadline.saturating_duration_since(now)` for adopt (1180–1193).

**Counterexample value:** `worker_timeout = W`, spawn+drain overhead `ε > 0`, `child_runtime = W − ε` → total `> W` → post-check fails.

## Counterevidence Checked

- Fast captures (typical sub-second) leave enough slack that the bug is latent in common use.
- Inline mode checks deadline inside a single blocking job (1089–1107); only `SubprocessWorker` has the split-budget problem.
- No reserved adoption slice is subtracted from `worker_timeout`.

## Suggested Next Step

Anchor one end-to-end deadline at permit acquisition (or subtract measured spawn/IPC from worker wait budget) and reserve time for adoption; remove or relax the redundant post-worker deadline check that double-counts overhead.

DEVANA-KEY: src/mcp/tools.rs:1078 | P1 | worker-timeout-budget-exhausted
DEVANA-SUMMARY: P1 high src/mcp/tools.rs:1078 - Subprocess captures near the timeout limit fail storage_failed despite worker success because spawn/IPC overhead is not accounted for in the worker wait budget.