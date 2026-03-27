# Benchmark Methodology

## Goal

Measure whether PRISM improves Codex on real issue-resolution benchmarks without contaminating the comparison.

The benchmark program answers three separate questions:

1. Does Codex with PRISM resolve more benchmark instances?
2. Does Codex with PRISM behave better on Rust repositories?
3. Does PRISM reduce wasted work such as repeated reads, unnecessary tool calls, or excess wall time?

## Tracks

The first benchmark tracks are:

- `swe-bench-pro`
- `swe-bench-multilingual-rust`

These tracks should be reported separately.

They also require different evaluator backends:

- `swe-bench-multilingual-rust`: local `swebench` harness
- `swe-bench-pro`: `scaleapi/SWE-bench_Pro-os`

## Evaluated Agent

The evaluated agent family is `Codex`.

Every reported run must record:

- the exact model name
- the exact reasoning setting
- the exact prompt pair used for `control` and `prism`
- the exact benchmark config file
- the git commit of this repository

## A/B Contract

The comparison must hold the following constant between arms:

- agent family
- model
- reasoning effort
- per-instance timeout
- max turns
- retry budget
- instance list
- benchmark harness version
- output parsing and scoring rules

The only intended difference between the two arms is PRISM availability.

- `control`: Codex without PRISM MCP access
- `prism`: Codex with PRISM MCP access and explicit permission to prefer PRISM for repo awareness
- both arms must run against separate isolated workspaces derived from the same pristine source checkout

The prompt pair should also enforce the same execution discipline in both arms:

- prefer the smallest patch that addresses the named benchmark issue
- require at least one targeted local validation step when a relevant command is discoverable within budget
- prefer targeted validation over extra repo exploration once a plausible fix is identified
- avoid broad new fixtures, snapshots, or unrelated test expansion

## Telemetry Rule

Benchmark outcome and PRISM telemetry must be collected separately.

Benchmark outcome includes:

- instances attempted
- instances resolved
- instances unresolved
- harness errors
- resolution rate

The benchmark harness may be external to this repo.

The benchmark subsystem here is responsible for:

- preparing predictions artifacts
- running Codex on prepared benchmark workspaces
- rendering or executing the external harness commands
- ingesting harness reports back into the local result schema

PRISM telemetry includes:

- total tool calls
- PRISM query count
- shell command count
- shell read count
- repeated read count
- patch attempt count
- prompt and completion token counts
- wall time

## Anti-Bias Rules

- Do not give the PRISM arm extra retries.
- Do not give the control arm a weaker prompt.
- Do not let telemetry instrumentation change the action space of one arm but not the other.
- Do not mix Python and Rust conclusions into a single headline number.
- Do not treat lower tool count as success if benchmark outcome regresses.

## Execution Phases

### Phase 1: Pilot

Use a small, explicitly listed manifest for each track.

Purpose:

- validate the run contract
- validate output schemas
- validate telemetry capture
- surface harness bugs before large runs

### Phase 2: Full

Run the full approved manifest for each track only after pilot stability.

Purpose:

- produce the main external benchmark numbers
- produce the Rust companion numbers
- compare PRISM and control at meaningful scale

## Published Outputs

Each published comparison should include three artifacts:

1. benchmark outcome summary
2. telemetry summary
3. methodology snapshot referencing the exact config and prompt files used
