# PRISM Next Improvements

## Goal

The next round of PRISM improvements should optimize for agent effectiveness, not for replacing
every existing shell tool.

The target is:

* PRISM handles semantic narrowing, ownership, blast radius, and exact edit targeting well enough
  that shell reads become a thin raw-text layer.
* `rg`, `sed`, and `cat` remain available for exact text search and file reading.
* In the common case, the agent should only need one precise shell read after PRISM identifies the
  right symbol, block, or hunk.

This is a better target than trying to re-implement general-purpose text search and file reading
inside PRISM.

## Product Direction

PRISM should be the system that answers:

* what is the relevant symbol or subsystem?
* what code actually owns this behavior?
* which neighboring symbols and tests matter?
* what exact lines should be inspected or edited next?
* what changed recently that matters for this task?

Shell tools should remain the system that answers:

* show me the raw bytes for this exact file slice
* run a literal or regex text search
* display the final raw patch or command output

## Highest-Value Improvements

## 1. Exact Edit Spans And Stable Line Mapping

The most important improvement is better exact line targeting.

PRISM should make it easy to ask for the exact lines that matter for an edit instead of returning a
broad excerpt that still requires manual searching.

Needed capabilities:

* exact start and end lines for the intended symbol or code block
* tighter edit slices around the relevant implementation logic
* adjacent anchors with exact line references such as callers, write paths, validations, and tests
* stable line mapping across reindexing when symbols move or files shift
* symbol-to-hunk mapping for recently changed regions

Success condition:

* most shell file reads become a single precise call because PRISM already identified the right
  span

## 2. Native, Token-Efficient "What Changed" View

PRISM should add a first-class change view so the agent does not need to default to broad
`git diff` output.

The main reason is token efficiency and relevance. Most of the time the agent does not need the
full diff for every changed file. It needs the changed hunks that are relevant to the current
symbol, task, or blast radius.

Desired behavior:

* summarize changes semantically rather than dumping the full patch first
* return changed files, changed symbols, moved lineages, and likely affected tests
* allow narrowing to only the hunks related to a symbol, lineage, file, or task
* provide exact line spans for the changed hunks so one follow-up raw read is enough
* default to summaries and capped excerpts rather than full patches

Useful query shapes:

* `prism.changed(...)`
* `prism.diffFor(target, ...)`
* `prism.taskChanges(taskId)`
* `prism.recentPatches(target)`
* `prism.changedFiles()` and `prism.changedSymbols(file)`

Success condition:

* `git diff` becomes an occasional fallback rather than the default inspection surface for recent
  changes

## 3. Better Runtime And Log Introspection

PRISM should expose runtime and daemon observability directly enough that the agent does not need
to rely on shell log tailing and process inspection for normal diagnosis.

Needed capabilities:

* daemon status and health inspection
* recent startup and refresh timelines
* access to recent structured log events with filtering
* process-topology visibility for daemon and bridge processes
* direct visibility into recent performance hotspots and counters

Success condition:

* runtime diagnosis mostly happens through PRISM queries instead of `ps`, log tails, and ad hoc
  shell inspection

## 4. Stronger Non-Symbol Coverage

PRISM is already useful for symbol-oriented repo work. It should become better at repo-awareness
outside code symbols.

Needed coverage:

* Markdown docs and headings
* config files and structured settings
* fixture files and test data
* generated artifacts worth inspecting
* log-backed anchors where runtime evidence matters

Success condition:

* agents can navigate docs, config, tests, and implementation through one coherent repo-awareness
  surface instead of switching to shell discovery too early

## 5. Better Exact Context Retrieval Around Anchors

Even when PRISM finds the right symbol, the returned excerpt should be more useful for editing.

Needed improvements:

* ask for a focused surrounding block rather than a generic excerpt
* include exact neighboring control flow or write-path context
* make caller, callee, and validation references directly expandable with exact line locations
* reduce ambiguity between "best symbol match" and "best edit slice"

Success condition:

* PRISM can usually tell the agent not just which symbol matters, but which exact code block inside
  it matters

## 6. Better Ambiguity Handling And Narrowing

PRISM should require less manual refinement when symbol names or candidate owners are ambiguous.

Needed improvements:

* clearer ranking and explanation for ambiguous matches
* more direct narrowing by path, module, owner kind, or task context
* stronger default disambiguation for common symbol collisions
* better integration between search results and exact source locations

Success condition:

* ambiguous lookups usually resolve with one PRISM follow-up rather than falling back to manual
  text search

## Explicit Non-Goals

The following are not the priority for this round:

* replacing `rg` as a general-purpose text or regex search tool
* replacing `sed` or `cat` as raw file readers
* building a generic full-file diff viewer inside PRISM that always dumps complete patches

Those tools already solve their narrow jobs well. PRISM should instead make their use more precise
and less frequent.

## Practical Win Condition

This effort is successful when the common agent workflow becomes:

1. ask PRISM what symbol, block, or changed hunk matters
2. get exact lines, adjacent anchors, and likely validations from PRISM
3. make one precise shell read if raw text is still needed
4. edit with minimal extra searching

Short version:

**PRISM should be the semantic narrowing layer, the exact edit-targeting layer, and the
change-aware inspection layer. Shell tools can remain the raw-text layer.**
