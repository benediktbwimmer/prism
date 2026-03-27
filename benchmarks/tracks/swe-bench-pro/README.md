# SWE-bench Pro Track

This track is the primary external benchmark result for Codex with and without PRISM.

Use this track for the main Python-facing benchmark claim.

Files:

- `configs/pilot.json`: small explicit pilot cohort
- `configs/full.json`: full approved cohort
- `manifests/`: explicit instance lists used by the configs

This track uses the separate `scaleapi/SWE-bench_Pro-os` evaluator rather than `sb-cli`. The configs assume a local checkout under `benchmarks/external/SWE-bench_Pro-os` and a raw sample JSONL export path provided through `benchmark.raw_sample_path`.

The tracked configs also pass `DOCKER_HOST` from the active Docker context into the evaluator process so the underlying Python Docker client uses the same daemon as the host CLI.

The operational flow for this track is:

- export the official `ScaleAI/SWE-bench_Pro` split to `data/swe_bench_pro_test.jsonl`
- sync the required GitHub repos into `benchmarks/external/repo-mirrors`
- run `prepare-track` to create pristine source workspaces and a derived prepared config
- run `materialize`, `prepare-predictions`, Codex execution, and the Pro evaluator against that prepared config

The prompt pair for this track is shared with the other benchmark tracks so the A/B contract stays consistent.
