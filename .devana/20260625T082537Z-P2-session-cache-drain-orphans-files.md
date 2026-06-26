DEVANA-FINDING: v1
Priority: P2 | Confidence: medium | Security-sensitive: no | Status: open
Location: src/storage/mod.rs:346 | Slug: session-cache-drain-orphans-files

# Session artifact cache eviction drops entries without deleting files

## Finding

When `session_artifacts.len() > max_artifacts`, `finalize_artifact` calls `artifacts.drain(0..overflow)` to trim the in-memory cache. This removes metadata only; no `fs::remove_file` runs for drained entries. If directory pruning previously failed to delete the same file, the artifact file remains on disk but disappears from `list_session_artifacts` and `clear_session_artifacts`.

## Violated Invariant Or Contract

Session retention and `clear_session_artifacts` should remove all screenshot files created in the session. README/skills describe privacy-oriented session cleanup.

## Oracle

`storage_retention_keeps_candidate_when_delete_fails` shows prune can leave files on disk when deletion fails. `clear_session_artifacts` only deletes paths still present in the session cache (`src/storage/mod.rs:275–300).

## Counterexample

`max_artifacts = 2`. Captures A, B, C succeed. Prune fails to delete A (e.g. read-only parent directory). `finalize_artifact` for C: `drain(0..1)` drops A from cache; cache is `[B, C]`. `clear_session_artifacts` deletes B and C only. File A remains on disk, untracked.

## Why It Might Matter

Orphaned screenshot files leak session captures past explicit cleanup, undermining retention and privacy expectations.

## Proof

**Control-flow trace:** `prune_artifacts` (may fail silently per retention tests) → `artifacts.push` → `drain(0..overflow)` mutates `Vec<StoredArtifact>` only (`src/storage/mod.rs:343–349`) → `clear_session_artifacts` iterates drained cache, not filesystem scan.

**State transition mismatch:** In-memory session state shrinks while on-disk artifact count may not.

## Counterevidence Checked

- When prune succeeds in a single artifact directory (normal happy path), drained cache entries usually correspond to pruned files.
- `list_session_artifacts` `retain`s by `path.exists()` but does not rediscover orphaned files.
- Subprocess staging uses a separate `zeuxis-worker-artifacts` dir; mixed paths increase divergence risk.

## Suggested Next Step

Delete files for entries removed by `drain`, or only evict cache entries after confirmed `remove_file` success; align cache eviction with `prune_artifacts` outcomes.

DEVANA-KEY: src/storage/mod.rs:346 | P2 | session-cache-drain-orphans-files
DEVANA-SUMMARY: P2 medium src/storage/mod.rs:346 - Session cache drain evicts artifact metadata without deleting files, so clear_session_artifacts can miss orphaned screenshots.