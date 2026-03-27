# Benchmarks

This directory is the dedicated benchmark and evaluation area for PRISM.

It exists separately from the product crates because benchmark work is a mix of:

- methodology
- track-specific configs
- agent prompts
- telemetry contracts
- published results

The first benchmark split is:

- `SWE-bench Pro` for the primary Python-facing external result
- `SWE-bench Multilingual` Rust subset for the Rust companion result
- a PRISM-specific telemetry layer that is reported separately from benchmark outcome

The primary evaluated agent is `Codex`.

For the first phase, this directory provides:

- benchmark methodology and anti-bias rules
- shared schemas for run configs, results, and telemetry
- per-track pilot and full config stubs
- Codex prompt stubs for `control` and `prism` arms
- a lightweight config validation script
- a runner CLI that can plan, materialize, record, and finalize benchmark runs
- harness-oriented helpers for predictions templates, command rendering, report ingestion, and external command execution
- Codex CLI execution helpers that can run rich benchmark instances and write patches back into predictions files

## Layout

```text
benchmarks/
  methodology.md
  prompts/
    codex/
      control.md
      prism.md
  schemas/
    run-config.schema.json
    run-result.schema.json
    telemetry.schema.json
  tracks/
    swe-bench-pro/
      README.md
      configs/
      manifests/
    swe-bench-multilingual-rust/
      README.md
      configs/
      manifests/
  scripts/
    run_benchmark.py
    validate_configs.py
  results/
    local/
    published/
```

## Working Rule

Benchmark outcome and PRISM efficiency telemetry are not the same thing.

- Benchmark outcome answers whether the agent resolved the task under the benchmark harness.
- PRISM telemetry answers whether PRISM changed how the agent worked while doing it.

Those must be reported side by side, not blended into one score.

## Quick Start

Validate the scaffold:

```bash
python3 benchmarks/scripts/validate_configs.py
```

Create a run plan and initial result artifacts:

```bash
python3 benchmarks/scripts/run_benchmark.py materialize \
  --config benchmarks/tracks/swe-bench-pro/configs/pilot.json
```

Prepare empty predictions files for both arms:

```bash
python3 benchmarks/scripts/run_benchmark.py prepare-predictions \
  --config benchmarks/tracks/swe-bench-pro/configs/pilot.json
```

Render the external harness command for one arm:

```bash
python3 benchmarks/scripts/run_benchmark.py render-harness \
  --config benchmarks/tracks/swe-bench-pro/configs/pilot.json \
  --arm prism
```

Run Codex for one rich manifest instance:

```bash
python3 benchmarks/scripts/run_benchmark.py run-codex-instance \
  --config benchmarks/tracks/swe-bench-pro/configs/pilot.json \
  --arm control \
  --instance demo__instance
```

The configs in `tracks/*/configs/` are the source of truth for the first benchmark runs.
