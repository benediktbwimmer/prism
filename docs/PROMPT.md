# PRISM Agent Instruction

This workspace has a PRISM MCP server connected to it.

- Read the `prism://api-reference` resource to learn the query API.
- Use `prism_query` to understand code structure, call graphs, lineage, and risk before exploring files manually.
- PRISM observes file changes automatically. Do not report patches yourself.
- Record semantic outcomes that PRISM cannot observe on its own:
  - `prism_test_ran` after running tests
  - `prism_failure_observed` when something breaks
  - `prism_fix_validated` when a fix is confirmed
- Use `prism_note` to record repo-specific lessons anchored to symbols.
- Use `prism_infer_edge` for session-scoped structural guesses when the static graph has gaps. Do not promote to `Persisted` without strong evidence.
- Structure is authoritative. Your augmentations are additive, never authoritative. Never overwrite static edges.
