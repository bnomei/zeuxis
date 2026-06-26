DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: invalid
Location: src/mcp/tools.rs:1862 | Slug: downscale-squashes-aspect-ratio

# Analysis preset downscale squashes non-square captures to a square

## Finding

`downscale_if_needed` resizes every oversized capture to `max_dimension × max_dimension`, forcing a square output. Non-square source images (typical widescreen monitors) are stretched rather than proportionally scaled. The default `analysis` output preset applies `max_dimension: 2560` on every capture unless the client opts out.

## Violated Invariant Or Contract

Downscaling must preserve capture geometry. README describes "moderate downscaling" for analysis/compact presets; clients rely on `source_scale_factor` and artifact dimensions to reason about on-screen layout.

## Oracle

`default_output_settings(OutputPreset::Analysis)` sets `max_dimension: Some(2560)` (`src/mcp/tools.rs:1818–1824`). Unit test `mcp_tools_downscale_if_needed_scales_only_when_required` only asserts `width/height <= max_dimension`, not aspect-ratio preservation (`src/mcp/tools.rs:2310–2319`). The same logic is duplicated in `src/worker/child.rs:278–294`.

## Counterexample

A 3840×2160 (16:9) screen capture with default `output: "analysis"` (or explicit `max_dimension: 2560`) passes the early-exit guard (`max(3840,2160) > 2560`) and is resized to 2560×2560. Text and UI appear vertically stretched. `source_width`/`source_height` in the response still report 3840×2160 while the artifact is 2560×2560.

## Why It Might Matter

Agents use analysis-preset captures for vision reasoning. Distorted aspect ratios misrepresent UI layout, break coordinate mapping, and undermine trust in `source_scale_factor` metadata.

## Proof

**Counterexample value:** 3840×2160 input, `max_dimension = 2560`.

**Control-flow trace:** `parse_common_params` → `default_output_settings(Analysis)` → worker/inline capture → `downscale_if_needed` → `DynamicImage::resize(2560, 2560, …)` (`src/mcp/tools.rs:1868–1873`, `src/worker/child.rs:288–293`).

## Counterevidence Checked

- `image::imageops::thumbnail` and proportional resize helpers are not used anywhere in `src/`.
- `OutputPreset::Exact` skips downscale (`max_dimension: None`); bug affects analysis, compact, and custom `max_dimension` paths.
- Early-exit only compares `max(width,height)` to `max_dimension`; it never computes proportional `(new_w, new_h)`.

## Suggested Next Step

Replace square `resize(max, max)` with proportional scaling (e.g. scale longest edge to `max_dimension`, compute the other dimension by ratio) in both `src/mcp/tools.rs` and `src/worker/child.rs`, and add a test asserting aspect-ratio preservation.

## Status Notes

- 2026-06-26: invalid. The finding misreads `image` 0.25 semantics. `DynamicImage::resize(max, max, filter)` does NOT force a square — it scales the image to the largest size that fits *within* `max × max` while preserving aspect ratio (only `resize_exact` squashes). Verified empirically: a 3840×2160 (16:9) input with `max_dimension = 2560` resizes to 2560×1440, not 2560×2560. Added a regression assertion to `mcp_tools_downscale_if_needed_scales_only_when_required` pinning aspect-ratio preservation (3840×2160 → 2560×1440); test passes. Same conclusion applies to the duplicated logic in `src/worker/child.rs`. No code fix needed.

DEVANA-KEY: src/mcp/tools.rs:1862 | P1 | downscale-squashes-aspect-ratio
DEVANA-SUMMARY: Status=invalid | P1 high src/mcp/tools.rs:1862 - INVALID: DynamicImage::resize preserves aspect ratio (fits within bounds); 3840x2160 -> 2560x1440, not a square. Regression test added.