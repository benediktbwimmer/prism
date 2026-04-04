## Mutations

- Use explicit PRISM mutation tools when durable state should be recorded instead of leaving it implicit.
- Do not rely on a separate `prism_session` mutation tool; use `prism://session` to inspect current context.
- Before any authoritative mutation, call `prism_mutate` with `action: "declare_work"` unless you are intentionally supplying an explicit mutation `taskId` or `claimId`.
- PRISM rejects authenticated mutations that do not have declared work context. Reads remain allowed without active work.
- Treat detached `currentTask` session leftovers as transient compatibility state, not durable intent. Bare session-task context is not guaranteed to survive restart unless it is anchored by declared work.
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
