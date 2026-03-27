#!/usr/bin/env python3

from __future__ import annotations

import json
import os
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


def _resolve_path_like(value: str) -> Path:
    path = Path(value)
    return path if path.is_absolute() else ROOT / path


def _maybe_resolve_repo_path(value: str) -> str:
    if not value:
        return value
    path = Path(value)
    if path.is_absolute():
        return str(path)
    return str((ROOT / path).resolve())


def _render_value(template: str, context: dict[str, str]) -> str:
    return template.format(**context)


def _detect_docker_host() -> str:
    explicit = os.environ.get("DOCKER_HOST", "").strip()
    if explicit:
        return explicit

    try:
        context_name = subprocess.run(
            ["docker", "context", "show"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=True,
        ).stdout.strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return ""

    if not context_name:
        return ""

    try:
        raw = subprocess.run(
            ["docker", "context", "inspect", context_name],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=True,
        ).stdout
        payload = json.loads(raw)
    except (subprocess.CalledProcessError, FileNotFoundError, json.JSONDecodeError):
        return ""

    if not isinstance(payload, list) or not payload:
        return ""
    context = payload[0]
    endpoints = context.get("Endpoints", {})
    if not isinstance(endpoints, dict):
        return ""
    docker_endpoint = endpoints.get("docker", {})
    if not isinstance(docker_endpoint, dict):
        return ""
    host = docker_endpoint.get("Host")
    return str(host).strip() if isinstance(host, str) else ""


def _template_context(config: dict[str, Any], arm_name: str) -> dict[str, str]:
    run_id = _load_run_id(config)
    arm = _arm(config, arm_name)
    arm_output_dir = ROOT / "benchmarks" / "results" / "local" / run_id / arm_name
    base_context = {
        "run_id": run_id,
        "arm": arm_name,
        "track": config["track"],
        "cohort": config["cohort"],
        "model": config["agent"]["model"],
        "model_slug": config["agent"]["model"].replace("/", "__"),
        "reasoning_effort": config["agent"]["reasoning_effort"],
        "suite": config["benchmark"]["suite"],
        "dataset_name": str(config["benchmark"].get("dataset_name", "")),
        "split": str(config["benchmark"].get("split", "")),
        "subset": str(config["benchmark"].get("subset", "")),
        "language": str(config["benchmark"].get("language", "")),
        "raw_sample_path": _maybe_resolve_repo_path(str(config["benchmark"].get("raw_sample_path", ""))),
        "docker_host": _detect_docker_host(),
        "arm_output_dir": str(arm_output_dir),
        "submission_run_id": f"{run_id}-{arm_name}",
        "tool_profile": arm["tool_profile"],
        "parallelism": str(config["run"]["parallelism"]),
        "max_instances": str(config["run"]["max_instances"]),
        "timeout_minutes": str(config["run"]["timeout_minutes_per_instance"]),
        "timeout_seconds": str(config["run"]["timeout_minutes_per_instance"] * 60),
        "max_turns": str(config["run"]["max_turns_per_instance"]),
        "retry_budget": str(config["run"]["retry_budget"]),
    }
    predictions_path = Path(
        _render_value(str(config["harness"]["predictions_path"]), base_context)
    )
    report_path = Path(
        _render_value(str(config["harness"]["report_path"]), base_context)
    )
    if not predictions_path.is_absolute():
        predictions_path = ROOT / predictions_path
    if not report_path.is_absolute():
        report_path = ROOT / report_path

    context = {
        **base_context,
        "predictions_path": str(predictions_path),
        "report_path": str(report_path),
    }
    for key, value in config["harness"].get("template_vars", {}).items():
        context[key] = _render_value(str(value), context)
    return context


def _render_template_parts(parts: list[str], context: dict[str, str]) -> list[str]:
    rendered = [_render_value(part, context) for part in parts]
    if rendered:
        executable = Path(rendered[0])
        if not executable.is_absolute() and len(executable.parts) > 1:
            rendered[0] = str(ROOT / executable)
    return rendered


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
    working_dir_template = config["harness"].get("working_dir")
    working_dir = (
        _resolve_path_like(_render_value(working_dir_template, context))
        if isinstance(working_dir_template, str) and working_dir_template.strip()
        else ROOT
    )
    environment = {
        key: _render_value(str(value), context)
        for key, value in config["harness"].get("environment", {}).items()
    }
    commands = {
        name: _render_template_parts(parts, context)
        for name, parts in config["harness"]["command_templates"].items()
        if parts is not None
    }
    return {
        "arm": arm_name,
        "context": context,
        "working_dir": str(working_dir),
        "environment": environment,
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
    working_dir = Path(rendered["working_dir"])
    environment = dict(os.environ)
    environment.update(rendered["environment"])
    if dry_run:
        return {
            "arm": arm_name,
            "step": step,
            "command": command,
            "working_dir": str(working_dir),
            "environment": rendered["environment"],
            "dry_run": True,
        }

    proc = subprocess.run(
        command,
        cwd=working_dir,
        text=True,
        capture_output=True,
        env=environment,
    )
    return {
        "arm": arm_name,
        "step": step,
        "command": command,
        "working_dir": str(working_dir),
        "environment": rendered["environment"],
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
