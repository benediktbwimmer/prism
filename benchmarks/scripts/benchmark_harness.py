#!/usr/bin/env python3

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from typing import Any

from benchmark_common import ROOT, dump_json, load_json


class HarnessError(RuntimeError):
    pass


def _result_artifact_paths(config: dict[str, Any]) -> tuple[Path, Path]:
    return (
        Path(config["output"]["result_abspath"]),
        Path(config["output"]["telemetry_abspath"]),
    )


def _load_run_id(config: dict[str, Any]) -> str:
    result_path, _ = _result_artifact_paths(config)
    if not result_path.exists():
        raise HarnessError(
            f"result artifact does not exist yet: {result_path}. Run `materialize` first."
        )
    result_payload = load_json(result_path)
    return result_payload["run_id"]


def _arm(config: dict[str, Any], arm_name: str) -> dict[str, Any]:
    for arm in config["arms"]:
        if arm["name"] == arm_name:
            return arm
    raise HarnessError(f"unknown arm: {arm_name}")


def _template_context(config: dict[str, Any], arm_name: str) -> dict[str, str]:
    run_id = _load_run_id(config)
    arm = _arm(config, arm_name)
    arm_output_dir = ROOT / "benchmarks" / "results" / "local" / run_id / arm_name
    predictions_path = Path(
        str(config["harness"]["predictions_path"]).format(run_id=run_id, arm=arm_name)
    )
    report_path = Path(
        str(config["harness"]["report_path"]).format(run_id=run_id, arm=arm_name)
    )
    if not predictions_path.is_absolute():
        predictions_path = ROOT / predictions_path
    if not report_path.is_absolute():
        report_path = ROOT / report_path

    submission_run_id = f"{run_id}-{arm_name}"
    return {
        "run_id": run_id,
        "arm": arm_name,
        "track": config["track"],
        "cohort": config["cohort"],
        "suite": config["benchmark"]["suite"],
        "subset": str(config["benchmark"].get("subset", "")),
        "language": str(config["benchmark"].get("language", "")),
        "predictions_path": str(predictions_path),
        "report_path": str(report_path),
        "arm_output_dir": str(arm_output_dir),
        "submission_run_id": submission_run_id,
        "tool_profile": arm["tool_profile"],
    }


def _render_template_parts(parts: list[str], context: dict[str, str]) -> list[str]:
    return [part.format(**context) for part in parts]


def prepare_predictions(config: dict[str, Any], force: bool = False) -> list[dict[str, Any]]:
    created: list[dict[str, Any]] = []
    predictions_format = config["harness"]["predictions_format"]
    model_name = config["agent"]["model"]

    for arm in config["arms"]:
        context = _template_context(config, arm["name"])
        predictions_path = Path(context["predictions_path"])
        if predictions_path.exists() and not force:
            raise HarnessError(f"refusing to overwrite existing predictions file without --force: {predictions_path}")

        predictions_path.parent.mkdir(parents=True, exist_ok=True)
        if predictions_format == "dict":
            payload: dict[str, Any] = {
                instance_id: {"model_patch": "", "model_name_or_path": model_name}
                for instance_id in config["instance_ids"]
            }
        else:
            payload = [
                {
                    "instance_id": instance_id,
                    "model_patch": "",
                    "model_name_or_path": model_name,
                }
                for instance_id in config["instance_ids"]
            ]
        dump_json(predictions_path, payload)
        created.append({"arm": arm["name"], "predictions_path": str(predictions_path)})
    return created


def render_harness_commands(config: dict[str, Any], arm_name: str) -> dict[str, Any]:
    context = _template_context(config, arm_name)
    commands = {
        name: _render_template_parts(parts, context)
        for name, parts in config["harness"]["command_templates"].items()
        if parts is not None
    }
    return {
        "arm": arm_name,
        "context": context,
        "commands": commands,
    }


def run_harness_command(config: dict[str, Any], arm_name: str, step: str, dry_run: bool = False) -> dict[str, Any]:
    rendered = render_harness_commands(config, arm_name)
    commands = rendered["commands"]
    if step not in commands:
        raise HarnessError(f"unknown harness step `{step}`")

    context = rendered["context"]
    Path(context["arm_output_dir"]).mkdir(parents=True, exist_ok=True)
    command = commands[step]
    if dry_run:
        return {"arm": arm_name, "step": step, "command": command, "dry_run": True}

    proc = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        capture_output=True,
    )
    return {
        "arm": arm_name,
        "step": step,
        "command": command,
        "returncode": proc.returncode,
        "stdout": proc.stdout,
        "stderr": proc.stderr,
        "dry_run": False,
    }


def ingest_report(
    config: dict[str, Any],
    arm_name: str,
    report_path: Path | None = None,
) -> dict[str, Any]:
    result_path, telemetry_path = _result_artifact_paths(config)
    if not result_path.exists() or not telemetry_path.exists():
        raise HarnessError("run artifacts do not exist yet. Run `materialize` first.")

    context = _template_context(config, arm_name)
    report_file = report_path or Path(context["report_path"])
    if not report_file.is_absolute():
        report_file = ROOT / report_file
    if not report_file.exists():
        raise HarnessError(f"report file does not exist: {report_file}")

    report = load_json(report_file)
    result_payload = load_json(result_path)
    telemetry_payload = load_json(telemetry_path)

    result_arm = next(arm for arm in result_payload["arms"] if arm["name"] == arm_name)
    telemetry_arm = next(arm for arm in telemetry_payload["arms"] if arm["name"] == arm_name)
    instance_outcomes = _extract_instance_outcomes(report)
    if instance_outcomes is not None:
        result_arm["instance_outcomes"] = instance_outcomes
        for telemetry_item in telemetry_arm["instances"]:
            for outcome in instance_outcomes:
                if outcome["instance_id"] == telemetry_item["instance_id"]:
                    telemetry_item["resolved"] = outcome["status"] == "resolved"
                    break

    summary = _extract_summary(report, result_arm["instance_outcomes"])
    result_arm["benchmark_outcome"] = summary
    dump_json(result_path, result_payload)
    dump_json(telemetry_path, telemetry_payload)

    return {
        "arm": arm_name,
        "report_path": str(report_file),
        "benchmark_outcome": summary,
        "instance_outcome_count": len(result_arm["instance_outcomes"]),
    }


def _extract_int(report: dict[str, Any], key: str) -> int | None:
    value = report.get(key)
    if isinstance(value, int):
        return value
    if isinstance(value, list):
        return len(value)
    return None


def _extract_ids(report: dict[str, Any], *candidates: str) -> list[str] | None:
    for key in candidates:
        value = report.get(key)
        if isinstance(value, list) and all(isinstance(item, str) for item in value):
            return value
    return None


def _extract_instance_outcomes(report: dict[str, Any]) -> list[dict[str, str]] | None:
    resolved = _extract_ids(report, "resolved_ids", "resolved_instances_ids", "resolved_instances")
    unresolved = _extract_ids(report, "unresolved_ids", "unresolved_instances_ids", "unresolved_instances")
    errors = _extract_ids(report, "error_ids", "error_instances_ids", "error_instances")
    if resolved is None and unresolved is None and errors is None:
        return None

    outcomes: list[dict[str, str]] = []
    for instance_id in resolved or []:
        outcomes.append({"instance_id": instance_id, "status": "resolved"})
    for instance_id in unresolved or []:
        outcomes.append({"instance_id": instance_id, "status": "unresolved"})
    for instance_id in errors or []:
        outcomes.append({"instance_id": instance_id, "status": "error"})
    outcomes.sort(key=lambda item: item["instance_id"])
    return outcomes


def _extract_summary(report: dict[str, Any], instance_outcomes: list[dict[str, str]]) -> dict[str, Any]:
    attempted = _extract_int(report, "submitted_instances")
    resolved = _extract_int(report, "resolved_instances")
    unresolved = _extract_int(report, "unresolved_instances")
    errors = _extract_int(report, "error_instances")

    if attempted is None:
        attempted = len(instance_outcomes)
    if resolved is None:
        resolved = sum(1 for item in instance_outcomes if item["status"] == "resolved")
    if unresolved is None:
        unresolved = sum(1 for item in instance_outcomes if item["status"] == "unresolved")
    if errors is None:
        errors = sum(1 for item in instance_outcomes if item["status"] == "error")

    resolution_rate = float(resolved / attempted) if attempted else 0.0
    return {
        "instances_attempted": attempted,
        "instances_resolved": resolved,
        "instances_unresolved": unresolved,
        "harness_errors": errors,
        "resolution_rate": resolution_rate,
    }
