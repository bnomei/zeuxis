DEVANA-FINDING: v1
Priority: P2 | Confidence: high | Security-sensitive: no | Status: fixed
Location: src/mcp/tools.rs:993 | Slug: delay-outside-timeout-budget

# Configured capture delay runs before and outside blocking_task_timeout_ms

## Finding

`execute_capture` applies `tokio::time::sleep(delay)` (lines 993–995) before acquiring the capture slot and starting the blocking-phase timeout budget (line 1018). A maximum `delay_ms` of 30000 ms can add 30 seconds of wall time that is not counted against `blocking_task_timeout_ms`.

## Violated Invariant Or Contract

README table describes `--blocking-task-timeout-ms` as "Overall capture deadline before timeout/worker termination." Runtime safety limits list both `delay_ms` max 30000 and capture timeout 15000 ms default without stating they stack.

## Oracle

`README.md:115–121` (delay max 30000) and `README.md:138` ("Overall capture deadline"); `MAX_DELAY_MILLISECONDS = 30000` in tools; blocking budget starts at `blocking_phase_started` (1018), after delay.

## Counterexample

`blocking_task_timeout_ms = 1000`, `delay_ms = 30000`. Client waits ~31 seconds total before timeout or result. Timeout enforcement only applies to the ~1 second blocking phase after the 30 second delay.

## Why It Might Matter

Agents expecting a 15-second overall cap can block for up to 45 seconds (30s delay + 15s capture). MCP clients with their own RPC timeouts may disconnect while Zeuxis is still in the delay phase.

## Proof

**Control-flow trace:** `parse_common_params` validates delay ≤ 30000 → `tokio::time::sleep(delay).await` (993–995) → `blocking_phase_started = Instant::now()` (1018) → timeout applies only from there.

**Contract mismatch:** README "overall" wording vs implementation where delay is excluded.

## Counterevidence Checked

- Env var name `BLOCKING_TASK_TIMEOUT_MS` suggests blocking-phase-only intent, but README table explicitly says "Overall."
- Delay is a documented feature; the bug is the unstated stacking semantics, not delay itself.
- No code subtracts delay from the timeout budget.

## Suggested Next Step

Either subtract applied delay from `blocking_task_timeout_ms`, or update README to state delay is additive and not bounded by the capture timeout.

## Status Notes

- 2026-06-26: fixed (documentation). Confirmed: `tokio::time::sleep(delay)` (`src/mcp/tools.rs:993-995`) runs before `blocking_phase_started = Instant::now()` (1018) and slot acquisition, so the configured delay (up to 30s) is excluded from the capture timeout budget. Resolved via docs rather than code: the timeout is deliberately scoped to the capture phase (variable `blocking_phase_started`, env `BLOCKING_TASK_TIMEOUT_MS`) and the delay is an explicit user-requested pre-capture wait — subtracting a large delay from the capture budget would cripple the feature (e.g. delay=14s + timeout=15s would near-always fail). Updated README: the `--blocking-task-timeout-ms` row now states the deadline is measured from the start of capture-slot acquisition (after any delay), and Runtime Safety Limits now states the delay is applied first and is additive to the capture timeout (total wall time up to `delay + timeout`), advising clients to size RPC timeouts accordingly. No behavior change.

DEVANA-KEY: src/mcp/tools.rs:993 | P2 | delay-outside-timeout-budget
DEVANA-SUMMARY: Status=fixed | P2 high src/mcp/tools.rs:993 - Documented (not code-changed): delay_ms/delay_seconds is an intentional pre-capture wait, additive to and not bounded by the capture timeout; README clarified that timeout covers only the capture phase and total wall time can be delay+timeout.