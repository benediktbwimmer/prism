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

## Path Policy

- The entire codebase, including documentation, must stay free of absolute paths.
- Use relative paths exclusively in source code, configuration, and docs.
- Do not introduce filesystem-specific paths such as `/Users/...` into committed files.
- When referencing repo files in docs, use repo-relative paths.

## Refactoring Guidance

When touching code that violates this policy, move it toward the target architecture instead of extending the violation.

## Validation Expectations

Use tiered validation to balance velocity and correctness.

### Tier 1: Targeted tests (always required)

- After making edits, run targeted tests for the crate(s) you changed.
- This is the minimum validation bar for every change.

### Tier 2: Downstream dependents (when public API changes)

- If you changed a public type, trait, function signature, or shared data structure in a crate that other crates depend on, also run tests for direct downstream dependents.
- Example: changes to `prism-core` should also test `prism-mcp` and `prism-cli`.

### Tier 3: Full workspace test suite (selective)

Run the full `cargo test` workspace suite only when:

- You changed shared coordination ref formats or SQLite schema.
- You changed the daemon startup, shutdown, or bridge transport path.
- You are about to merge and push to `main` (pre-merge validation).
- The current PRISM plan or task explicitly requires it in its validation requirements.

Do not run the full suite after every small edit. Tier 1 and Tier 2 are sufficient for most work.

### Flake policy

- If a full suite run flakes on individual tests, rerun the failing tests in isolation.
- When those isolated reruns pass, treat validation as successful and do not keep rerunning the full workspace suite only to chase the same non-deterministic flakes.

## Multi-Worktree Workflow

This repository uses multiple git worktrees for parallel agent development. Each worktree is a persistent slot, not a permanent branch. Follow trunk-based development strictly.

Before starting any new task:

```sh
git fetch origin
git checkout main
git reset --hard origin/main
```

During a task:

- Create a short-lived branch: `git checkout -b task/short-description`
- Do the work and commit as needed.

When the task is complete:

```sh
git checkout main
git merge --squash task/short-description
git commit -m "description of what was done"
git push origin main
git branch -d task/short-description
```

Rules:

- Never maintain a long-lived divergent branch in a worktree.
- Never leave uncommitted or unmerged work between tasks.
- Sync with `main`, not with other worktrees. There is no cross-worktree merging.
- If two agents finish simultaneously and the second push is rejected, that agent should `git pull --rebase` and resolve conflicts before pushing.

## PRISM MCP Workflow

- When the PRISM MCP server is available for this repo, use it as the primary repo-awareness surface.
- In a fresh worktree or any session where the bridge may still be warming up, read `prism://startup` first.
- If `prism://startup` reports that PRISM is not ready yet, wait for the suggested interval, then read `prism://startup` again until it reports `phase: ready`.
- Once `prism://startup` reports ready, read `prism://instructions`, then follow those instructions closely.
- If the server is unavailable, fall back to targeted local inspection until it is available again.
- After meaningful changes to PRISM MCP behavior or query/runtime behavior, rebuild the release binaries and restart the MCP daemon so the live PRISM server reflects the current code during the same Codex session.
- From the repo root, use these exact commands:
  - `cargo build --release -p prism-cli -p prism-mcp`
  - `./target/release/prism-cli mcp restart --internal-developer`
  - `./target/release/prism-cli mcp status`
  - `./target/release/prism-cli mcp health`
- Prefer the release binaries for restart and verification instead of `cargo run`, so the daemon and CLI are both using the freshly rebuilt release executables.
- For PRISM-on-PRISM Codex work across multiple worktrees, prefer `scripts/prism-mcp-codex-launcher.sh` as the MCP command. It resolves the current worktree from the launch directory, prefers that worktree's own release binaries when they exist, and otherwise starts a bootstrap bridge that exposes `prism://startup` while the worktree-local release build and daemon startup finish in the background.

## Dogfooding Feedback Loop

When you use PRISM while working on PRISM, record notable validation cases immediately instead of waiting for a later replay-harness pass.

- Record a feedback entry whenever PRISM is materially wrong, stale, noisy, or unusually helpful during real repo work.
- Include the task or query context, the involved anchors, what PRISM said, what was actually true, the subsystem category (`structural`, `lineage`, `memory`, `projection`, `coordination`, `freshness`, or `other`), and whether you corrected it manually.
- When a dogfooding session also produces a reusable repo lesson, record both artifacts: validation feedback for PRISM quality and episodic memory for the lesson itself.
- Favor episodic memories that name the target, the observed behavior, and the practical implication for the next agent, so later promotion to structural memory has concrete source material.
- Prefer `prism_mutate` with action `validation_feedback` when the PRISM MCP server is available.
- Otherwise use `prism feedback record ...` from the CLI.
- The log is append-only and lives at `.prism/validation_feedback.jsonl`; treat it as seed material for the future replay validation harness, not as scratch prose.
