# PRISM Next Improvements

## Goal

The previous improvement round closed major gaps in exact targeting, semantic change views,
runtime introspection, query logging, focused block retrieval, and basic ambiguity handling.

The next round should optimize for one thing above all else:

* PRISM should become a compact context compressor for agents, not a query-first interface that
  burns tokens through exploratory composition.

The product target is now:

* a compact staged default ABI for agent work
* a rich semantic core that still exists underneath it
* first-hop success and payload size as explicit product goals
* `prism_query` preserved as the semantic IR and escape hatch, not the default surface

The detailed target state lives in [AGENT_COMPRESSION_LAYER.md](/Users/bene/code/prism/docs/AGENT_COMPRESSION_LAYER.md).

## What We Observed

The live system is in a much better place than before:

* exact edit targeting is strong
* semantic diff inspection is fast enough
* runtime and query introspection are available
* ambiguity handling is much clearer

The main remaining pain points are:

* the default `prism_query`-first interaction model is still too expensive in agent loops
* repeated target rediscovery and repeated option blobs waste prompt tokens
* rich result payloads often return more than the next likely action needs
* broad noun queries are better, but first-hop ranking still decides whether the compact path will
  actually save tokens

## Priority List

## 0. Add Session-Local Handles And Compact Response Contracts

This is the top priority.

Needed improvements:

* store first-class session-local handles in the MCP layer
* keep handle resolution cheap and stable within a session
* return compact handles instead of large repeated symbol objects
* standardize compact status handling for empty, ambiguous, truncated, and stale-handle cases
* avoid repeated repo-root and target metadata across follow-up calls

Success condition:

* state moves from prompt tokens into server-side handles without losing determinism

## 1. Implement `prism_locate` And `prism_open` As The New Default Path

These should be thin compact wrappers over the existing search and file-read machinery.

Needed improvements:

* `prism_locate` returns the top 1 to 3 compact candidates only
* `prism_open` returns a bounded `focus`, `edit`, or `raw` slice only
* `edit` stays narrow and does not drift into a mini-bundle
* default first hop prefers editable implementation targets

Success condition:

* common `find -> open` flows no longer require `prism_query` as the first step

## 2. Add Payload And Tool-Count Telemetry Before Prompt Migration

Before changing benchmark prompts, PRISM should measure the compression layer directly.

Needed improvements:

* track dedicated compact-tool calls separately from `prism_query`
* track shell read calls and repeated shell reads
* record returned payload bytes by tool type
* make `prism_query` fallback usage visible in benchmark telemetry

Success condition:

* prompt and benchmark changes can be evaluated against real payload and usage data

## 3. Add `prism_workset` With A Hard Compactness Budget

`prism_workset` is high-value, but it is also the highest-risk compact tool.

Rules:

* keep it to:
  * `primary`
  * up to 3 `supporting_reads`
  * up to 2 `likely_tests`
  * one short `why`
* do not return nested diagnostics, rich discovery payloads, or helpful extras by default
* split the tool if pressure builds to add more

Success condition:

* one post-locate workset call replaces multiple exploratory follow-ups without becoming the new
  verbose bundle surface

## 4. Add `prism_expand` For Explicit Depth-On-Demand

The compact path needs an explicit place for optional depth so the default path stays small.

Needed improvements:

* add narrow expansions for:
  * diagnostics
  * lineage
  * neighbors
  * diff
  * validation
* ensure each expansion returns only its requested class of detail

Success condition:

* rich detail exists without inflating the default response contract

## 5. Keep Pushing First-Hop Ranking

Success condition:

* broad searches land on the thing an agent would actually inspect next often enough that repeated
  `locate` calls are rare

Needed improvements:

* broad nouns prefer implementation targets by default
* modules, containers, tests, examples, and docs are demoted unless requested
* behavioral-owner evidence is merged before ranking
* top-1 and top-3 quality become tracked quality targets

## 6. Keep `prism_query` Rich, But Make It Secondary

`prism_query` still matters, but its role changes.

Needed improvements:

* keep `prism_query` as the semantic IR and escape hatch
* stop teaching it as the default first hop in prompts, docs, and recipes
* keep bundle/query helpers available temporarily without presenting them as the preferred path

Success condition:

* PRISM preserves its expressive core without forcing the agent to pay for that expressiveness on
  most turns

## 7. Build Compression-Focused Replay And Benchmark Coverage

Needed improvements:

* add contract tests for compact tool outputs and handle stability
* add payload-size assertions for representative compact responses
* keep a fixed dogfood ranking set such as:
  * `session`
  * `helper`
  * `status`
  * `runtime`
  * `validation`
* measure whether the new path lowers prompt tokens, completion tokens, and payload size

Success condition:

* the compact path wins on both first-hop quality and token cost in representative agent tasks

## Explicit Non-Goals

The next round is not about:

* rebuilding a general shell inside PRISM
* adding unbounded dump surfaces
* deleting the semantic query model
* optimizing for conceptual elegance at the expense of agent token efficiency

The semantic core should remain rich. The agent-facing ABI should become smaller and more staged.

## Practical Win Condition

This round is successful when the common workflow becomes:

1. call `prism_locate`
2. call `prism_open`
3. optionally call `prism_workset`
4. call `prism_expand` only if deeper context is still needed
5. use `prism_query` only when the compact path cannot express the task

Short version:

**The next PRISM milestone is not a more expressive default query shell. It is a compact staged
agent ABI that preserves PRISM's rich mind while making its first words short.**
