#!/usr/bin/env python3

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from benchmark_common import BENCHMARKS, ROOT, dump_json, dump_jsonl, load_json
from benchmark_config import ConfigError, load_normalized_config
from benchmark_harness import HarnessError
from benchmark_workspace import prepare_detached_worktree


def _load_dataset_records(path: Path) -> list[dict[str, Any]]:
    if path.suffix == ".jsonl":
        records: list[dict[str, Any]] = []
        with path.open("r", encoding="utf-8") as handle:
            for raw_line in handle:
                line = raw_line.strip()
                if not line or line.startswith("#"):
                    continue
                payload = json.loads(line)
                if not isinstance(payload, dict):
                    raise ConfigError(f"dataset line is not an object: {line}")
                records.append(payload)
        return records

    payload = load_json(path)
    if not isinstance(payload, list):
        raise ConfigError(f"dataset file must be a JSON array or JSONL object stream: {path}")
    records = [item for item in payload if isinstance(item, dict)]
    if len(records) != len(payload):
        raise ConfigError(f"dataset file contains non-object items: {path}")
    return records


def _record_key(record: dict[str, Any]) -> str:
    raw = record.get("instance_id") or record.get("id")
    if not isinstance(raw, str) or not raw.strip():
        raise ConfigError(f"dataset record is missing `instance_id`: {record}")
    return raw


def _dataset_map(records: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    mapping: dict[str, dict[str, Any]] = {}
    for record in records:
        key = _record_key(record)
        if key in mapping:
            raise ConfigError(f"duplicate dataset record for instance_id `{key}`")
        mapping[key] = record
    return mapping


def _repo_slug(repo: str) -> str:
    return repo.replace("/", "__")


def _resolve_repo_source(record: dict[str, Any], repo_template: str | None) -> Path:
    repo_path = record.get("repo_path") or record.get("repo_dir")
    if isinstance(repo_path, str) and repo_path.strip():
        path = Path(repo_path)
        return path if path.is_absolute() else (ROOT / path).resolve()

    repo = record.get("repo")
    if not isinstance(repo, str) or not repo.strip():
        raise ConfigError(
            f"instance `{_record_key(record)}` is missing `repo` and does not provide `repo_path`"
        )
    if not repo_template:
        raise ConfigError(
            f"instance `{_record_key(record)}` requires `--repo-template` when dataset records omit `repo_path`"
        )

    path = Path(repo_template.format(repo=repo, repo_slug=_repo_slug(repo)))
    return path if path.is_absolute() else (ROOT / path).resolve()


def _base_commit(record: dict[str, Any]) -> str:
    for key in ("base_commit", "base_sha", "base_ref"):
        value = record.get(key)
        if isinstance(value, str) and value.strip():
            return value
    raise ConfigError(f"instance `{_record_key(record)}` is missing a base commit field")


def _workspace_subdir(record: dict[str, Any]) -> Path:
    raw = record.get("workspace_subdir")
    if not isinstance(raw, str) or not raw.strip():
        return Path(".")
    path = Path(raw)
    if path.is_absolute():
        raise ConfigError(f"instance `{_record_key(record)}` has absolute workspace_subdir: {path}")
    return path


def _problem_statement(record: dict[str, Any]) -> str:
    for key in ("prompt", "problem_statement", "task", "issue_text"):
        value = record.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    raise ConfigError(
        f"instance `{_record_key(record)}` is missing prompt/problem_statement/task text"
    )


def _render_list_section(title: str, value: Any) -> str | None:
    if isinstance(value, str) and value.strip():
        return f"## {title}\n\n{value.strip()}\n"
    if isinstance(value, list) and value:
        lines = [f"- {item}" for item in value if isinstance(item, str) and item.strip()]
        if lines:
            return f"## {title}\n\n" + "\n".join(lines) + "\n"
    return None


def _render_prompt(record: dict[str, Any]) -> str:
    explicit = record.get("prompt")
    if isinstance(explicit, str) and explicit.strip() and "problem_statement" not in record:
        return explicit.strip() + "\n"

    parts = [f"# Benchmark Instance `{_record_key(record)}`", ""]

    repo = record.get("repo")
    if isinstance(repo, str) and repo.strip():
        parts.append(f"- Repository: `{repo}`")
    parts.append(f"- Base commit: `{_base_commit(record)}`")
    language = record.get("language")
    if isinstance(language, str) and language.strip():
        parts.append(f"- Language: `{language}`")

    subdir = _workspace_subdir(record)
    if subdir != Path("."):
        parts.append(f"- Workspace subdirectory: `{subdir.as_posix()}`")

    parts.extend(["", "## Problem Statement", "", _problem_statement(record), ""])

    for title, key in (
        ("Hints", "hints"),
        ("Fail To Pass", "fail_to_pass"),
        ("Pass To Pass", "pass_to_pass"),
    ):
        rendered = _render_list_section(title, record.get(key))
        if rendered:
            parts.append(rendered.rstrip())
            parts.append("")

    return "\n".join(parts).rstrip() + "\n"


def _prepared_root(config: dict[str, Any], output_dir: Path | None) -> Path:
    if output_dir is not None:
        return output_dir.resolve()
    return (BENCHMARKS / "results" / "local" / "prepared" / f"{config['track']}-{config['cohort']}").resolve()


def _relative_to_root(path: Path) -> str:
    try:
        return str(path.resolve().relative_to(ROOT))
    except ValueError:
        return str(path.resolve())


def _materialize_instance(
    prepared_root: Path,
    record: dict[str, Any],
    repo_template: str | None,
    force: bool,
) -> dict[str, Any]:
    key = _record_key(record)
    source_repo = _resolve_repo_source(record, repo_template)
    if not source_repo.exists():
        raise ConfigError(f"source repo for `{key}` does not exist: {source_repo}")

    dest_repo = prepared_root / "sources" / key / "repo"
    if dest_repo.exists() and not force:
        raise HarnessError(
            f"prepared source workspace already exists; rerun with --force to replace it: {dest_repo}"
        )
    prepare_detached_worktree(source_repo, dest_repo, _base_commit(record))

    workspace_dir = (dest_repo / _workspace_subdir(record)).resolve()
    if not workspace_dir.exists():
        raise ConfigError(f"prepared workspace path is missing for `{key}`: {workspace_dir}")

    prompt_path = prepared_root / "prompts" / f"{key}.md"
    prompt_path.parent.mkdir(parents=True, exist_ok=True)
    prompt_path.write_text(_render_prompt(record), encoding="utf-8")

    manifest_entry = {
        "instance_id": key,
        "workspace_dir": str(workspace_dir),
        "prompt_path": str(prompt_path.resolve()),
        "source_repo_dir": str(source_repo.resolve()),
        "base_commit": _base_commit(record),
    }
    for optional_key in ("repo", "language", "hints", "fail_to_pass", "pass_to_pass"):
        if optional_key in record:
            manifest_entry[optional_key] = record[optional_key]
    return manifest_entry


def _write_prepared_config(
    raw_config: dict[str, Any],
    config_path: Path,
    prepared_root: Path,
    prepared_manifest: Path,
    dataset_path: Path,
) -> Path:
    prepared_config = dict(raw_config)
    prepared_benchmark = dict(raw_config["benchmark"])
    prepared_benchmark["instance_manifest"] = _relative_to_root(prepared_manifest)
    prepared_benchmark["prepared_from_config"] = _relative_to_root(config_path)
    prepared_benchmark["prepared_from_dataset"] = _relative_to_root(dataset_path)
    prepared_config["benchmark"] = prepared_benchmark

    prepared_config_path = prepared_root / "config.json"
    dump_json(prepared_config_path, prepared_config)
    return prepared_config_path


def prepare_track(
    config_path: Path,
    dataset_path: Path,
    *,
    repo_template: str | None = None,
    output_dir: Path | None = None,
    force: bool = False,
) -> dict[str, Any]:
    config = load_normalized_config(config_path, require_instances=True)
    raw_config = load_json(config_path if config_path.is_absolute() else ROOT / config_path)
    prepared_root = _prepared_root(config, output_dir)

    records = _dataset_map(_load_dataset_records(dataset_path))
    missing = [key for key in config["instance_ids"] if key not in records]
    if missing:
        raise ConfigError(
            f"dataset `{dataset_path}` is missing {len(missing)} configured instances: {', '.join(missing[:10])}"
        )

    manifest_entries = [
        _materialize_instance(prepared_root, records[key], repo_template, force)
        for key in config["instance_ids"]
    ]

    prepared_manifest = prepared_root / "manifest.jsonl"
    dump_jsonl(prepared_manifest, manifest_entries)
    prepared_config_path = _write_prepared_config(
        raw_config,
        config_path if config_path.is_absolute() else (ROOT / config_path),
        prepared_root,
        prepared_manifest,
        dataset_path.resolve(),
    )

    return {
        "track": config["track"],
        "cohort": config["cohort"],
        "instance_count": len(manifest_entries),
        "prepared_root": str(prepared_root),
        "prepared_manifest": str(prepared_manifest),
        "prepared_config": str(prepared_config_path),
        "dataset_path": str(dataset_path.resolve()),
    }
