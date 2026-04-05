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

The evaluator is track-specific:

- `swe-bench-multilingual-rust` runs through the local `swebench` harness.
- `swe-bench-pro` runs through the separate `scaleapi/SWE-bench_Pro-os` evaluator.
- the currently published `sb-cli` is not the right harness for either of those tracks.

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
- isolated per-arm workspaces so `control` and `prism` runs do not share a mutable repo
- a preparation step that turns benchmark metadata plus local repo mirrors into rich executable manifests

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

Install the local multilingual evaluator used by the Rust track:

```bash
python3 -m venv .venv-swebench
.venv-swebench/bin/python -m pip install swebench
```

Export the official dataset split and explicit instance manifests:

```bash
.venv-swebench/bin/python benchmarks/scripts/export_hf_dataset.py \
  --dataset-name ScaleAI/SWE-bench_Pro \
  --split test \
  --output-jsonl benchmarks/tracks/swe-bench-pro/data/swe_bench_pro_test.jsonl \
  --manifest-full benchmarks/tracks/swe-bench-pro/manifests/full.txt \
  --manifest-pilot benchmarks/tracks/swe-bench-pro/manifests/pilot.txt \
  --pilot-size 25 \
  --overwrite
```

Sync the local repo mirrors needed by one dataset slice:

```bash
python3 benchmarks/scripts/run_benchmark.py sync-repos \
  --dataset benchmarks/tracks/swe-bench-pro/data/swe_bench_pro_test.jsonl \
  --manifest benchmarks/tracks/swe-bench-pro/manifests/pilot.txt \
  --output-dir benchmarks/external/repo-mirrors
```

Prepare a rich manifest and derived config from the dataset export plus local mirrors:

```bash
python3 benchmarks/scripts/run_benchmark.py prepare-track \
  --config benchmarks/tracks/swe-bench-pro/configs/pilot.json \
  --dataset benchmarks/tracks/swe-bench-pro/data/swe_bench_pro_test.jsonl \
  --repo-template benchmarks/external/repo-mirrors/{repo_slug} \
  --output-dir benchmarks/results/local/prepared/swe-bench-pro-pilot \
  --force
```

Create a run plan and initial result artifacts from the prepared config:

```bash
python3 benchmarks/scripts/run_benchmark.py materialize \
  --config benchmarks/results/local/prepared/swe-bench-pro-pilot/config.json
```

Prepare empty predictions files for both arms:

```bash
python3 benchmarks/scripts/run_benchmark.py prepare-predictions \
  --config benchmarks/results/local/prepared/swe-bench-pro-pilot/config.json
```

Render the external harness command for one arm:

```bash
python3 benchmarks/scripts/run_benchmark.py render-harness \
  --config benchmarks/results/local/prepared/swe-bench-pro-pilot/config.json \
  --arm prism
```

Run Codex for one rich manifest instance:

```bash
python3 benchmarks/scripts/run_benchmark.py run-codex-instance \
  --config benchmarks/results/local/prepared/swe-bench-pro-pilot/config.json \
  --arm control \
  --instance demo__instance
```

Run Codex for all instances in one arm:

```bash
python3 benchmarks/scripts/run_benchmark.py run-codex-batch \
  --config benchmarks/results/local/prepared/swe-bench-pro-pilot/config.json \
  --arm prism \
  --continue-on-error
```

Run both `control` and `prism` arms for the same manifest:

```bash
python3 benchmarks/scripts/run_benchmark.py run-comparison \
  --config benchmarks/results/local/prepared/swe-bench-pro-pilot/config.json \
  --continue-on-error
```

`run-codex-instance`, `run-codex-batch`, and `run-comparison` execute against generated isolated git workspaces for each arm and instance. The manifest `workspace_dir` is treated as the pristine source checkout, and the benchmark runner leaves it untouched while capturing each arm's patch from its own isolated worktree.

`prepare-track` creates those pristine source checkouts first. It expects a dataset export in `.json` or `.jsonl` form containing at least:

- `instance_id`
- `base_commit` or `base_sha` or `base_ref`
- `problem_statement` or `prompt` or `task`
- either `repo_path` or `repo` plus `--repo-template`

The generated output includes:

- `sources/<instance_id>/repo`: detached source checkout at the benchmark base commit
- `prompts/<instance_id>.md`: rendered per-instance task prompt
- `manifest.jsonl`: rich manifest consumed by the runner
- `config.json`: derived config pointing at the generated rich manifest

Track notes:

- `swe-bench-multilingual-rust` expects `.venv-swebench/bin/python` to exist, emits list-style predictions with explicit `instance_id`, and uses `python -m swebench.harness.run_evaluation` with `dataset_name=SWE-bench/SWE-bench_Multilingual` and `split=test`.
- `swe-bench-pro` expects a local checkout at `benchmarks/external/SWE-bench_Pro-os` and a raw sample JSONL export referenced by the config’s `benchmark.raw_sample_path`.
- The harness now derives `docker_host` from the active Docker context when `DOCKER_HOST` is not already set and passes it to evaluator commands through harness environment templates.

The configs in `tracks/*/configs/` are the source of truth for the first benchmark runs.
