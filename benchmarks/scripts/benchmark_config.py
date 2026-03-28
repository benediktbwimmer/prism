#!/usr/bin/env python3

from __future__ import annotations

from pathlib import Path
from typing import Any

from benchmark_common import ROOT, TRACKS, instance_id, load_json, load_manifest_entries


class ConfigError(RuntimeError):
    pass


def find_config_files() -> list[Path]:
    return sorted(TRACKS.glob("*/configs/*.json"))


def _require(config: dict[str, Any], key: str, errors: list[str], ctx: str) -> None:
    if key not in config:
        errors.append(f"{ctx}: missing required key `{key}`")


def _validate_arm(arm: dict[str, Any], path: Path, errors: list[str]) -> None:
    for key in ("name", "prism_enabled", "prompt_path", "tool_profile"):
        _require(arm, key, errors, str(path))
    prompt_path = ROOT / arm.get("prompt_path", "")
    if "prompt_path" in arm and not prompt_path.exists():
        errors.append(f"{path}: prompt path does not exist: {arm['prompt_path']}")
    preview_policy = arm.get("compact_preview_policy", "off")
    if preview_policy not in {"off", "adaptive"}:
        errors.append(
            f"{path}: unsupported compact_preview_policy `{preview_policy}`; expected `off` or `adaptive`"
        )


def _validate_execution(execution: dict[str, Any], path: Path, errors: list[str]) -> None:
    for key in (
        "adapter",
        "sandbox",
        "ephemeral",
        "transcript_path",
        "stderr_path",
        "last_message_path",
        "patch_path",
        "config_overrides",
    ):
        _require(execution, key, errors, f"{path}:execution")


def validate_config_payload(config: dict[str, Any], path: Path) -> list[str]:
    errors: list[str] = []

    for key in (
        "version",
        "track",
        "cohort",
        "agent",
        "benchmark",
        "harness",
        "execution",
        "arms",
        "run",
        "telemetry",
        "output",
    ):
        _require(config, key, errors, str(path))

    agent = config.get("agent", {})
    for key in ("name", "model", "reasoning_effort"):
        _require(agent, key, errors, f"{path}:agent")

    benchmark = config.get("benchmark", {})
    _require(benchmark, "suite", errors, f"{path}:benchmark")
    _require(benchmark, "instance_manifest", errors, f"{path}:benchmark")
    manifest_path = ROOT / benchmark.get("instance_manifest", "")
    if "instance_manifest" in benchmark and not manifest_path.exists():
        errors.append(f"{path}: manifest path does not exist: {benchmark['instance_manifest']}")

    harness = config.get("harness", {})
    for key in ("adapter", "predictions_path", "predictions_format", "report_path", "command_templates", "report_parser"):
        _require(harness, key, errors, f"{path}:harness")
    command_templates = harness.get("command_templates", {})
    if "submit" not in command_templates:
        errors.append(f"{path}:harness: missing required command template `submit`")

    execution = config.get("execution", {})
    _validate_execution(execution, path, errors)

    arms = config.get("arms", [])
    if len(arms) != 2:
        errors.append(f"{path}: expected exactly 2 arms, found {len(arms)}")
    else:
        names = {arm.get("name") for arm in arms}
        if names != {"control", "prism"}:
            errors.append(f"{path}: arm names must be `control` and `prism`")
        prism_values = {arm.get("name"): arm.get("prism_enabled") for arm in arms}
        if prism_values.get("control") is not False:
            errors.append(f"{path}: `control` arm must set prism_enabled=false")
        if prism_values.get("prism") is not True:
            errors.append(f"{path}: `prism` arm must set prism_enabled=true")
        for arm in arms:
            _validate_arm(arm, path, errors)

    run = config.get("run", {})
    for key in ("max_instances", "max_turns_per_instance", "timeout_minutes_per_instance", "parallelism", "retry_budget"):
        _require(run, key, errors, f"{path}:run")

    telemetry = config.get("telemetry", {})
    for key in (
        "capture_tokens",
        "capture_tool_calls",
        "capture_prism_queries",
        "capture_shell_reads",
        "capture_wall_time",
        "capture_patch_attempts",
    ):
        _require(telemetry, key, errors, f"{path}:telemetry")

    output = config.get("output", {})
    for key in ("result_path", "telemetry_path"):
        _require(output, key, errors, f"{path}:output")

    return errors


def validate_config_file(path: Path) -> list[str]:
    return validate_config_payload(load_json(path), path)


def load_normalized_config(path: Path, require_instances: bool = False) -> dict[str, Any]:
    resolved_path = path if path.is_absolute() else (ROOT / path)
    config = load_json(resolved_path)
    errors = validate_config_payload(config, resolved_path)
    if errors:
        raise ConfigError("\n".join(errors))

    manifest_path = ROOT / config["benchmark"]["instance_manifest"]
    instances = load_manifest_entries(manifest_path)
    max_instances = config["run"]["max_instances"]
    if max_instances is not None:
        instances = instances[:max_instances]
    if require_instances and not instances:
        raise ConfigError(f"{path}: no benchmark instances resolved from manifest")

    normalized = dict(config)
    try:
        normalized["config_path"] = str(resolved_path.relative_to(ROOT))
    except ValueError:
        normalized["config_path"] = str(resolved_path)
    normalized["benchmark"] = dict(config["benchmark"])
    normalized["benchmark"]["instance_manifest_path"] = str(manifest_path)
    normalized["harness"] = dict(config["harness"])
    normalized["execution"] = dict(config["execution"])
    normalized["instances"] = instances
    normalized["instance_ids"] = [instance_id(item) for item in instances]
    normalized["arms"] = [dict(arm) for arm in config["arms"]]
    for arm in normalized["arms"]:
        arm["compact_preview_policy"] = arm.get("compact_preview_policy", "off")
        arm["prompt_abspath"] = str((ROOT / arm["prompt_path"]).resolve())
    normalized["output"] = dict(config["output"])
    normalized["output"]["result_abspath"] = str((ROOT / config["output"]["result_path"]).resolve())
    normalized["output"]["telemetry_abspath"] = str((ROOT / config["output"]["telemetry_path"]).resolve())
    return normalized
