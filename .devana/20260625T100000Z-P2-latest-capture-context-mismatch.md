DEVANA-FINDING: v1
Priority: P2 | Confidence: medium | Security-sensitive: no | Status: open
Location: src/mcp/tools.rs:599 | Slug: latest-capture-context-mismatch

# get_latest_capture pairs the latest artifact with a different capture's context

## Finding

`get_latest_capture` builds its response from two independently-locked singletons that
are updated at different points in a capture's lifecycle:

- the artifact comes from `storage.latest_artifact()` (`src/mcp/tools.rs:547`), whose
  backing `latest_artifact_cache` is set **inside** the blocking capture task at
  `src/storage/mod.rs:351-353`.
- the context comes from `self.last_capture_context` (`src/mcp/tools.rs:565`), which is
  set **after** the blocking task completes and the capture permit is dropped, at
  `src/mcp/tools.rs:1250-1252`.

These two updates are not atomic and are guarded by two different locks. When two
captures run concurrently (`max_concurrent_captures >= 2`), the "latest artifact" and
the "latest context" can originate from different captures. `success_result`
(`src/mcp/result.rs`) then takes `width`/`height`/`path`/`artifact_sha256` from one
capture's artifact and `target`/`source_width`/`source_height`/`input_width`/
`input_height`/`source_scale_factor` from another capture's context.

## Violated Invariant Or Contract

Every field in a single `CaptureSuccessPayload` must describe the same capture. In
particular `source_scale_factor` is derived as `source_width / input_width`
(`src/mcp/result.rs`), and `target.rect` is expected to describe the artifact actually
referenced by `path`/`uri`/`artifact_sha256`.

## Oracle

The normal capture path (`src/mcp/tools.rs:1240-1274`) constructs the result from a
single locally-built `context` paired with that same capture's artifact, establishing
that artifact and context are meant to be coherent. README also documents
`source_scale_factor` and the artifact metadata as describing one capture.

## Counterexample

`max_concurrent_captures = 2`. A client fires two tool calls concurrently:

- Call A: `capture_rect{x:0,y:0,width:100,height:100}` (input_width=100, target rect 100x100, global).
- Call B: `capture_screen` on a 2560x1440 monitor.

Interleaving: B's blocking task writes its artifact into `latest_artifact_cache` last,
so storage latest = B's full-screen PNG (2560x1440). A's task returns from its `await`
last, so `last_capture_context` = A's context (input_width=100, target 100x100).

`get_latest_capture` now returns B's artifact (path/hash, width=2560, height=1440)
merged with A's context, yielding `source_scale_factor = 2560 / 100 = 25.6` and a
`coordinate_space:"global"` target rect of 100x100 that does not describe the
full-screen image at all.

## Why It Might Matter

`get_latest_capture` can hand a client an artifact link whose reported geometry,
coordinate space, and scale factor belong to an unrelated screenshot. An agent reasoning
over those numbers (e.g. to map a detected element back to screen coordinates) would
compute wrong positions. Confined to `get_latest_capture`; single-shot capture responses
are unaffected.

## Proof

State/concurrency interleaving + dataflow:
- `src/storage/mod.rs:351-353`: latest artifact set inside the permit-held blocking task.
- `src/mcp/tools.rs:1250-1252`: context set after the task completes and permit drops.
- `src/mcp/tools.rs:547` and `:565`: `get_latest_capture` reads the two singletons
  independently, with no shared lock or generation token tying them together.
- `src/mcp/tools.rs:599-603`: merges them via `success_result`.

## Counterevidence Checked

- The normal capture path builds the result from the local `context`
  (`src/mcp/tools.rs:1274`), not the shared lock, so single captures stay coherent — the
  divergence only surfaces through `get_latest_capture`.
- With `max_concurrent_captures == 1` it cannot trigger, but that value is configurable
  (range `1..=16`, default `2`), so the multi-capture case is the default.
- Distinct from the already-filed `cursor-window-fallback-metadata` finding (that is the
  `unwrap_or_else` default-context branch at `:570`); here both singletons are populated
  but from different captures.

## Suggested Next Step

Tie the artifact and its context together as one atomic unit (e.g. store the
`CaptureContextPayload` alongside the artifact in a single cache entry updated under one
lock), so `get_latest_capture` retrieves a matched (artifact, context) pair instead of
two independently-latched singletons.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...`
and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`,
`duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.

DEVANA-KEY: src/mcp/tools.rs:599 | P2 | latest-capture-context-mismatch
DEVANA-SUMMARY: Status=open | P2 medium src/mcp/tools.rs:599 - get_latest_capture merges the latest artifact with last_capture_context from two unsynchronized singletons, so under concurrent captures it returns one capture's image with another capture's geometry/scale metadata.
