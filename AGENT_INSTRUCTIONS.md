# PRISM MCP Agent Instructions

Use PRISM MCP as the primary repo-awareness surface when it is available.

## Familiarization

- Start with `prism://session` to confirm the active workspace root, task context, limits, and feature flags.
- Then inspect `prism://capabilities` to confirm the available query methods, resources, tools, and feature gates.
- Inspect `prism://vocab` before guessing enum spellings, action names, status values, edge kinds, or other closed vocabularies.
- Use `prism://tool-schemas` and `prism://schema/tool/{toolName}` when the task depends on exact MCP mutation or tool payload shapes.
- Use `prism://api-reference` after the basic server shape is clear and you need the typed query surface or usage recipes.

## Default Path

- Prefer the staged PRISM-first path for normal agent work:
  - orient with `prism://session`, `prism://capabilities`, and `prism://vocab`
  - use `prism_locate`, `prism_gather`, `prism_open`, `prism_workset`, and `prism_expand` for bounded context and edit targeting
  - use `prism_concept` when the unit of thought is a broad repo-native subsystem or multi-artifact cluster
  - use a `memory` lens on concept reads before substantial work in an unfamiliar subsystem
  - use typed query views when the task is semantic guidance rather than raw code lookup
  - use ad hoc read-only `prism_query` snippets only when the compact surface and typed views cannot express the needed read
- Use `prism_gather` for bounded exact-text slices when you know the text to inspect and a symbol handle is not the right first hop.
- Use `prism_concept` when the task is framed as a broad repo-native term such as `validation`, `runtime`, `session`, `memory`, `status`, `compact tools`, or `task continuity`.
- Prefer concept retrieval before symbol or text search when the likely unit is a multi-artifact repo concept rather than one file, symbol, or exact string match.

## Query Views

- Treat the typed query views as first-class workflow tools:
  - `repoPlaybook()` for repo workflow, build, test, lint, format, and gotcha guidance
  - `validationPlan(...)` for fast and broader validation recommendations after a change
  - `impact(...)` for downstream blast radius, affected surfaces, and recommended checks
  - `afterEdit(...)` for immediate next reads, tests, docs, and risk follow-through after an edit
  - `commandMemory(...)` for recalled command evidence merged with current repo playbook guidance
- Treat custom `prism_query` snippets as the semantic escape hatch, not the default first hop.
- Keep `prism_query` read-only. Do not encode writes or side effects inside typed query views or custom query snippets.

## Read Strategy

- Prefer checking `prism://vocab` before guessing enum spellings or mutation action names.
- Prefer checking `prism.tool("...")`, `prism://tool-schemas`, and `prism://schema/tool/{toolName}` before hand-writing non-trivial mutation payloads.
- Prefer compact top-level tools and typed query views over ad hoc query snippets whenever they can express the task.
- Prefer PRISM-native file inspection and bounded context retrieval when they can replace multiple shell reads with one staged call, especially `prism_locate`, `prism_gather`, `prism_open`, `prism_workset`, `prism_expand`, `prism.file(path).read(...)`, `prism.file(path).around(...)`, and `prism.searchText(...)`.
- Prefer compact PRISM tools and bounded PRISM-native reads over manual line-window shell reads such as `sed` and `cat` when the work can be expressed in one staged PRISM flow.
- Targeted `rg` is acceptable for exact-text narrowing, test-name lookup, or fast filename discrimination before returning to PRISM for the actual read or edit context.
- Keep shell reads as a fallback for raw bytes, command output, or cases where PRISM cannot yet express the needed inspection precisely.

## Compression

- Return the minimum sufficient answer for the next likely agent action.
- Prefer carrying forward compact server-side state such as handles instead of rediscovering the same target by text.
- Treat first-hop ranking quality as core product behavior, not as a secondary polish pass.

## Plans

- When working on a PRISM plan, always claim a task by marking it `in_progress` before you start any research on that task, not just before making edits. Otherwise your research effort might be wasted if another actor claims the task in the meantime.

## Mutations

- Use explicit PRISM mutation tools when durable state should be recorded instead of leaving it implicit.
- Do not rely on a separate `prism_session` mutation tool; use `prism://session` to inspect current context and let the first `prism_mutate` create a task implicitly when no active task exists.
- Use `prism_mutate` with actions `outcome`, `test_ran`, `failure_observed`, and `fix_validated` to record meaningful task results.
- Use `prism_mutate` with action `memory` to store anchored memory when you learn something worth preserving.
- Use the persistence ladder intentionally:
  - `local` for tentative runtime-only observations
  - `session` for lessons likely to matter again in the current clone
  - `repo` for durable published repo knowledge that a fresh clone should inherit
- Prefer storing new durable lessons as episodic memory first when they come from live repo work, concrete debugging, or dogfooding.
- During meaningful PRISM work, look for chances to capture 1 to 3 high-signal episodic memories instead of ending with no reusable memory.
- Use `prism_mutate` with action `infer_edge` when a new inferred relationship should be captured explicitly.
- Use `prism_mutate` with actions `coordination`, `claim`, and `artifact` when the work involves shared planning, task state, claims, handoffs, or reviewable artifacts.
- Task-scoped reads may occasionally return a server-authored instruction to call `prism_mutate` with action `heartbeat_lease`.
- When that heartbeat instruction appears, satisfy it before continuing other task work.

## Memory Guidance

- Record memory only when the information is likely to help later tasks and is specific enough to anchor to code, lineage, files, or kinds.
- Prefer episodic memory for concrete findings such as a misranked target, a required workflow workaround, a spec-to-owner mapping, or a repeated failure pattern with a useful validation recipe.
- Do not wait for a perfect generalized rule before recording memory; capture the useful episodic fact while it is fresh.
- Record outcomes for meaningful tests, failures, validations, and task milestones, not for trivial intermediate noise.
- Prefer explicit anchored PRISM state over ad hoc scratch notes when the information should survive the current session.
- Treat repo-scoped memory as published repo knowledge. It should be evidence-backed, anchored, reusable, and safe to commit.
- Do not publish repo memory for ephemeral chatter, one-off debugging notes, rebuildable projections, or sensitive material.

## Concept Packs

- Treat concept packs as a reusable repo vocabulary layer, not as a taxonomy exercise.
- Use semantic pathfinding to move from high-level abstractions down to concrete code handles instead of repeatedly searching or guessing file locations.
- Use `verbosity` intentionally:
  - `summary` for discovery
  - `standard` for balanced architectural context
  - `full` only when deep reasoning or historical detail is required
- When entering an unfamiliar subsystem, decode the governing concept with `lens: "memory"` before writing code.
- Prefer carrying forward an existing concept handle when it matches the task instead of rediscovering the same cluster.
- Request binding detail with `includeBindingMetadata` when you need to inspect lineage-backed member bindings, drift, or rebinding behavior.
- Promote a concept candidate when real task work has resolved a broad or fuzzy term into a stable multi-artifact cluster that future agents would likely reuse.
- Prefer concepts that match how future agents will naturally think and speak about the repo, such as `validation pipeline`, `runtime surface`, `session lifecycle`, `memory system`, `compact tools`, or `task continuity`.
- Use the same `local -> session -> repo` promotion ladder for concepts.
- Use `prism_mutate` with action `concept` and operation `promote`, `update`, or `retire` when the concept lifecycle matters.
- Do not let concepts silently rot. If a concept no longer matches the live subsystem, update or retire it.
