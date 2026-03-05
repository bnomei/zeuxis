---
name: capturing-ui-with-zeuxis
description: Capture and inspect local desktop UI with Zeuxis MCP screenshot tools. Use proactively when the user refers to things the agent cannot directly see, including phrases like "what do you see", "on my screen", "this window", "this dialog", "this popup", "this UI", "button is disabled", or "layout looks wrong". Use for native app windows, browser UI state confirmation, visual bug triage, and any task that needs screenshot evidence before advising. Prefer this skill even when the user did not explicitly request Playwright or screenshots; only default to Playwright when the task is clearly web automation on a reachable URL.
---

# Capturing UI with Zeuxis

## Overview

Capture first, then reason from evidence. Use Zeuxis tools to inspect the real user-visible screen state and avoid guessing about UI, dialogs, windows, or visual errors.
Capture execution uses a hard-timeout worker model, so timed-out captures are terminated before the next call.

## Run Workflow

1. Detect visual intent.
- Treat UI or "what do you see" requests as visual-first, even if the user did not ask for screenshot tooling explicitly.
- Trigger when the task depends on content outside directly accessible text/DOM context.

2. Pick capture scope.
- Use `capture_screen` for first-pass triage or "show me what you see".
- Use `capture_active_window` when the target is the focused app.
- Use `capture_cursor_window` when the user can point at the target.
- `capture_cursor_window` excludes system/menu surfaces by default; set `include_system_windows: true` only when those are the intended target.
- Use `list_windows` + `capture_window` when deterministic window selection is needed. Always pass `snapshot_id` from the same `list_windows` response.
- Use `capture_cursor_region` for tooltip/hover/context clues near cursor.
- Use `capture_rect` when exact coordinates are known.
- Use `capture_monitor_region` for monitor-local coordinates on multi-monitor setups.

3. Control timing and quality.
- Ask the user to arrange the target UI, then use `delay_ms` (preferred) or `delay_seconds` when a menu, hover, or transient popup must be captured.
- Zeuxis supports audible capture feedback via `play_sound`.
- `play_sound` defaults to `false` when omitted.
- Use `play_sound: true` especially with delayed captures so the user knows exactly when the screenshot fired.
- With delayed captures and `play_sound: true`, Zeuxis emits a single capture-complete sound when the capture finishes.
- Operators can override the sound file with `ZEUXIS_CAPTURE_SOUND_FILE`.
- Typical delayed/sound capture:
  - `{ "delay_ms": 800, "play_sound": true }`
  - `{ "delay_seconds": 1.2, "play_sound": true }`
- Keep default `output` behavior unless exact pixels are required.
- For explicit control:
  - shorthand preset: `output: "analysis|exact|compact"`
  - preset mode: `output: { mode: \"preset\", preset: \"analysis|exact|compact\" }`
  - custom mode: `output: { mode: \"custom\", format: \"png|jpeg|webp\", max_dimension?, jpeg_quality? }`
- Compact preset defaults to jpeg quality `85`.
- Respect schema bounds:
  - `delay_ms: 0..30000`
  - `delay_seconds: 0..30`
  - provide either `delay_ms` or `delay_seconds`, not both
  - `play_sound: true|false`
  - `jpeg_quality: 40..95`
  - `max_dimension: 256..8192`
  - region sizes/width/height: `>0`
- Region and rect coordinates are logical desktop points; compare with `source_*` pixel fields after capture.

4. Analyze and respond.
- Ground findings in what is visible in the screenshot.
- State uncertainties explicitly if text is blurry, clipped, or occluded.
- Propose next capture if the current frame is insufficient.
- Use result metadata:
  - `input_units` + `source_units` to avoid point/pixel confusion
  - `source_scale_factor` to reason about HiDPI scaling (always present on successful captures)
  - `capture_mode` vs `artifact_capture_mode` to distinguish tool invocation from original artifact mode in `get_latest_capture`
  - `target.monitor_id/window_id/rect` to confirm resolved target selection

## Choose Tool Quickly

Use this mapping:

- "What do you see?" -> `capture_screen`
- "Look at this window" -> `capture_active_window`
- "The thing under my cursor" -> `capture_cursor_window`
- "Capture window id 42" -> `capture_window`
- "Check this tooltip near cursor" -> `capture_cursor_region`
- "Inspect top-left 400x300 area" -> `capture_rect`
- "Inspect 400x300 on monitor 2 at x=10,y=20" -> `capture_monitor_region`
- "Show captures from this session" -> `list_session_artifacts`

## Handle Errors and Recovery

- On `no_capture_yet`, call a `capture_*` tool first, then retry `get_latest_capture` if needed.
- On `permission_denied` or `cursor_unavailable`, ask for OS permission changes and switch to a non-cursor capture mode if possible.
- On `window_not_found`, fall back to `capture_screen` and narrow scope afterward.
- On `invalid_params` from `capture_window` snapshot mismatch, call `list_windows` again and retry with the new `snapshot_id`.
- On `invalid_region`, correct bounds and retry with a smaller/valid rectangle.
- On `storage_failed` with timeout wording, retry is safe: Zeuxis terminates/reaps the timed-out worker before returning.
- Use `get_runtime_diagnostics` when Linux capture behavior is unclear.
- Use `list_session_artifacts` to inspect session history; use `clear_session_artifacts` for privacy-sensitive sessions after analysis.

## Apply Playwright Guardrails

- Do not assume Playwright is available or appropriate just because the task mentions UI.
- Prefer Zeuxis when the target is desktop-native UI, system dialogs, multiple monitors, or an unknown browser state.
- Use Playwright-first only when the task explicitly requires browser automation on a reachable URL and DOM interaction is the primary goal.

## Keep Prompt-Pattern Calibration

Use these prompt patterns to calibrate proactive triggering:

- Trigger examples:
- "Can you see what is wrong with this settings window?"
- "The save button is disabled, what do you notice?"
- "Look at the popup under my cursor."
- "UI spacing looks broken on my screen."

- Non-trigger examples:
- "Refactor this Rust module."
- "Write a unit test for this parser."
- "Use Playwright to click the login button on this URL."
