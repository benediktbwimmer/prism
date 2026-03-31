# AGENTS.md

## Purpose

This repository must stay highly modularized. Prefer small, focused modules with clear responsibilities over large, mixed-purpose files.

## Architectural Rule

`main.rs` and `lib.rs` files are facades only.

- Do not place core business logic, parsing logic, coordination logic, storage logic, or domain rules directly in `main.rs` or `lib.rs`.
- Use `main.rs` only to wire together the executable entrypoint, CLI/bootstrap setup, and top-level module boundaries.
- Use `lib.rs` only to declare modules, define the crate's public surface, and re-export narrowly chosen APIs.
- Move substantive logic into dedicated submodules with descriptive names.

## Modularity Expectations

- Keep modules narrowly scoped and cohesive.
- Split large files before they accumulate unrelated responsibilities.
- Favor composition between modules instead of growing monolithic entrypoint files.
- Keep public APIs minimal and intentional.
- When adding a feature, first decide which module owns it; if no clear owner exists, create one.

## Refactoring Guidance

When touching code that violates this policy, move it toward the target architecture instead of extending the violation.

## Validation Expectations

- After making edits, run targeted tests for the area you changed.
- After targeted tests pass, always run the full workspace test suite to confirm the entire repo is green before finishing the task.

## PRISM MCP Workflow

- When the PRISM MCP server is available for this repo, use it as the primary repo-awareness surface.
- Start by reading `prism://instructions`, then follow those instructions closely.
- If the server is unavailable, fall back to targeted local inspection until it is available again.
- After meaningful changes to PRISM MCP behavior or query/runtime behavior, rebuild the release binaries and restart the MCP daemon so the live PRISM server reflects the current code during the same Codex session.
- From the repo root, use these exact commands:
  - `cargo build --release -p prism-cli -p prism-mcp`
  - `./target/release/prism-cli mcp restart --internal-developer`
  - `./target/release/prism-cli mcp status`
  - `./target/release/prism-cli mcp health`
- Prefer the release binaries for restart and verification instead of `cargo run`, so the daemon and CLI are both using the freshly rebuilt release executables.

## Dogfooding Feedback Loop

When you use PRISM while working on PRISM, record notable validation cases immediately instead of waiting for a later replay-harness pass.

- Record a feedback entry whenever PRISM is materially wrong, stale, noisy, or unusually helpful during real repo work.
- Include the task or query context, the involved anchors, what PRISM said, what was actually true, the subsystem category (`structural`, `lineage`, `memory`, `projection`, `coordination`, `freshness`, or `other`), and whether you corrected it manually.
- When a dogfooding session also produces a reusable repo lesson, record both artifacts: validation feedback for PRISM quality and episodic memory for the lesson itself.
- Favor episodic memories that name the target, the observed behavior, and the practical implication for the next agent, so later promotion to structural memory has concrete source material.
- Prefer `prism_mutate` with action `validation_feedback` when the PRISM MCP server is available.
- Otherwise use `prism feedback record ...` from the CLI.
- The log is append-only and lives at `.prism/validation_feedback.jsonl`; treat it as seed material for the future replay validation harness, not as scratch prose.
