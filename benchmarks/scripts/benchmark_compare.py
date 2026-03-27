#!/usr/bin/env python3

from __future__ import annotations

from pathlib import Path
from typing import Any

from benchmark_common import load_json
from benchmark_codex import run_codex_batch
from benchmark_config import load_normalized_config
from benchmark_harness import prepare_predictions
from benchmark_runner import materialize_run


def _result_exists(config: dict[str, Any]) -> bool:
    return Path(config["output"]["result_abspath"]).exists()


def _predictions_path(config: dict[str, Any], run_id: str, arm_name: str) -> Path:
    path = Path(str(config["harness"]["predictions_path"]).format(run_id=run_id, arm=arm_name))
    if path.is_absolute():
        return path
    from benchmark_common import ROOT

    return ROOT / path


def ensure_comparison_artifacts(config_path: Path, *, force: bool = False) -> dict[str, Any]:
    config = load_normalized_config(config_path)
    if force or not _result_exists(config):
        materialize_run(config_path, force=force)
        config = load_normalized_config(config_path)

    result_payload = load_json(Path(config["output"]["result_abspath"]))
    run_id = result_payload["run_id"]
    predictions_missing = any(
        not _predictions_path(config, run_id, arm["name"]).exists() for arm in config["arms"]
    )
    if force or predictions_missing:
        prepare_predictions(config, force=force)

    return load_normalized_config(config_path)


def run_codex_comparison(
    config_path: Path,
    *,
    force: bool = False,
    continue_on_error: bool = False,
    instance_names: list[str] | None = None,
) -> dict[str, Any]:
    config = ensure_comparison_artifacts(config_path, force=force)
    result_payload = load_json(Path(config["output"]["result_abspath"]))
    run_id = result_payload["run_id"]

    arms: list[dict[str, Any]] = []
    for arm_name in ("control", "prism"):
        outcome = run_codex_batch(
            config,
            arm_name,
            force=force,
            continue_on_error=continue_on_error,
            instance_names=instance_names,
        )
        arms.append(outcome)
        if outcome["failures"] and not continue_on_error:
            break

    return {
        "run_id": run_id,
        "config_path": config["config_path"],
        "track": config["track"],
        "cohort": config["cohort"],
        "instance_ids": instance_names or config["instance_ids"],
        "arms": arms,
    }
