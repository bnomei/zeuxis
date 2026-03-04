---
name: capturing-ui-with-zeuxis
description: Capture and inspect local desktop UI with Zeuxis MCP screenshot tools. Use proactively when the user refers to things the agent cannot directly see, including phrases like "what do you see", "on my screen", "this window", "this dialog", "this popup", "this UI", "button is disabled", or "layout looks wrong". Use for native app windows, browser UI state confirmation, visual bug triage, and any task that needs screenshot evidence before advising. Prefer this skill even when the user did not explicitly request Playwright or screenshots; only default to Playwright when the task is clearly web automation on a reachable URL.
---

# Capturing UI with Zeuxis

## Overview

Capture first, then reason from evidence. Use Zeuxis tools to inspect the real user-visible screen state and avoid guessing about UI, dialogs, windows, or visual errors.

## Run Workflow

1. Detect visual intent.
- Treat UI or "what do you see" requests as visual-first, even if the user did not ask for screenshot tooling explicitly.
- Trigger when the task depends on content outside directly accessible text/DOM context.

2. Pick capture scope.
- Use `capture_screen` for first-pass triage or "show me what you see".
- Use `capture_active_window` when the target is the focused app.
- Use `capture_window_at_cursor` when the user can point at the target.
- Use `capture_cursor_region` for tooltip/hover/context clues near cursor.
- Use `capture_rect` when exact coordinates are known.

3. Control timing and quality.
- Ask the user to arrange the target UI, then use `delay_seconds` when a menu, hover, or transient popup must be captured.
- Keep default `output_preset: analysis` unless exact pixels are required.

4. Analyze and respond.
- Ground findings in what is visible in the screenshot.
- State uncertainties explicitly if text is blurry, clipped, or occluded.
- Propose next capture if the current frame is insufficient.

## Choose Tool Quickly

Use this mapping:

- "What do you see?" -> `capture_screen`
- "Look at this window" -> `capture_active_window`
- "The thing under my cursor" -> `capture_window_at_cursor`
- "Check this tooltip near cursor" -> `capture_cursor_region`
- "Inspect top-left 400x300 area" -> `capture_rect`

## Handle Errors and Recovery

- On `no_capture_yet`, call a `capture_*` tool first, then retry `get_latest_screenshot` if needed.
- On `permission_denied` or `cursor_unavailable`, ask for OS permission changes and switch to a non-cursor capture mode if possible.
- On `window_not_found`, fall back to `capture_screen` and narrow scope afterward.
- On `invalid_region`, correct bounds and retry with a smaller/valid rectangle.

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
