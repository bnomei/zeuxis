DEVANA-FINDING: v1
Priority: P1 | Confidence: high | Security-sensitive: no | Status: open
Location: src/mcp/tools.rs:1314 | Slug: artifact-dir-ignored-subprocess

# --artifact-dir is ignored in default SubprocessWorker capture path

## Finding

Production servers use `CaptureExecutionMode::SubprocessWorker` (`src/mcp/server.rs:196`). Worker artifacts are always staged under `std::env::temp_dir().join("zeuxis-worker-artifacts")` via `create_worker_artifact_path`, regardless of `RuntimeConfig.artifact_dir`. `adopt_artifact` keeps files at the worker path; `TempPngStorage::write_image` (which respects `artifact_dir`) is never called on this path.

## Violated Invariant Or Contract

README configuration table: `--artifact-dir` / `ZEUXIS_ARTIFACT_DIR` is "Directory for managed capture artifacts" with precedence `CLI flag > env var > default`.

## Oracle

`README.md:137`; `main.rs:107` passes `cli.artifact_dir` into `RuntimeConfig`; `TempPngStorage::with_settings(..., config.artifact_dir, ...)` at `src/mcp/server.rs:187–191`.

## Counterexample

Start Zeuxis with `--artifact-dir=/custom/captures`. Call `capture_screen`. Artifact is written to `$TMPDIR/zeuxis-worker-artifacts/zeuxis-capture_screen-*.png`, not `/custom/captures/`. `list_session_artifacts` and `clear_session_artifacts` operate on the worker staging path.

## Why It Might Matter

Operators who set a dedicated artifact directory (compliance, disk layout, cleanup policies) silently get default temp-dir behavior. Session cleanup and retention policies may not apply where users expect.

## Proof

**Dataflow trace:** `with_runtime_config` → `SubprocessWorker` → `execute_capture` subprocess branch → `create_worker_artifact_path` hardcodes `temp_dir/zeuxis-worker-artifacts` (1314–1318) → `adopt_artifact` in place (1179–1191). `artifact_dir` only reaches `write_image` in `Inline` mode (1109).

**Contract mismatch:** README/config promise vs `create_worker_artifact_path` implementation.

## Counterevidence Checked

- `Inline` mode (`with_components`, tests) does use `storage.write_image` and honors `artifact_dir`.
- Default binary path is `SubprocessWorker`, not `Inline`.
- `auto_managed_artifact_dir` chmod hardening applies to `TempPngStorage.artifact_dir`, not the hardcoded worker staging dir.

## Suggested Next Step

Stage worker artifacts under `config.artifact_dir` (or a subdirectory thereof) in `create_worker_artifact_path`, and add an integration test asserting `--artifact-dir` is honored in production mode.

DEVANA-KEY: src/mcp/tools.rs:1314 | P1 | artifact-dir-ignored-subprocess
DEVANA-SUMMARY: P1 high src/mcp/tools.rs:1314 - Default subprocess capture ignores --artifact-dir and always writes to temp_dir/zeuxis-worker-artifacts.