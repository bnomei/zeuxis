# Tasks — mcp-screenshot-server

Meta:
- Spec: `mcp-screenshot-server` — Local read-only MCP screenshot server
- Depends on: none
- Global scope:
  - `Cargo.toml`
  - `src/**`
  - `tests/**`
  - `docs/**`
  - `specs/mcp-screenshot-server/**`

## In Progress

- (none)

## Blocked

- (none)

## Todo

- (none)

## Done

- [x] T001: Initialize crate skeleton and dependencies (owner: mayor)
  - Validation: `cargo check`

- [x] T002: Implement capture backend abstraction and xcap backend (owner: mayor)
  - Validation: `cargo test capture -- --nocapture`

- [x] T003: Implement cursor provider and coordinate mapping utilities (owner: mayor)
  - Validation: `cargo test coordinate`

- [x] T004: Implement tool input validation and shared pre-capture flow (owner: mayor)
  - Validation: `cargo test mcp_tools_validation`

- [x] T005: Implement all five capture tools (owner: mayor)
  - Validation: `cargo test tool_calls`

- [x] T006: Implement storage writer and MCP result payload mapper (owner: mayor)
  - Validation: `cargo test result_payload`

- [x] T007: Add platform permission gate and capability errors (owner: mayor)
  - Validation: `cargo test platform_permissions`

- [x] T008: Add deterministic tests with mock backend (owner: mayor)
  - Validation: `cargo test`

- [x] T009: Add docs and operational notes (owner: mayor)
  - Validation: manual doc review against `requirements.md`

- [x] T010: Final quality gate (owner: mayor)
  - Validation: `cargo fmt --all --check`, `cargo clippy --all-targets --all-features`, `cargo test --all-targets`
