# PRISM Next Improvements

## Goal

The next round of PRISM improvements should optimize for agent effectiveness, not for replacing
every existing shell tool.

The target is:

* PRISM handles semantic narrowing, ownership, blast radius, and exact edit targeting well enough
  that shell reads become a thin raw-text layer.
* PRISM should also grow native file reading and strong text search where doing so materially
  improves single-call composability for agent workflows.
* `rg`, `sed`, and `cat` remain available as fallback raw tools even if PRISM covers more of these
  jobs directly.
* In the common case, the agent should only need one precise shell read after PRISM identifies the
  right symbol, block, or hunk.

This is a better target than re-implementing shell tools for their own sake. If native file reads
and text search allow much richer single-call PRISM inspection, they are worth adding.

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

PRISM should increasingly be able to answer those same questions in a bounded, agent-friendly way
when doing so reduces round-trips and lets one query combine semantic and exact inspection.

## Highest-Value Improvements

## 0. Native File Reader And Strong Text Search

PRISM should add a native file reader and a strong repo text-search surface.

The reason is not that shell tools are unavailable. The reason is that these capabilities become
much more valuable when they compose directly with PRISM's semantic graph, change view, and anchor
system in one query round-trip.

Needed capabilities:

* exact file reads by path and line range
* focused reads around a line or anchor
* exact text search with line-numbered results
* regex search and path filtering
* bounded snippets with exact spans rather than full-file dumps
* composition with semantic narrowing so one PRISM query can find a symbol, locate nearby text, and
  return the exact slice to inspect

Suggested shape:

* `prism.file(path).read({ startLine, endLine })`
* `prism.file(path).around({ line, before, after })`
* `prism.searchText(query, { regex, caseSensitive, path, glob, limit, contextLines })`

Success condition:

* many repo-inspection workflows that currently require `prism_query` plus `rg` plus `sed` can be
  done in one bounded PRISM call

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

## 4. First-Class Query Log

PRISM should expose a first-class query log so slow, noisy, or surprising queries can be debugged
without reconstructing behavior indirectly from daemon logs.

This matters for both system performance and trust. When a query is slow or returns something
unexpected, the agent should be able to inspect what happened directly.

Needed capabilities:

* query text or query identifier
* timestamp and total duration
* helper or phase breakdown for expensive queries
* diagnostics emitted during execution
* result-size and truncation metadata
* output-cap or node-cap indicators
* task or session correlation where available
* filters for recent queries, slow queries, or queries touching a given target

Useful query shapes:

* `prism.queryLog(...)`
* `prism.slowQueries(...)`
* `prism.queryTrace(id)`

Success condition:

* slow-query analysis and query-behavior debugging happen through PRISM itself instead of requiring
  manual correlation from daemon logs

## 5. Stronger Non-Symbol Coverage

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

## 6. Better Exact Context Retrieval Around Anchors

Even when PRISM finds the right symbol, the returned excerpt should be more useful for editing.

Needed improvements:

* ask for a focused surrounding block rather than a generic excerpt
* include exact neighboring control flow or write-path context
* make caller, callee, and validation references directly expandable with exact line locations
* reduce ambiguity between "best symbol match" and "best edit slice"
* give non-symbol nodes like config keys and document anchors exact local spans instead of
  falling back to coarse whole-file excerpts when more precise context is available

Success condition:

* PRISM can usually tell the agent not just which symbol matters, but which exact code block inside
  it matters

## 7. Better Ambiguity Handling And Narrowing

PRISM should require less manual refinement when symbol names or candidate owners are ambiguous.

Needed improvements:

* clearer ranking and explanation for ambiguous matches
* more direct narrowing by path, module, owner kind, or task context
* stronger default disambiguation for common symbol collisions
* better integration between search results and exact source locations
* stronger exact narrowing for structured config/document nodes so queries like top-level TOML keys
  do not overmatch nested paths or similarly named descendants
* make tool payload expectations easier to discover from the primary surface so agents do not need
  a separate schema lookup just to send common session or mutation actions correctly

Success condition:

* ambiguous lookups usually resolve with one PRISM follow-up rather than falling back to manual
  text search

## Explicit Non-Goals

The following are not the priority for this round:

* building a generic full-file diff viewer inside PRISM that always dumps complete patches
* adding unbounded file or diff dumping that is worse than existing shell tools
* treating native file read and search support as a goal by itself rather than as part of better
  PRISM composability

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
