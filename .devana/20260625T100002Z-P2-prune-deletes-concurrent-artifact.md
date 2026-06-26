DEVANA-FINDING: v1
Priority: P2 | Confidence: medium | Security-sensitive: no | Status: fixed
Location: src/storage/mod.rs:316 | Slug: prune-deletes-concurrent-artifact

# Retention prune deletes a concurrent capture's freshly written artifact

## Finding

`finalize_artifact` calls `prune_artifacts(&path, self.retention_policy)`
(`src/storage/mod.rs:316`) while holding no lock. `prune_artifacts_in_dir`
(`src/storage/mod.rs:549-580`) scans the whole artifact directory and deletes the oldest
files until the count/byte budget is satisfied, excluding only its own `current_path`
(`:560-566`). It does not protect the in-flight artifacts of other captures running
concurrently in the same directory. A capture that returns `Ok(StoredArtifact)` can
therefore reference a file another capture's prune just deleted.

## Violated Invariant Or Contract

A capture that returns `Ok(artifact)` must reference a file that exists on disk at return
time, and "pruning ... never deletes the current artifact being returned" (README). The
prune protects only the *caller's* `current_path`, not a sibling capture's just-written,
still-to-be-returned artifact in the same directory.

## Oracle

README "Temp artifact retention": "older artifacts are pruned on each successful write"
and "never deletes the current artifact being returned". The returned `StoredArtifact`
carries `uri`/`path` that the MCP client is expected to open.

## Counterexample

Configure `max_concurrent_captures = 2` and `max_artifacts = 1` (both within their
documented ranges: concurrency `1..=16`, artifacts `1..=10000`). All captures in one
server session share a single artifact directory (`default_artifact_dir()` is
per-session). Fire two capture tool calls concurrently:

1. Capture A and capture B each write their file into the shared session dir.
2. A enters `finalize_artifact`, computes integrity for file A, then
   `prune_artifacts(&pathA)`: with `max_artifacts = 1` and two files present, it deletes
   the oldest file that is not `pathA` — i.e. **file B**.
3. B (slightly behind) runs its own `finalize_artifact`: it already computed integrity
   for file B at `:313-314`, sets `latest = B` (`:351`), pushes B into
   `session_artifacts` (`:345`), and returns `Ok(artifact_B)` — but file B was already
   deleted by A.
4. The MCP success response for capture B advertises a `uri`/`path` to a file that no
   longer exists.

## Why It Might Matter

A successful capture call returns a `file://` link to a missing file; the client opens it
and gets nothing. The in-memory getters self-heal, but the immediate capture result does
not, so the failure is delivered as a success. Requires concurrency greater than
`max_artifacts`, which is reachable with default-or-stricter retention.

## Proof

Concurrency / lost-update on a shared filesystem namespace not covered by any lock:
- `src/storage/mod.rs:316`: `prune_artifacts` runs with no lock held.
- `src/storage/mod.rs:560-566`: prune's only exclusion is `entry.path != current_path`,
  so any concurrent capture's file is a deletion candidate.
- `src/storage/mod.rs:567`: `fs::remove_file` unlinks the victim.
- `src/storage/mod.rs:343-353`: the in-memory caches are updated under separate
  short-lived locks *after* the prune; the returned artifact (caller in
  `src/mcp/tools.rs`) is handed back without an `.exists()` check.

## Counterevidence Checked

- `latest_artifact()` (`:253`) clears the cache on a missing file and
  `list_session_artifacts()` (`:269`) retains-by-existence, so those two getters
  self-heal — but the immediate capture result returned to the caller is not guarded.
- Single-capture sessions and `max_artifacts >= max_concurrent_captures` do not trigger
  it; the race needs concurrency strictly greater than the retained count.
- Distinct from the already-filed `session-cache-drain-orphans-files` finding (that is
  about cache drain leaving files on disk; this is about a file being deleted out from
  under a returning capture).

## Suggested Next Step

Serialize prune against concurrent finalize (e.g. take a storage-wide lock around
integrity-compute + prune + cache-update), or have prune protect every artifact that is
currently being finalized — not just its own `current_path` — so a sibling capture's
just-written file is never a deletion victim.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `Status: ...`
and the final `DEVANA-SUMMARY:` status. Use one of: `open`, `fixed`, `invalid`, `stale`,
`duplicate`, `wontfix`. Add dated notes below with the evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-26: fixed. Confirmed: `prune_artifacts_in_dir` excluded only its own `current_path`, so a concurrent capture's just-written file in the shared dir was a deletion candidate; with concurrency > `max_artifacts` one capture's prune could delete a sibling's file, and the sibling then returned `Ok` pointing to a missing path. (Note this race now also applies to worker mode after the artifact-dir-ignored-subprocess fix co-located worker artifacts in the storage dir.) Fix: added an in-flight registry to `TempPngStorage` — `in_flight: Mutex<HashSet<PathBuf>>`. `write_image` registers the artifact path immediately after `create_temp_file` (which already created the file on disk under its final, kept name); `adopt_artifact` registers at entry; both hold an RAII `InFlightGuard` that unregisters on drop after `finalize_artifact` returns. `finalize_artifact` passes a snapshot of the in-flight set to `prune_artifacts`, which now skips any candidate whose path is `current_path` **or** in the protected set. So no prune deletes a file that any capture is mid-finalize on, and every returned `Ok` references a live file at return time; the protected files are reclaimed by retention on later prunes once their captures have returned. Worst-case residual in worker mode (a prune in the narrow window between the worker writing its file and the parent entering `adopt_artifact`) yields a clean error for the losing capture, never a dangling success. Added regression test `storage_retention_protects_in_flight_concurrent_artifact`; updated the four existing prune tests for the new parameter. Full lib (142) + integration suites pass.

DEVANA-KEY: src/storage/mod.rs:316 | P2 | prune-deletes-concurrent-artifact
DEVANA-SUMMARY: Status=fixed | P2 medium src/storage/mod.rs:316 - Retention prune now skips an in-flight registry of artifacts that are mid-finalize (registered at file creation / adopt entry, released after finalize), so a concurrent capture's prune can no longer delete a sibling's just-written file and every returned Ok references a live file.
