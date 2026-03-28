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

COMPACT_PRISM_TOOLS = {
    "prism_locate",
    "prism_gather",
    "prism_open",
    "prism_workset",
    "prism_expand",
}


def _compact_preview_policy(config: dict[str, Any], arm_name: str) -> str:
    arm = _arm(config, arm_name)
    return str(arm.get("compact_preview_policy", "off"))


def _preview_guidance(config: dict[str, Any], arm_name: str) -> str:
    if _compact_preview_policy(config, arm_name) != "adaptive":
        return ""
    return (
        "- Adaptive preview policy: on `prism_locate`, request `includeTopPreview: true` when you mainly need to confirm the top candidate or glance at a likely signature/heading before opening it.\n"
        "- On `prism_expand` with `kind: \"neighbors\"`, request `includeTopPreview: true` only when the top neighbor is likely the next read and a short teaser may save a full `prism_open`.\n"
        "- If a preview already answers the immediate question, skip `prism_open` and continue with the handle you have.\n"
        "- If the preview is truncated or clearly insufficient, escalate once with `prism_open` instead of repeating more preview requests.\n"
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
    preview_guidance = _preview_guidance(config, arm_name)
    return (
        f"{arm_prompt}\n\n"
        f"Benchmark instance: `{instance_id(instance)}`\n"
        f"Working directory: `{workspace_dir}`\n\n"
        "PRISM workspace guidance:\n"
        f"- The benchmark repo for this run lives under `{workspace_dir}`.\n"
        "- Preferred staged PRISM path: `prism_locate`, then `prism_gather` for exact-text/config/schema/script slices when that is cheaper, then `prism_open`, then `prism_workset`, and `prism_expand` only if needed.\n"
        "- Treat `prism_query` as an explicit fallback only when the compact staged tools cannot express the need.\n"
        f"- Constrain PRISM searches to this repo with `path: \"{workspace_dir}\"` or `glob: \"{workspace_glob}\"` when the tool supports it.\n"
        "- Carry forward compact PRISM handles instead of rediscovering the same target by text.\n"
        "- After a successful compact PRISM locate/gather/open/workset call, do not reread that same target through shell tools unless you specifically need raw command output.\n\n"
        f"{preview_guidance}"
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
    prism_query_calls = 0
    prism_compact_tool_calls = 0
    compact_tool_counts = {tool: 0 for tool in COMPACT_PRISM_TOOLS}
    locate_preview_requests = 0
    locate_preview_hits = 0
    locate_preview_bytes = 0
    locate_preview_direct_opens = 0
    locate_preview_direct_progressions = 0
    expand_preview_requests = 0
    expand_preview_hits = 0
    expand_preview_bytes = 0
    expand_preview_direct_opens = 0
    expand_preview_direct_progressions = 0
    tool_calls = 0
    repeated_reads = 0
    seen_read_commands: dict[str, int] = {}
    usage = {"input_tokens": 0, "output_tokens": 0}
    prism_payload_bytes = 0
    pending_preview: dict[str, str] | None = None

    def _unwrap_tool_result(result: Any) -> Any:
        if not isinstance(result, dict):
            return result
        structured = result.get("structured_content")
        if structured is not None:
            return structured
        content = result.get("content")
        if isinstance(content, list):
            for item in content:
                if not isinstance(item, dict):
                    continue
                text = item.get("text")
                if not isinstance(text, str):
                    continue
                try:
                    return json.loads(text)
                except json.JSONDecodeError:
                    continue
        return result

    def _serialized_len(value: Any) -> int:
        return len(json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8"))

    def _preview_requested(tool_name: str, arguments: Any) -> bool:
        if not isinstance(arguments, dict):
            return False
        include_top_preview = arguments.get("includeTopPreview")
        if include_top_preview is None:
            include_top_preview = arguments.get("include_top_preview")
        if tool_name == "prism_expand":
            return bool(include_top_preview) and arguments.get("kind") == "neighbors"
        return tool_name == "prism_locate" and bool(include_top_preview)

    def _preview_source(tool_name: str) -> str | None:
        if tool_name == "prism_locate":
            return "locate"
        if tool_name == "prism_expand":
            return "expand"
        return None

    def _extract_preview_handle(payload: Any) -> str | None:
        if not isinstance(payload, dict):
            return None
        top_preview = payload.get("topPreview")
        if not isinstance(top_preview, dict):
            return None
        handle = top_preview.get("handle")
        if isinstance(handle, str) and handle:
            return handle
        return None

    def _extract_preview_value(payload: Any) -> Any:
        if not isinstance(payload, dict):
            return None
        return payload.get("topPreview")

    def _followup_handle(tool_name: str, arguments: Any) -> str | None:
        if tool_name not in COMPACT_PRISM_TOOLS or not isinstance(arguments, dict):
            return None
        handle = arguments.get("handle")
        if isinstance(handle, str) and handle:
            return handle
        return None

    for event in events:
        event_type = event.get("type")
        if event_type == "item.completed":
            item = event.get("item", {})
            item_type = item.get("type")
            if item_type == "command_execution":
                tool_calls += 1
                shell_commands += 1
                pending_preview = None
                command = str(item.get("command", ""))
                normalized = command.strip()
                if any(normalized.startswith(prefix) or prefix in normalized for prefix in READ_COMMAND_PREFIXES):
                    shell_read_commands += 1
                    seen_read_commands[normalized] = seen_read_commands.get(normalized, 0) + 1
                    if seen_read_commands[normalized] > 1:
                        repeated_reads += 1
            elif item_type in {"mcp_tool_call", "mcp_call"}:
                tool_calls += 1
                tool_name = str(item.get("tool", ""))
                arguments = item.get("arguments")
                if item.get("server") == "prism":
                    prism_queries += 1
                    if tool_name == "prism_query":
                        prism_query_calls += 1
                        pending_preview = None
                    elif tool_name in COMPACT_PRISM_TOOLS:
                        prism_compact_tool_calls += 1
                        compact_tool_counts[tool_name] += 1
                        followup_handle = _followup_handle(tool_name, arguments)
                        if pending_preview is not None:
                            if followup_handle == pending_preview["handle"]:
                                if pending_preview["source"] == "locate":
                                    if tool_name == "prism_open":
                                        locate_preview_direct_opens += 1
                                    elif tool_name in {"prism_workset", "prism_expand"}:
                                        locate_preview_direct_progressions += 1
                                elif pending_preview["source"] == "expand":
                                    if tool_name == "prism_open":
                                        expand_preview_direct_opens += 1
                                    elif tool_name in {"prism_workset", "prism_expand"}:
                                        expand_preview_direct_progressions += 1
                            pending_preview = None
                    else:
                        pending_preview = None
                    result_payload = _unwrap_tool_result(item.get("result"))
                    if _preview_requested(tool_name, arguments):
                        preview_source = _preview_source(tool_name)
                        preview_handle = _extract_preview_handle(result_payload)
                        preview_value = _extract_preview_value(result_payload)
                        if preview_source == "locate":
                            locate_preview_requests += 1
                            if preview_handle is not None and preview_value is not None:
                                locate_preview_hits += 1
                                locate_preview_bytes += _serialized_len(preview_value)
                                pending_preview = {"source": "locate", "handle": preview_handle}
                        elif preview_source == "expand":
                            expand_preview_requests += 1
                            if preview_handle is not None and preview_value is not None:
                                expand_preview_hits += 1
                                expand_preview_bytes += _serialized_len(preview_value)
                                pending_preview = {"source": "expand", "handle": preview_handle}
                    result = item.get("result")
                    if result is not None:
                        prism_payload_bytes += _serialized_len(result)
                else:
                    pending_preview = None
        elif event_type == "turn.completed":
            usage = event.get("usage", usage)

    return {
        "events": events,
        "prompt_tokens": int(usage.get("input_tokens", 0)),
        "completion_tokens": int(usage.get("output_tokens", 0)),
        "tool_calls": tool_calls,
        "prism_queries": prism_queries,
        "prism_query_calls": prism_query_calls,
        "prism_compact_tool_calls": prism_compact_tool_calls,
        "prism_locate_calls": compact_tool_counts["prism_locate"],
        "prism_gather_calls": compact_tool_counts["prism_gather"],
        "prism_open_calls": compact_tool_counts["prism_open"],
        "prism_workset_calls": compact_tool_counts["prism_workset"],
        "prism_expand_calls": compact_tool_counts["prism_expand"],
        "locate_preview_requests": locate_preview_requests,
        "locate_preview_hits": locate_preview_hits,
        "locate_preview_bytes": locate_preview_bytes,
        "locate_preview_direct_opens": locate_preview_direct_opens,
        "locate_preview_direct_progressions": locate_preview_direct_progressions,
        "expand_preview_requests": expand_preview_requests,
        "expand_preview_hits": expand_preview_hits,
        "expand_preview_bytes": expand_preview_bytes,
        "expand_preview_direct_opens": expand_preview_direct_opens,
        "expand_preview_direct_progressions": expand_preview_direct_progressions,
        "prism_payload_bytes": prism_payload_bytes,
        "shell_commands": shell_commands,
        "shell_read_commands": shell_read_commands,
        "repeated_reads": repeated_reads,
    }


def _daemon_log_path(workspace_dir: Path) -> Path:
    return workspace_dir / ".prism" / "prism-mcp-daemon.log"


def _daemon_log_offset(workspace_dir: Path) -> int:
    path = _daemon_log_path(workspace_dir)
    if not path.exists():
        return 0
    return path.stat().st_size


def _zero_compact_timing_summary() -> dict[str, int]:
    return {
        "compact_query_duration_ms": 0,
        "compact_refresh_duration_ms": 0,
        "compact_handler_duration_ms": 0,
        "compact_other_duration_ms": 0,
        "prism_locate_duration_ms": 0,
        "prism_gather_duration_ms": 0,
        "prism_open_duration_ms": 0,
        "prism_workset_duration_ms": 0,
        "prism_expand_duration_ms": 0,
    }


def _parse_compact_query_timings(workspace_dir: Path, start_offset: int) -> dict[str, int]:
    path = _daemon_log_path(workspace_dir)
    summary = _zero_compact_timing_summary()
    if not path.exists():
        return summary

    with path.open("r", encoding="utf-8", errors="replace") as handle:
        try:
            handle.seek(start_offset)
        except OSError:
            handle.seek(0)
        for line in handle:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get("message") != "compact query timing":
                continue
            tool = event.get("tool")
            if tool not in COMPACT_PRISM_TOOLS:
                continue
            total_ms = int(event.get("total_ms", 0) or 0)
            refresh_ms = int(event.get("refresh_ms", 0) or 0)
            handler_ms = int(event.get("handler_ms", 0) or 0)
            other_ms = int(event.get("other_ms", 0) or 0)
            summary["compact_query_duration_ms"] += total_ms
            summary["compact_refresh_duration_ms"] += refresh_ms
            summary["compact_handler_duration_ms"] += handler_ms
            summary["compact_other_duration_ms"] += other_ms
            summary[f"{tool}_duration_ms"] += total_ms
    return summary


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
    daemon_log_offset = _daemon_log_offset(isolated_workspace)

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
    compact_timings = _parse_compact_query_timings(isolated_workspace, daemon_log_offset)
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
        prism_query_calls=parsed["prism_query_calls"],
        prism_compact_tool_calls=parsed["prism_compact_tool_calls"],
        prism_locate_calls=parsed["prism_locate_calls"],
        prism_gather_calls=parsed["prism_gather_calls"],
        prism_open_calls=parsed["prism_open_calls"],
        prism_workset_calls=parsed["prism_workset_calls"],
        prism_expand_calls=parsed["prism_expand_calls"],
        locate_preview_requests=parsed["locate_preview_requests"],
        locate_preview_hits=parsed["locate_preview_hits"],
        locate_preview_bytes=parsed["locate_preview_bytes"],
        locate_preview_direct_opens=parsed["locate_preview_direct_opens"],
        locate_preview_direct_progressions=parsed["locate_preview_direct_progressions"],
        expand_preview_requests=parsed["expand_preview_requests"],
        expand_preview_hits=parsed["expand_preview_hits"],
        expand_preview_bytes=parsed["expand_preview_bytes"],
        expand_preview_direct_opens=parsed["expand_preview_direct_opens"],
        expand_preview_direct_progressions=parsed["expand_preview_direct_progressions"],
        prism_payload_bytes=parsed["prism_payload_bytes"],
        compact_query_duration_ms=compact_timings["compact_query_duration_ms"],
        compact_refresh_duration_ms=compact_timings["compact_refresh_duration_ms"],
        compact_handler_duration_ms=compact_timings["compact_handler_duration_ms"],
        compact_other_duration_ms=compact_timings["compact_other_duration_ms"],
        prism_locate_duration_ms=compact_timings["prism_locate_duration_ms"],
        prism_gather_duration_ms=compact_timings["prism_gather_duration_ms"],
        prism_open_duration_ms=compact_timings["prism_open_duration_ms"],
        prism_workset_duration_ms=compact_timings["prism_workset_duration_ms"],
        prism_expand_duration_ms=compact_timings["prism_expand_duration_ms"],
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
        "prism_query_calls": parsed["prism_query_calls"],
        "prism_compact_tool_calls": parsed["prism_compact_tool_calls"],
        "prism_locate_calls": parsed["prism_locate_calls"],
        "prism_gather_calls": parsed["prism_gather_calls"],
        "prism_open_calls": parsed["prism_open_calls"],
        "prism_workset_calls": parsed["prism_workset_calls"],
        "prism_expand_calls": parsed["prism_expand_calls"],
        "locate_preview_requests": parsed["locate_preview_requests"],
        "locate_preview_hits": parsed["locate_preview_hits"],
        "locate_preview_bytes": parsed["locate_preview_bytes"],
        "locate_preview_direct_opens": parsed["locate_preview_direct_opens"],
        "locate_preview_direct_progressions": parsed["locate_preview_direct_progressions"],
        "expand_preview_requests": parsed["expand_preview_requests"],
        "expand_preview_hits": parsed["expand_preview_hits"],
        "expand_preview_bytes": parsed["expand_preview_bytes"],
        "expand_preview_direct_opens": parsed["expand_preview_direct_opens"],
        "expand_preview_direct_progressions": parsed["expand_preview_direct_progressions"],
        "prism_payload_bytes": parsed["prism_payload_bytes"],
        "compact_query_duration_ms": compact_timings["compact_query_duration_ms"],
        "compact_refresh_duration_ms": compact_timings["compact_refresh_duration_ms"],
        "compact_handler_duration_ms": compact_timings["compact_handler_duration_ms"],
        "compact_other_duration_ms": compact_timings["compact_other_duration_ms"],
        "prism_locate_duration_ms": compact_timings["prism_locate_duration_ms"],
        "prism_gather_duration_ms": compact_timings["prism_gather_duration_ms"],
        "prism_open_duration_ms": compact_timings["prism_open_duration_ms"],
        "prism_workset_duration_ms": compact_timings["prism_workset_duration_ms"],
        "prism_expand_duration_ms": compact_timings["prism_expand_duration_ms"],
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
