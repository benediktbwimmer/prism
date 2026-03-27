# PRISM Next Improvements

## Goal

The previous improvement round closed the biggest gaps in exact targeting, semantic change views,
runtime introspection, query logging, focused block retrieval, and basic ambiguity handling.

The next round should optimize for one thing above all else:

* `prism_query` should be reliable enough for normal multi-step TypeScript snippets that the agent
  rarely needs retry queries, fallback shell reads, or syntax-guessing.

The product target is now:

* one expressive `prism_query` should usually work on the first try
* one `prism_query` should often replace several narrower PRISM calls
* ambiguity should still be explained well, but the agent should need fewer refinement steps
* shell fallback should be for raw bytes, builds, and command output, not for working around query
  fragility

## What We Observed

The live system is in a much better place than before:

* exact edit targeting is strong
* semantic diff inspection is fast enough
* runtime and query introspection are available
* ambiguity handling is much clearer

The main remaining pain points are:

* richer `prism_query` snippets can still fail unexpectedly
* the query authoring contract is too brittle for normal TypeScript composition
* broad noun queries are better, but still not intent-aware enough in some repos
* the agent still sometimes needs several PRISM calls where one higher-level read should suffice

## Priority List

## 0. Make `prism_query` Multi-Statement Snippets Reliable

This is the top priority.

Normal query snippets should support the patterns an agent naturally writes:

* `const` and `let` bindings
* multiple statements
* `await`
* `return { ... }`
* intermediate arrays and maps
* conditional branches
* object literals with nested method calls

Current failure mode:

* snippets that look like ordinary TypeScript can still fail with parser errors such as
  `expecting ';'`, which forces trial and error

Needed improvements:

* make the parser and evaluator accept normal statement-oriented snippets consistently
* remove surprising differences between expression-only and block-style queries
* clearly define whether top-level `await`, implicit returns, and semicolon insertion are supported
* add golden tests for the real snippets that failed during live usage
* harden the wrapper/eval path so valid-looking snippets do not depend on incidental formatting

Success condition:

* the agent can write ordinary multi-step query snippets without having to simplify them into
  single-expression form

## 1. Improve Query Error Messages And Repair Hints

When `prism_query` fails, the current errors are too low-signal.

Needed improvements:

* exact line and column reporting that matches the submitted snippet
* clearer separation between parse failure, type/runtime failure, output serialization failure, and
  PRISM diagnostic output
* repair hints for common mistakes:
  * missing `return`
  * non-JSON-serializable output
  * unsupported syntax shape
  * query output too large
* include the rewritten or wrapped query shape when that helps explain the failure
* add examples of valid multi-statement snippets directly in the error help path

Success condition:

* when a query fails, the next attempt is obvious instead of guesswork

## 2. Add Higher-Level Single-Call Query Helpers

Even when `prism_query` works, the agent still sometimes needs several PRISM round-trips to gather
one practical working set.

PRISM should expose more composite helpers so the agent asks for intent, not orchestration.

High-value candidates:

* a search result bundle that includes ambiguity, focused block, and nearby validations in one call
* a target bundle that combines:
  * `focusedBlock`
  * `diffFor`
  * `editContext`
  * likely tests
* a file/search bundle that combines text matches with semantic neighbors
* a query helper that returns both primary result data and diagnostics in one stable envelope

Success condition:

* common agent workflows need fewer sequential `prism_query` calls

## 3. Make Broad Search More Intent-Aware

Broad noun queries are improved, but still lean too heavily on lexical matching.

Observed gap:

* queries like `helper` still return module-heavy results when the likely user intent is
  editable implementation code

Needed improvements:

* stronger preference for concrete implementation targets when the user likely wants code to inspect
  or edit
* better separation between container/module names and actionable symbols
* lower default weight for test-only results unless the query implies tests
* optional explicit modes such as:
  * prefer callable code
  * prefer editable targets
  * prefer behavioral owners over lexical collisions

Success condition:

* broad searches more often land on the thing an agent would actually inspect next

## 4. Build A Replay Corpus For Real Query Failures

The next hardening pass should be driven by real usage, not invented examples.

Needed improvements:

* capture failing `prism_query` snippets from live sessions
* store the expected result or expected error class
* replay them in tests across parser, wrapper, and runtime layers
* include ambiguity-ranking cases from real repo queries such as:
  * `search`
  * `helper`
  * `status`
  * `runtime`

Success condition:

* regressions in real agent workflows are caught before release

## 5. Tighten Query Result Ergonomics

The returned query shape should be easier to use without defensive probing.

Needed improvements:

* keep result and diagnostics behavior consistent across success, ambiguity, truncation, and empty
  matches
* make structured next actions easy to consume programmatically
* avoid surprising `null` result shapes when the real issue is ambiguity or truncation
* standardize how query summaries, touched targets, and truncation metadata are surfaced

Success condition:

* the agent can treat query output as a predictable interface instead of a special case per method

## 6. Continue Reducing Shell Fallbacks For Raw Inspection

PRISM should keep replacing multi-tool read workflows where the semantic context is already known.

High-value follow-ups:

* better composition between `prism.file(...)`, `prism.searchText(...)`, and semantic targets
* exact jump-to-file-slice follow-ups from ambiguity diagnostics
* easier “show me this target and the nearby raw file context” helpers

Success condition:

* once PRISM identifies the right target, the agent rarely needs more than one extra raw read

## Explicit Non-Goals

The next round is not about:

* rebuilding a general shell inside PRISM
* adding unbounded dump surfaces
* growing more features before the `prism_query` authoring experience is dependable

The reliability and composability of the existing surface matter more than adding another large
feature bucket right now.

## Practical Win Condition

This round is successful when the common workflow becomes:

1. write one ordinary multi-step `prism_query`
2. get the intended result on the first try
3. use one composite PRISM response instead of several narrower follow-ups
4. fall back to shell only for raw bytes, builds, or command output

Short version:

**The next PRISM milestone is not more surface area. It is making `prism_query` dependable enough
that agents can compose richer repo reads in one shot with much less trial and error.**
