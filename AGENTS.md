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

## PRISM MCP Workflow

When the PRISM MCP server is available for this repo, use it as the primary repo-awareness surface.

- Start with `prism://api-reference` and `prism://session` to confirm the available query surface, active task, and session limits.
- The default agent path is compact and staged:
  - `prism_locate`
  - `prism_gather`
  - `prism_open`
  - `prism_workset`
  - `prism_expand`
  - `prism_concept`
  - `prism_query` only when the compact surface cannot express the needed read
- Use `prism_gather` for bounded exact-text slices, especially config/schema/script work or when you know the literal text to inspect and a symbol handle is not the right first hop.
- Use `prism_concept` when the user asks about a broad repo-native term or subsystem concept such as `validation`, `runtime`, `session`, `memory`, `status`, `compact tools`, or `task continuity`.
- Prefer concept retrieval before symbol or text search when the likely unit is a multi-artifact repo concept rather than one file, symbol, or exact text match.
- Treat `prism_query` as the rich semantic escape hatch, not the default first hop.
- Prefer the compact top-level tools over ad hoc query snippets whenever they can express the task.
- Prefer PRISM-native file inspection and search when they can replace multiple shell reads with one bounded call, especially `prism_locate`, `prism_gather`, `prism_open`, `prism_workset`, `prism_expand`, `prism.file(path).read(...)`, `prism.file(path).around(...)`, and `prism.searchText(...)`.
- Prefer the compact PRISM tools and bounded PRISM-native reads over `sed`, `cat`, and `rg` when the work can be expressed in one staged PRISM flow.
- Keep shell reads as a fallback for raw bytes, command output, or cases where PRISM cannot yet express the needed inspection precisely.
- Keep `prism_query` read-only. Do not try to encode writes or side effects inside query snippets.
- After meaningful changes to PRISM MCP behavior or query/runtime behavior, rebuild the release binaries and restart the MCP daemon so the live PRISM server reflects the current code during the same Codex session.
- From the repo root, use these exact commands:
  - `cargo build --release -p prism-cli -p prism-mcp`
  - `./target/release/prism-cli mcp restart --internal-developer`
  - `./target/release/prism-cli mcp status`
  - `./target/release/prism-cli mcp health`
- Prefer the release binaries for restart and verification instead of `cargo run`, so the daemon and CLI are both using the freshly rebuilt release executables.

Compression-layer guidance:

- Return the minimum sufficient answer for the next likely agent action.
- Prefer carrying forward compact server-side state such as handles; avoid rediscovering the same target by text once a handle exists.
- Treat first-hop ranking quality as core product behavior, not as a secondary polish pass.

When mutations make sense, use the explicit PRISM mutation tools instead of leaving the state implicit.

- Use `prism_session` with action `start_task` when beginning a meaningful unit of work and no suitable active task already exists.
- Use `prism_mutate` with actions `outcome`, `test_ran`, `failure_observed`, and `fix_validated` to record task outcomes that matter for future reasoning.
- Use `prism_mutate` with action `memory` to store anchored memory when you learn something worth preserving, especially repo-specific constraints, invariants, migration rules, repeated failure patterns, or other durable lessons.
- Use the explicit persistence ladder for both memories and concepts:
  - `local`: runtime-only working state for tentative observations, scratch understanding, or hypotheses that should not survive reload.
  - `session`: persisted workspace state that should survive reload and help later work in the same clone, but is not yet strong enough to publish into repo knowledge.
  - `repo`: published repo knowledge that exports to committed JSONL, hydrates on clone/reload, and should be safe, reusable, and durable enough for future agents.
- Prefer storing new durable lessons as episodic memory first when they come from live repo work, concrete debugging, or dogfooding. Promote to structural memory later only after the lesson looks stable, repeated, or broadly invariant.
- During meaningful PRISM work, look for chances to capture 1 to 3 high-signal episodic memories instead of ending the task with no reusable memory at all.
- Use `prism_mutate` with action `infer_edge` when a new inferred relationship should be captured explicitly rather than only described in prose.
- Use `prism_mutate` with actions `coordination`, `claim`, and `artifact` when the work involves shared planning, task state, claims, handoffs, or reviewable artifacts.

Mutation guidance:

- Record memory only when the information is likely to help later tasks and is specific enough to anchor to code, lineage, files, or kinds.
- Prefer episodic memory for concrete findings such as "this query path misranked this target", "this workflow required this workaround", "this spec heading mapped to these implementation owners", or "this failure pattern needed this validation recipe".
- Do not wait for a perfect generalized rule before recording memory; capture the useful episodic fact while it is fresh, then promote or consolidate later if the pattern repeats.
- Record outcomes for meaningful tests, failures, validations, and task milestones, not for trivial intermediate noise.
- Prefer explicit anchored PRISM state over ad hoc scratch notes when the information should survive the current session.
- Prefer `local` memory for tentative or per-attempt findings, `session` memory for lessons likely to matter again in the current clone, and `repo` memory only for durable learned knowledge that a fresh clone should inherit.
- Treat repo-scoped memory as published repo knowledge. It should be evidence-backed, anchored, reusable, non-trivial to re-derive from raw code/history, and safe to commit.
- Do not publish repo memory for ephemeral task chatter, one-off debugging notes, rebuildable projections, or anything sensitive that should not live in the repo.

## Concept Pack Guidance

Concept packs are repo-native concept objects. They capture what belongs together across files, symbols, tests, config, docs, outcomes, and history so future agents do not have to rebuild that meaning from scratch.

- Treat concept packs as a reusable repo vocabulary layer, not as a taxonomy exercise.
- Prefer carrying forward an existing concept handle when it matches the task instead of rediscovering the same cluster through repeated search, locate, or open calls.
- When a concept packet needs inspection or curation help, request binding detail explicitly rather than assuming from prose alone. Use `includeBindingMetadata` on concept reads when you need to inspect lineage-backed member bindings, drift, or rebinding behavior.
- Promote a concept candidate when you resolved a broad or fuzzy repo term into a stable multi-artifact cluster that a future agent would likely want to reuse.
- Favor concept candidates that emerged from real task work: successful worksets, repeated broad-query resolution, repeated reuse of the same handles, meaningful outcome clusters, or handoffs that need compact repo-native shorthand.
- Record the concept in a compact shape: canonical name, common aliases, a short summary, 2 to 5 core handles, optional supporting handles, and optional likely tests or risks when they are already clear from the task.
- Prefer concepts that reflect how a future agent would naturally think or speak about the repo, such as `validation pipeline`, `runtime surface`, `session lifecycle`, `memory system`, `compact tools`, or `task continuity`.
- Do not promote one-off local details, temporary debugging clusters, arbitrary file groups, unstable intermediate hypotheses, or vague labels with no clear repo-native meaning.
- If a concept drifts, splits, or stops matching the real center of gravity, refresh or narrow it instead of continuing to accrete unrelated members.
- Use the same `local -> session -> repo` promotion ladder for concepts:
  - `local` concept: tentative working cluster for the current runtime only.
  - `session` concept: reusable within the current clone and persisted in workspace state, but not yet published to the repo.
  - `repo` concept: published repo knowledge that exports to `.prism/concepts/events.jsonl` and should be useful to future clones and sessions.
- Use `prism_mutate` with action `concept` and operation `promote` when you are creating a new concept packet at the right scope.
- Use `prism_mutate` with action `concept` and operation `update` when the concept’s summary, aliases, members, tests, evidence, risk hint, or scope have materially changed.
- Use `prism_mutate` with action `concept` and operation `retire` when a concept is misleading, obsolete, superseded, split into multiple concepts, or no longer matches the repo’s actual structure.
- Prefer `session` scope for concepts that are useful but still maturing. Promote to `repo` only after the concept has proven durable through repeated use, successful task flow, stable handoffs, or repeated broad-query convergence.
- Record concept candidates as episodic memory first when the cluster is still tentative, incomplete, or not yet worth even a session-scoped concept packet.
- Treat repo-scoped concepts as published repo knowledge, not convenient scratch bundles. They should be compact, evidence-backed, inspectable, and stable enough that a future agent would actually want to think and speak in that unit.
- Maintain concepts lazily but explicitly:
  - create when a real task has already discovered a reusable cluster
  - update after refactors, member drift, changed validation surfaces, repeated new risks, or better summaries/aliases
  - retire when the concept became too broad, stale, misleading, or superseded
- Do not let concepts silently rot. If you notice that a concept’s members no longer represent the live subsystem, update or retire it during the task instead of leaving stale published knowledge behind.

## Dogfooding Feedback Loop

When you use PRISM while working on PRISM, record notable validation cases immediately instead of waiting for a later replay-harness pass.

- Record a feedback entry whenever PRISM is materially wrong, stale, noisy, or unusually helpful during real repo work.
- Include the task or query context, the involved anchors, what PRISM said, what was actually true, the subsystem category (`structural`, `lineage`, `memory`, `projection`, `coordination`, `freshness`, or `other`), and whether you corrected it manually.
- When a dogfooding session also produces a reusable repo lesson, record both artifacts: validation feedback for PRISM quality and episodic memory for the lesson itself.
- Favor episodic memories that name the target, the observed behavior, and the practical implication for the next agent, so later promotion to structural memory has concrete source material.
- Prefer `prism_mutate` with action `validation_feedback` when the PRISM MCP server is available.
- Otherwise use `prism feedback record ...` from the CLI.
- The log is append-only and lives at `.prism/validation_feedback.jsonl`; treat it as seed material for the future replay validation harness, not as scratch prose.
