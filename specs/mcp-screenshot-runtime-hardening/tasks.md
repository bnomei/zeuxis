# Tasks — mcp-screenshot-runtime-hardening

Meta:
- Spec: `mcp-screenshot-runtime-hardening` — Runtime timeout, diagnostics, and observability hardening
- Depends on: `mcp-screenshot-server`
- Global scope:
  - `src/mcp/tools.rs`
  - `src/mcp/server.rs`
  - `src/mcp/result.rs`
  - `src/runtime_config.rs`
  - `src/storage/mod.rs`
  - `tests/tool_calls.rs`
  - `tests/result_payload.rs`
  - `specs/mcp-screenshot-runtime-hardening/**`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Implement capture-timeout budget split across slot acquisition and worker execution (owner: mayor)
  - Scope: `src/mcp/tools.rs`, `tests/tool_calls.rs`
  - Depends: -
  - DoD: Capture path enforces timeout on semaphore acquisition and remaining worker budget.
  - Validation: `cargo test --all-targets`

- [x] T002: Consolidate blocking timeout/join error behavior with a shared helper (owner: mayor)
  - Scope: `src/mcp/tools.rs`
  - Depends: T001
  - DoD: Blocking timeout/join mapping reused across monitor listing, diagnostics probe, latest lookup, and capture worker.
  - Validation: `cargo test --all-targets`

- [x] T003: Extend runtime diagnostics contract with permission-check metadata (owner: mayor)
  - Scope: `src/mcp/tools.rs`, `src/mcp/result.rs`, `tests/result_payload.rs`, `tests/tool_calls.rs`
  - Depends: -
  - DoD: `permission_checked` and `permission_check_mode` are present and platform-mapped.
  - Validation: `cargo test --all-targets`

- [x] T004: Add runtime config warning fallbacks and timeout normalization in constructors (owner: mayor)
  - Scope: `src/runtime_config.rs`, `src/mcp/server.rs`
  - Depends: -
  - DoD: invalid/out-of-range env values warn and fall back; constructor timeout inputs clamp to policy bounds.
  - Validation: `cargo test --all-targets`

- [x] T005: Add retention prune warning path and coverage-focused regression tests (owner: mayor)
  - Scope: `src/storage/mod.rs`, `src/mcp/server.rs`, `src/runtime_config.rs`
  - Depends: -
  - DoD: prune delete failure logs warning without failing capture; additional tests cover newly added branches.
  - Validation: `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets`, `cargo llvm-cov --summary-only`
