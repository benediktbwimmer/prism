# SWE-bench Multilingual Rust Track

This track is the Rust companion benchmark result for Codex with and without PRISM.

Use this track for the Rust-specific benchmark claim.

Files:

- `configs/pilot.json`: small explicit pilot cohort from the Rust subset
- `configs/full.json`: full approved Rust-subset cohort
- `manifests/`: explicit instance lists used by the configs

The harness for this track is the local `swebench` evaluator, invoked through `.venv-swebench/bin/python -m swebench.harness.run_evaluation` against `SWE-bench/SWE-bench_Multilingual` with `split=test`.

The multilingual evaluator expects predictions in list form with explicit `instance_id` fields. The tracked configs now emit that shape directly and also pass `DOCKER_HOST` from the active Docker context into the evaluator process so the Python Docker SDK can reach Colima correctly.

The operational flow for this track is:

- export the official `SWE-bench/SWE-bench_Multilingual` split and filter it to the Rust repo allowlist
- sync the required GitHub repos into `benchmarks/external/repo-mirrors`
- run `prepare-track` to create pristine source workspaces and a derived prepared config
- run `materialize`, `prepare-predictions`, Codex execution, and the multilingual evaluator against that prepared config

This track must be reported separately from `swe-bench-pro`.
