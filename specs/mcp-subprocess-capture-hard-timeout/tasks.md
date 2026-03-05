# Tasks — mcp-subprocess-capture-hard-timeout

Meta:
- Spec: `mcp-subprocess-capture-hard-timeout` — Reliable hard timeout via subprocess capture workers
- Depends on:
  - `mcp-screenshot-server`
  - `mcp-screenshot-runtime-hardening`
- Global scope:
  - `src/main.rs`
  - `src/mcp/tools.rs`
  - `src/mcp/server.rs`
  - `src/mcp/errors.rs`
  - `src/runtime_config.rs`
  - `src/storage/mod.rs`
  - `src/worker/**`
  - `tests/tool_calls.rs`
  - `tests/support/mod.rs`
  - `tests/result_payload.rs`
  - `README.md`
  - `skills/capturing-ui-with-zeuxis/SKILL.md`
  - `specs/mcp-subprocess-capture-hard-timeout/**`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Add worker IPC contract module (owner: mayor)
  - Scope: `src/worker/contract.rs`, `src/worker/mod.rs`, `src/lib.rs`
  - Validation: `cargo test --all-targets`

- [x] T002: Add hidden CLI worker mode entrypoint (owner: mayor)
  - Scope: `src/main.rs`, `src/worker/child.rs`, `src/worker/mod.rs`
  - Validation: `cargo test --all-targets`

- [x] T003: Introduce runtime config knobs for worker timeout termination (owner: mayor)
  - Scope: `src/runtime_config.rs`, `src/main.rs`, `src/mcp/server.rs`
  - Validation: `cargo test --all-targets`

- [x] T004: Implement parent worker runner with hard-timeout kill (owner: mayor)
  - Scope: `src/worker/parent.rs`, `src/worker/mod.rs`, `src/mcp/tools.rs`
  - Validation: `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets`

- [x] T005: Add storage artifact adoption API for worker-produced files (owner: mayor)
  - Scope: `src/storage/mod.rs`
  - Validation: `cargo test --all-targets`

- [x] T006: Integrate worker execution path into all capture tools (owner: mayor)
  - Scope: `src/mcp/tools.rs`, `src/mcp/server.rs`
  - Validation: `cargo test --all-targets`

- [x] T007: Add regression tests for timeout cancellation and slot recovery (owner: mayor)
  - Scope: `tests/tool_calls.rs`, `tests/support/mod.rs`, `tests/result_payload.rs`
  - Validation: `cargo test --all-targets`

- [x] T008: Update human docs and skill guidance for worker timeout model (owner: mayor)
  - Scope: `README.md`, `skills/capturing-ui-with-zeuxis/SKILL.md`, `specs/mcp-subprocess-capture-hard-timeout/**`
  - Validation: manual review + `cargo test --all-targets`
