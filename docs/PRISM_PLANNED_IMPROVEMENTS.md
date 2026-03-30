# PRISM Planned Improvements

This note captures the highest-signal improvement areas surfaced while using PRISM MCP during real implementation, coordination, and daemon-health work.

## 1. Ownership-Aware First-Hop Ranking

Problem:
Compact discovery can still over-rank nearby text hits instead of the actual owner module for the task. During routing work, suggestions leaned toward operation-detail code instead of the shell/router boundary that actually owned the change.

Why it matters:
The agent still gets useful context, but the first hop is slower and less trustworthy than it should be for implementation work.

Suggested work:
- Increase ranking weight for route owners, boundary modules, and app-shell modules when the query implies routing, entrypoints, assets, or page structure.
- Bias `prism_workset` and related compact tools toward edit owners rather than incidental text matches.
- Make ownership signals explicit in ranking diagnostics so misfires are easier to debug.

Validation:
- Use the same query family on known routing or shell tasks and confirm the top result is the owning module, not a nearby consumer.

## 2. Better Semantic-To-Exact Edit Handoff

Problem:
PRISM is strong at semantic orientation, but once a file becomes large or monolithic, compact slices stop being enough and the workflow falls back to raw shell reads.

Why it matters:
The last-mile transition from "I found the right area" to "I can safely edit this" is where a lot of agent time is still lost.

Suggested work:
- Improve `prism_open` and related tools for large files with better edit-centered windows and stronger local structure summaries.
- Add a mode that can return a compact decomposition of a large file into logical regions before opening raw slices.
- Prefer ownership blocks and structural boundaries over literal line windows when selecting edit context.

Validation:
- Run the flow on known large files and confirm the agent can stay inside PRISM-native reads longer before needing shell fallback.

## 3. Large-File And Monolith Pressure Signals

Problem:
When a target file is too large, the compact surface degrades quietly instead of clearly signaling that the file itself is the real issue.

Why it matters:
This makes the tool feel weaker than it is, when part of the problem is actually code shape.

Suggested work:
- Emit explicit diagnostics when a target file is too large or too mixed-purpose for high-quality compact follow-through.
- Suggest decomposition-aware next steps, such as opening likely subregions or owner blocks.

Validation:
- Confirm large-file reads return a useful warning and a better next action instead of only a shallow fragment.

## 4. Simpler Native-Node Versus Coordination-Task Semantics

Problem:
PRISM currently exposes two closely related workflow units, native plan nodes and coordination tasks, but the behavioral boundary is still easy to trip over. During live plan work, validation evidence recorded against a native node id as the current task could fail completion gates because some logic paths still expected a real coordination task.

Why it matters:
This makes the workflow model feel more complicated than it should be and creates avoidable bookkeeping bugs in the most operationally sensitive path: completing validated work.

Suggested work:
- Make the relationship between native plan nodes, coordination tasks, and current-task session state more explicit in server responses and diagnostics.
- Normalize validation, blockers, and completion semantics so agents do not need to know which internal workflow shape they are dealing with.
- Add clearer diagnostics when a behavior depends on node-backed versus task-backed evidence resolution.

Validation:
- Confirm the same validation-recording flow succeeds whether the active work unit is a coordination task or a native plan node.

## 5. Easier Durable Memory And Concept Publication

Problem:
PRISM makes it easy to record session memory, but promoting the strongest findings into repo-scoped knowledge is still too manual and too conservative in practice.

Why it matters:
High-value lessons are often rediscovered across sessions because they never cross the last mile from useful episodic memory into durable published repo knowledge.

Suggested work:
- Add a lighter-weight promotion workflow from strong session memories into repo scope, with duplicate detection and stronger suggested defaults.
- Surface candidate repo-memory promotions after meaningful tasks, especially when memories are well anchored and evidence-backed.
- Make it easier to review, supersede, and retire repo memories so publishing durable knowledge feels safer.

Validation:
- Run several real tasks and confirm that at least the strongest architectural or workflow lessons are routinely promoted without flooding repo memory with one-off notes.

## 6. Tighter Schema And Live-Behavior Parity

Problem:
Tool schemas, examples, and capability metadata can still drift from the actual live server behavior, especially on richer mutation and workflow surfaces.

Why it matters:
Once the surface contract feels unreliable, agents stop trusting the MCP layer and fall back to shell or ad hoc reasoning more often than necessary.

Suggested work:
- Keep tool schemas, schema examples, capabilities resources, and live mutation behavior under the same validation loop.
- Add more parity tests for high-value tagged-union tools and workflow-specific mutation paths.
- Prefer failing loudly with precise diagnostics when the documented contract and live behavior diverge.

Validation:
- Exercise schema-driven clients and confirm that documented payloads, tool examples, and live server behavior all agree on the same call shapes.

## 7. More Predictable Operational Smoothness

Problem:
PRISM can answer rich semantic and coordination questions, but operational friction still shows through in refresh timing, workflow fussiness, and occasional abstraction leaks.

Why it matters:
A powerful semantic surface only helps if it also feels dependable under normal editing and validation loops. If the runtime feels fussy, agents use the product less even when the underlying semantics are good.

Suggested work:
- Keep improving refresh, freshness, and workflow diagnostics so bounded lag or abstraction mismatches are easier to interpret.
- Prefer product behavior that is mechanically predictable over behavior that is internally clever but surprising.
- Add more dogfooding-oriented validation around common edit, validate, and plan-advance loops.

Validation:
- Use PRISM continuously across multi-step implementation tasks and confirm that the agent can stay inside the surface without repeated fallback caused by operational ambiguity.
