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
