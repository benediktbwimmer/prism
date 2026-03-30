# PRISM Planned Improvements

This note captures the highest-signal improvement areas surfaced while using PRISM MCP to implement the first Prism UI control-plane slice.

## 1. Freshness And New-File Anchoring

Problem:
Newly created files can exist on disk and be part of the active worktree, but still fail PRISM mutation anchoring immediately after edits or restart flows. During the UI work, `prism_mutate memory` rejected anchors for brand-new frontend files as "not indexed workspace file".

Why it matters:
This breaks the repo-native workflow right at the moment when fresh implementation context should be easiest to record.

Suggested work:
- Make file-anchor acceptance more tolerant for newly materialized workspace files.
- Clarify whether anchor validation should consult the live filesystem, the indexed graph, or both.
- Add a fast refresh/materialization path after new-file edits so mutations can anchor to them without a full rebuild cycle.

Validation:
- Create a new file, edit it, then immediately store memory and coordination mutations anchored to that file.
- Verify the same flow works both before and after daemon restart.

## 2. Ownership-Aware First-Hop Ranking

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

## 3. Better Semantic-To-Exact Edit Handoff

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

## 4. Large-File And Monolith Pressure Signals

Problem:
When a target file is too large, the compact surface degrades quietly instead of clearly signaling that the file itself is the real issue.

Why it matters:
This makes the tool feel weaker than it is, when part of the problem is actually code shape.

Suggested work:
- Emit explicit diagnostics when a target file is too large or too mixed-purpose for high-quality compact follow-through.
- Suggest decomposition-aware next steps, such as opening likely subregions or owner blocks.

Validation:
- Confirm large-file reads return a useful warning and a better next action instead of only a shallow fragment.
