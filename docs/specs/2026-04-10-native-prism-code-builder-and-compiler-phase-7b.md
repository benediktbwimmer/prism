# Native `prism_code` Builder And Compiler Phase 7b

Status: superseded  
Audience: prism-js, prism-mcp, prism-core, coordination, compiler, and runtime maintainers  
Scope: superseded by the stricter full-compiler cutover spec

---

Superseded by:

- [2026-04-10-full-prism-code-compiler-cutover-phase-7b.md](./2026-04-10-full-prism-code-compiler-cutover-phase-7b.md)

This earlier spec captured the first staged native-builder direction, but that target is no longer
strict enough.

The current implementation now needs a hard compiler cutover:

- full compiler/runtime coverage for all currently supported semantics
- no compatibility seams
- no residual old mutation API on the product path
- fixture-driven compiler proof
- one real SDK aligned with the same compiler-owned API surface
