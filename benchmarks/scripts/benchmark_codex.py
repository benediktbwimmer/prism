#!/usr/bin/env python3

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
from pathlib import Path
from typing import Any

from benchmark_common import ROOT, dump_json, instance_id, load_json
from benchmark_harness import HarnessError, _arm, _load_run_id
from benchmark_runner import record_telemetry_instance
from benchmark_workspace import prepare_isolated_workspace, source_workspace_dir


READ_COMMAND_PREFIXES = (
    "cat ",
    "sed ",
    "rg ",
    "find ",
    "ls",
    "pwd",
    "head ",
    "tail ",
    "wc ",
    "git status",
    "git diff",
    "git show",
)


def _artifact_path(template: str, run_id: str, arm_name: str, instance_name: str) -> Path:
    path = Path(template.format(run_id=run_id, arm=arm_name, instance_id=instance_name))
    return path if path.is_absolute() else ROOT / path


def _find_instance(config: dict[str, Any], instance_name: str) -> dict[str, Any]:
    for item in config["instances"]:
        if instance_id(item) == instance_name:
            return item
    raise HarnessError(f"unknown instance: {instance_name}")


def _instance_prompt(instance: dict[str, Any]) -> str:
    prompt = instance.get("prompt")
    if isinstance(prompt, str) and prompt.strip():
        return prompt
    prompt_path = instance.get("prompt_path")
    if isinstance(prompt_path, str) and prompt_path.strip():
        path = Path(prompt_path)
        if not path.is_absolute():
            path = ROOT / path
        return path.read_text(encoding="utf-8")
    raise HarnessError(
        f"instance `{instance_id(instance)}` is missing prompt content; provide `prompt` or `prompt_path` in the manifest"
    )


def _compose_prompt(
    config: dict[str, Any],
    arm_name: str,
    instance: dict[str, Any],
    workspace_dir: Path,
) -> str:
    arm = _arm(config, arm_name)
    arm_prompt = Path(arm["prompt_abspath"]).read_text(encoding="utf-8").strip()
    body = _instance_prompt(instance).strip()
    workspace_glob = f"{workspace_dir}/**/*"
    return (
        f"{arm_prompt}\n\n"
        f"Benchmark instance: `{instance_id(instance)}`\n"
        f"Working directory: `{workspace_dir}`\n\n"
        "PRISM workspace guidance:\n"
        f"- The benchmark repo for this run lives under `{workspace_dir}`.\n"
        f"- When using PRISM text search, constrain results to this repo with `path: \"{workspace_dir}\"` or `glob: \"{workspace_glob}\"`.\n"
        f"- When using `prism.file(...)`, prefer absolute paths rooted at `{workspace_dir}`.\n"
        f"- Prefer `prism.textSearchBundle(...)` or one scoped `prism.searchText(...)` call before any shell-based code inspection.\n"
        "- Valid file APIs are `prism.file(path).read({ startLine, endLine, maxChars })` and `prism.file(path).around({ line, before, after, maxChars })`.\n"
        "- After a successful PRISM search or PRISM file read, do not reread the same file or rerun the same search with shell tools unless you specifically need raw command output.\n\n"
        "Task:\n"
        f"{body}\n\n"
        "Requirements:\n"
        "- Make the needed code changes directly in the working directory.\n"
        "- Keep the patch as narrow as possible for the named benchmark issue.\n"
        "- After finding a plausible fix, run at least one targeted local test or validation command if a relevant one is discoverable within the time budget.\n"
        "- Prefer targeted validation over additional exploratory reading once the likely fix area is identified.\n"
        "- Do not add broad new fixtures, snapshots, or unrelated test coverage.\n"
        "- In the PRISM arm, do not use shell commands for code inspection before patching unless a concrete PRISM query for that same need has already failed.\n"
        "- If you fall back from PRISM to shell inspection, do it once for the failed need and then continue; do not bounce back and forth between PRISM and shell reads on the same topic.\n"
        "- Do not commit.\n"
        "- Leave the working tree with the intended patch applied.\n"
        "- End with a short summary of what you changed and what local validation you ran, or why you could not run it.\n"
    )


def _build_codex_command(
    config: dict[str, Any],
    arm_name: str,
    workspace_dir: Path,
    last_message_path: Path,
) -> list[str]:
    arm = _arm(config, arm_name)
    execution = config["execution"]

    command = [
        "codex",
        "exec",
        "--json",
        "-C",
        str(workspace_dir),
        "-s",
        execution["sandbox"],
        "-m",
        config["agent"]["model"],
        "-o",
        str(last_message_path),
    ]
    if execution["ephemeral"]:
        command.append("--ephemeral")
    for override in execution["config_overrides"]:
        command.extend(["-c", override.format(reasoning_effort=config["agent"]["reasoning_effort"])])
    for override in arm.get("codex_config_overrides", []):
        command.extend(["-c", override])
    for arg in arm.get("codex_args", []):
        command.append(arg)
    command.append("-")
    return command


def _codex_home_dir(run_id: str, arm_name: str, instance_name: str) -> Path:
    return (
        ROOT
        / "benchmarks"
        / "results"
        / "local"
        / "codex_home"
        / run_id
        / arm_name
        / instance_name
    )


def _prepare_codex_home(run_id: str, arm_name: str, instance_name: str) -> Path:
    source_home = Path.home() / ".codex"
    target_home = _codex_home_dir(run_id, arm_name, instance_name)
    target_home.mkdir(parents=True, exist_ok=True)

    for name in ("config.toml", "auth.json"):
        source = source_home / name
        if source.exists():
            shutil.copy2(source, target_home / name)

    return target_home


def _codex_exec_env(run_id: str, arm_name: str, instance_name: str) -> dict[str, str]:
    env = os.environ.copy()
    env["CODEX_HOME"] = str(_prepare_codex_home(run_id, arm_name, instance_name))
    return env


def _git_diff(workspace_dir: Path) -> str:
    proc = subprocess.run(
        ["git", "-C", str(workspace_dir), "diff", "--binary"],
        text=True,
        capture_output=True,
        check=True,
    )
    return proc.stdout


def _read_predictions(path: Path) -> Any:
    if not path.exists():
        raise HarnessError(
            f"predictions file does not exist: {path}. Run `prepare-predictions` first."
        )
    return load_json(path)


def _write_prediction_entry(
    path: Path,
    predictions_format: str,
    instance_name: str,
    model_name: str,
    patch: str,
) -> None:
    payload = _read_predictions(path)
    if predictions_format == "dict":
        payload.setdefault(instance_name, {})
        payload[instance_name]["model_patch"] = patch
        payload[instance_name]["model_name_or_path"] = model_name
    else:
        updated = False
        for item in payload:
            if item["instance_id"] == instance_name:
                item["model_patch"] = patch
                item["model_name_or_path"] = model_name
                updated = True
                break
        if not updated:
            payload.append(
                {
                    "instance_id": instance_name,
                    "model_patch": patch,
                    "model_name_or_path": model_name,
                }
            )
    dump_json(path, payload)


def _parse_exec_jsonl(stdout: str) -> dict[str, Any]:
    events: list[dict[str, Any]] = []
    for line in stdout.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue

    shell_commands = 0
    shell_read_commands = 0
    prism_queries = 0
    tool_calls = 0
    repeated_reads = 0
    seen_read_commands: dict[str, int] = {}
    usage = {"input_tokens": 0, "output_tokens": 0}

    for event in events:
        event_type = event.get("type")
        if event_type == "item.completed":
            item = event.get("item", {})
            item_type = item.get("type")
            if item_type == "command_execution":
                tool_calls += 1
                shell_commands += 1
                command = str(item.get("command", ""))
                normalized = command.strip()
                if any(normalized.startswith(prefix) or prefix in normalized for prefix in READ_COMMAND_PREFIXES):
                    shell_read_commands += 1
                    seen_read_commands[normalized] = seen_read_commands.get(normalized, 0) + 1
                    if seen_read_commands[normalized] > 1:
                        repeated_reads += 1
            elif item_type in {"mcp_tool_call", "mcp_call"}:
                tool_calls += 1
                if "prism" in json.dumps(item):
                    prism_queries += 1
        elif event_type == "turn.completed":
            usage = event.get("usage", usage)

    return {
        "events": events,
        "prompt_tokens": int(usage.get("input_tokens", 0)),
        "completion_tokens": int(usage.get("output_tokens", 0)),
        "tool_calls": tool_calls,
        "prism_queries": prism_queries,
        "shell_commands": shell_commands,
        "shell_read_commands": shell_read_commands,
        "repeated_reads": repeated_reads,
    }


def run_codex_instance(
    config: dict[str, Any],
    arm_name: str,
    instance_name: str,
    force: bool = False,
) -> dict[str, Any]:
    run_id = _load_run_id(config)
    instance = _find_instance(config, instance_name)
    execution = config["execution"]
    harness = config["harness"]

    transcript_path = _artifact_path(execution["transcript_path"], run_id, arm_name, instance_name)
    stderr_path = _artifact_path(execution["stderr_path"], run_id, arm_name, instance_name)
    last_message_path = _artifact_path(execution["last_message_path"], run_id, arm_name, instance_name)
    patch_path = _artifact_path(execution["patch_path"], run_id, arm_name, instance_name)
    predictions_path = Path(str(harness["predictions_path"]).format(run_id=run_id, arm=arm_name))
    if not predictions_path.is_absolute():
        predictions_path = ROOT / predictions_path

    for path in (transcript_path, stderr_path, last_message_path, patch_path):
        if path.exists() and not force:
            raise HarnessError(
                f"refusing to overwrite existing execution artifact without --force: {path}"
            )
        path.parent.mkdir(parents=True, exist_ok=True)

    source_workspace = source_workspace_dir(instance)
    isolated_workspace = prepare_isolated_workspace(config, run_id, arm_name, instance)

    prompt = _compose_prompt(config, arm_name, instance, isolated_workspace)
    command = _build_codex_command(config, arm_name, isolated_workspace, last_message_path)

    started = time.perf_counter()
    proc = subprocess.run(
        command,
        cwd=ROOT,
        input=prompt,
        text=True,
        capture_output=True,
        env=_codex_exec_env(run_id, arm_name, instance_name),
    )
    wall_time_seconds = time.perf_counter() - started

    transcript_path.write_text(proc.stdout, encoding="utf-8")
    stderr_path.write_text(proc.stderr, encoding="utf-8")

    patch = _git_diff(isolated_workspace)
    patch_path.write_text(patch, encoding="utf-8")
    _write_prediction_entry(
        predictions_path,
        harness["predictions_format"],
        instance_name,
        config["agent"]["model"],
        patch,
    )

    parsed = _parse_exec_jsonl(proc.stdout)
    patch_attempts = 1 if patch.strip() else 0
    telemetry_path = Path(config["output"]["telemetry_abspath"])
    record_telemetry_instance(
        telemetry_path,
        arm_name,
        instance_name,
        resolved=None,
        prompt_tokens=parsed["prompt_tokens"],
        completion_tokens=parsed["completion_tokens"],
        tool_calls=parsed["tool_calls"],
        prism_queries=parsed["prism_queries"],
        shell_commands=parsed["shell_commands"],
        shell_read_commands=parsed["shell_read_commands"],
        repeated_reads=parsed["repeated_reads"],
        patch_attempts=patch_attempts,
        wall_time_seconds=wall_time_seconds,
    )

    return {
        "arm": arm_name,
        "instance_id": instance_name,
        "source_workspace_dir": str(source_workspace),
        "workspace_dir": str(isolated_workspace),
        "returncode": proc.returncode,
        "patch_path": str(patch_path),
        "predictions_path": str(predictions_path),
        "transcript_path": str(transcript_path),
        "stderr_path": str(stderr_path),
        "last_message_path": str(last_message_path),
        "patch_bytes": len(patch.encode("utf-8")),
        "prompt_tokens": parsed["prompt_tokens"],
        "completion_tokens": parsed["completion_tokens"],
        "tool_calls": parsed["tool_calls"],
        "prism_queries": parsed["prism_queries"],
        "shell_commands": parsed["shell_commands"],
        "shell_read_commands": parsed["shell_read_commands"],
        "repeated_reads": parsed["repeated_reads"],
        "wall_time_seconds": wall_time_seconds,
    }


def run_codex_batch(
    config: dict[str, Any],
    arm_name: str,
    *,
    force: bool = False,
    continue_on_error: bool = False,
    instance_names: list[str] | None = None,
) -> dict[str, Any]:
    selected = instance_names or config["instance_ids"]
    completed: list[dict[str, Any]] = []
    failures: list[dict[str, Any]] = []

    for instance_name in selected:
        try:
            completed.append(
                run_codex_instance(
                    config,
                    arm_name,
                    instance_name,
                    force=force,
                )
            )
        except Exception as exc:
            failure = {
                "instance_id": instance_name,
                "error": str(exc),
            }
            failures.append(failure)
            if not continue_on_error:
                return {
                    "arm": arm_name,
                    "attempted": len(completed) + len(failures),
                    "completed": completed,
                    "failures": failures,
                    "stopped_early": True,
                }

    return {
        "arm": arm_name,
        "attempted": len(selected),
        "completed": completed,
        "failures": failures,
        "stopped_early": False,
    }
