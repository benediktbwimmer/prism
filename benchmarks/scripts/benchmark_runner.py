#!/usr/bin/env python3

from __future__ import annotations

from pathlib import Path
from typing import Any

from benchmark_common import dump_json, file_sha256, git_commit, load_json, make_run_id, utc_now_iso
from benchmark_config import load_normalized_config


def _zero_telemetry_summary() -> dict[str, Any]:
    return {
        "prompt_tokens": 0,
        "completion_tokens": 0,
        "tool_calls": 0,
        "prism_queries": 0,
        "prism_query_calls": 0,
        "prism_compact_tool_calls": 0,
        "prism_payload_bytes": 0,
        "shell_commands": 0,
        "shell_read_commands": 0,
        "repeated_reads": 0,
        "patch_attempts": 0,
        "wall_time_seconds": 0.0,
    }


def _empty_benchmark_outcome() -> dict[str, Any]:
    return {
        "instances_attempted": 0,
        "instances_resolved": 0,
        "instances_unresolved": 0,
        "harness_errors": 0,
        "resolution_rate": 0.0,
    }


def _plan_output_path(result_path: Path) -> Path:
    return result_path.with_name(f"{result_path.stem}.plan.json")


def _display_path(path: Path) -> str:
    try:
        return str(path.relative_to(Path.cwd().resolve()))
    except ValueError:
        return str(path)


def build_plan(config_path: Path) -> dict[str, Any]:
    config = load_normalized_config(config_path)
    generated_at = utc_now_iso()
    commit = git_commit()
    run_id = make_run_id(config["track"], config["cohort"])

    return {
        "version": config["version"],
        "run_id": run_id,
        "generated_at": generated_at,
        "git_commit": commit,
        "track": config["track"],
        "cohort": config["cohort"],
        "description": config.get("description", ""),
        "config_path": config["config_path"],
        "agent": config["agent"],
        "benchmark": {
          "suite": config["benchmark"]["suite"],
          "dataset_name": config["benchmark"].get("dataset_name"),
          "split": config["benchmark"].get("split"),
          "subset": config["benchmark"].get("subset"),
          "language": config["benchmark"].get("language"),
          "raw_sample_path": config["benchmark"].get("raw_sample_path"),
          "instance_manifest": config["benchmark"]["instance_manifest"],
          "instance_count": len(config["instances"]),
        },
        "arms": [
            {
                "name": arm["name"],
                "prism_enabled": arm["prism_enabled"],
                "prompt_path": arm["prompt_path"],
                "prompt_sha256": file_sha256(Path(arm["prompt_abspath"])),
                "tool_profile": arm["tool_profile"],
            }
            for arm in config["arms"]
        ],
        "instances": config["instance_ids"],
        "output": {
            "result_path": config["output"]["result_path"],
            "telemetry_path": config["output"]["telemetry_path"],
            "plan_path": _display_path(_plan_output_path(Path(config["output"]["result_abspath"]))),
        },
    }


def materialize_run(config_path: Path, force: bool = False) -> dict[str, Any]:
    config = load_normalized_config(config_path)
    plan = build_plan(config_path)
    result_path = Path(config["output"]["result_abspath"])
    telemetry_path = Path(config["output"]["telemetry_abspath"])
    plan_path = _plan_output_path(result_path)

    for target in (plan_path, result_path, telemetry_path):
        if target.exists() and not force:
            raise RuntimeError(f"refusing to overwrite existing file without --force: {target}")

    result_payload = {
        "version": config["version"],
        "run_id": plan["run_id"],
        "status": "planned",
        "track": config["track"],
        "cohort": config["cohort"],
        "git_commit": plan["git_commit"],
        "started_at": plan["generated_at"],
        "finished_at": None,
        "config_path": config["config_path"],
        "arms": [
            {
                "name": arm["name"],
                "prism_enabled": arm["prism_enabled"],
                "model": config["agent"]["model"],
                "reasoning_effort": config["agent"]["reasoning_effort"],
                "instance_outcomes": [],
                "benchmark_outcome": _empty_benchmark_outcome(),
                "telemetry_summary": _zero_telemetry_summary(),
            }
            for arm in config["arms"]
        ],
    }

    telemetry_payload = {
        "version": config["version"],
        "run_id": plan["run_id"],
        "status": "planned",
        "track": config["track"],
        "cohort": config["cohort"],
        "git_commit": plan["git_commit"],
        "started_at": plan["generated_at"],
        "finished_at": None,
        "arms": [
            {
                "name": arm["name"],
                "prism_enabled": arm["prism_enabled"],
                "summary": _zero_telemetry_summary(),
                "instances": [],
            }
            for arm in config["arms"]
        ],
    }

    dump_json(plan_path, plan)
    dump_json(result_path, result_payload)
    dump_json(telemetry_path, telemetry_payload)
    return {
        "plan_path": str(plan_path),
        "result_path": str(result_path),
        "telemetry_path": str(telemetry_path),
        "run_id": plan["run_id"],
    }


def _arm_by_name(payload: dict[str, Any], arm_name: str) -> dict[str, Any]:
    for arm in payload["arms"]:
        if arm["name"] == arm_name:
            return arm
    raise KeyError(f"unknown arm: {arm_name}")


def _upsert_instance(items: list[dict[str, Any]], instance_id: str, payload: dict[str, Any]) -> None:
    for index, item in enumerate(items):
        if item["instance_id"] == instance_id:
            items[index] = payload
            return
    items.append(payload)


def _recompute_benchmark_outcome(result_arm: dict[str, Any]) -> None:
    outcomes = result_arm["instance_outcomes"]
    attempted = len(outcomes)
    resolved = sum(1 for item in outcomes if item["status"] == "resolved")
    errors = sum(1 for item in outcomes if item["status"] == "error")
    unresolved = sum(1 for item in outcomes if item["status"] == "unresolved")
    resolution_rate = float(resolved / attempted) if attempted else 0.0

    result_arm["benchmark_outcome"] = {
        "instances_attempted": attempted,
        "instances_resolved": resolved,
        "instances_unresolved": unresolved,
        "harness_errors": errors,
        "resolution_rate": resolution_rate,
    }


def _recompute_telemetry_summary(telemetry_arm: dict[str, Any]) -> None:
    instances = telemetry_arm["instances"]
    summary = _zero_telemetry_summary()
    for item in instances:
        summary["prompt_tokens"] += item["prompt_tokens"]
        summary["completion_tokens"] += item["completion_tokens"]
        summary["tool_calls"] += item["tool_calls"]
        summary["prism_queries"] += item["prism_queries"]
        summary["prism_query_calls"] += item["prism_query_calls"]
        summary["prism_compact_tool_calls"] += item["prism_compact_tool_calls"]
        summary["prism_payload_bytes"] += item["prism_payload_bytes"]
        summary["shell_commands"] += item["shell_commands"]
        summary["shell_read_commands"] += item["shell_read_commands"]
        summary["repeated_reads"] += item["repeated_reads"]
        summary["patch_attempts"] += item["patch_attempts"]
        summary["wall_time_seconds"] += item["wall_time_seconds"]
    telemetry_arm["summary"] = summary


def record_telemetry_instance(
    telemetry_path: Path,
    arm_name: str,
    instance_id: str,
    *,
    resolved: bool | None,
    prompt_tokens: int,
    completion_tokens: int,
    tool_calls: int,
    prism_queries: int,
    prism_query_calls: int,
    prism_compact_tool_calls: int,
    prism_payload_bytes: int,
    shell_commands: int,
    shell_read_commands: int,
    repeated_reads: int,
    patch_attempts: int,
    wall_time_seconds: float,
) -> None:
    telemetry_payload = load_json(telemetry_path)
    telemetry_arm = _arm_by_name(telemetry_payload, arm_name)
    _upsert_instance(
        telemetry_arm["instances"],
        instance_id,
        {
            "instance_id": instance_id,
            "resolved": resolved,
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "tool_calls": tool_calls,
            "prism_queries": prism_queries,
            "prism_query_calls": prism_query_calls,
            "prism_compact_tool_calls": prism_compact_tool_calls,
            "prism_payload_bytes": prism_payload_bytes,
            "shell_commands": shell_commands,
            "shell_read_commands": shell_read_commands,
            "repeated_reads": repeated_reads,
            "patch_attempts": patch_attempts,
            "wall_time_seconds": wall_time_seconds,
        },
    )
    _recompute_telemetry_summary(telemetry_arm)
    dump_json(telemetry_path, telemetry_payload)


def _sync_result_summaries(result_payload: dict[str, Any], telemetry_payload: dict[str, Any]) -> None:
    for result_arm in result_payload["arms"]:
        telemetry_arm = _arm_by_name(telemetry_payload, result_arm["name"])
        _recompute_benchmark_outcome(result_arm)
        _recompute_telemetry_summary(telemetry_arm)
        result_arm["telemetry_summary"] = dict(telemetry_arm["summary"])


def record_instance(
    result_path: Path,
    telemetry_path: Path,
    arm_name: str,
    instance_id: str,
    status: str,
    prompt_tokens: int,
    completion_tokens: int,
    tool_calls: int,
    prism_queries: int,
    shell_commands: int,
    shell_read_commands: int,
    repeated_reads: int,
    patch_attempts: int,
    wall_time_seconds: float,
) -> None:
    result_payload = load_json(result_path)
    telemetry_payload = load_json(telemetry_path)

    if result_payload["run_id"] != telemetry_payload["run_id"]:
        raise RuntimeError("result and telemetry files refer to different runs")

    result_payload["status"] = "running"
    telemetry_payload["status"] = "running"

    result_arm = _arm_by_name(result_payload, arm_name)
    telemetry_arm = _arm_by_name(telemetry_payload, arm_name)

    _upsert_instance(
        result_arm["instance_outcomes"],
        instance_id,
        {
            "instance_id": instance_id,
            "status": status,
        },
    )
    _upsert_instance(
        telemetry_arm["instances"],
        instance_id,
        {
            "instance_id": instance_id,
            "resolved": status == "resolved",
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "tool_calls": tool_calls,
            "prism_queries": prism_queries,
            "shell_commands": shell_commands,
            "shell_read_commands": shell_read_commands,
            "repeated_reads": repeated_reads,
            "patch_attempts": patch_attempts,
            "wall_time_seconds": wall_time_seconds,
        },
    )

    _sync_result_summaries(result_payload, telemetry_payload)
    dump_json(result_path, result_payload)
    dump_json(telemetry_path, telemetry_payload)


def finalize_run(result_path: Path, telemetry_path: Path) -> None:
    result_payload = load_json(result_path)
    telemetry_payload = load_json(telemetry_path)

    if result_payload["run_id"] != telemetry_payload["run_id"]:
        raise RuntimeError("result and telemetry files refer to different runs")

    finished_at = utc_now_iso()
    result_payload["status"] = "completed"
    result_payload["finished_at"] = finished_at
    telemetry_payload["status"] = "completed"
    telemetry_payload["finished_at"] = finished_at

    _sync_result_summaries(result_payload, telemetry_payload)
    dump_json(result_path, result_payload)
    dump_json(telemetry_path, telemetry_payload)
