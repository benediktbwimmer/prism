#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path
from typing import Any

from benchmark_common import ROOT, load_manifest_entries
from benchmark_config import ConfigError


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Clone or refresh local GitHub repo mirrors for benchmark instances."
    )
    parser.add_argument("--dataset", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--github-host", default="https://github.com")
    return parser.parse_args()


def _load_dataset_records(path: Path) -> list[dict[str, Any]]:
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


def _dataset_map(records: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    mapping: dict[str, dict[str, Any]] = {}
    for record in records:
        raw = record.get("instance_id")
        if not isinstance(raw, str) or not raw.strip():
            raise ConfigError(f"dataset record is missing `instance_id`: {record}")
        if raw in mapping:
            raise ConfigError(f"duplicate dataset record for instance_id `{raw}`")
        mapping[raw] = record
    return mapping


def _repo_slug(repo: str) -> str:
    return repo.replace("/", "__")


def _repo_dir(output_dir: Path, repo: str) -> Path:
    return output_dir / _repo_slug(repo)


def _git(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
        cwd=cwd,
        check=True,
        text=True,
        capture_output=True,
    )


def _ensure_repo(path: Path, repo: str, github_host: str) -> str:
    remote_url = f"{github_host.rstrip('/')}/{repo}.git"
    if path.exists():
        actual = _git("-C", str(path), "remote", "get-url", "origin").stdout.strip()
        if actual != remote_url:
            raise ConfigError(
                f"existing repo mirror has unexpected origin for `{repo}`: {actual} != {remote_url}"
            )
        _git("-C", str(path), "fetch", "--tags", "--force", "--prune", "origin")
        return "fetched"

    path.parent.mkdir(parents=True, exist_ok=True)
    _git("clone", remote_url, str(path))
    return "cloned"


def sync_repo_mirrors(
    dataset_path: Path,
    output_dir: Path,
    *,
    manifest_path: Path | None = None,
    github_host: str = "https://github.com",
) -> dict[str, Any]:
    dataset = _dataset_map(_load_dataset_records(dataset_path))
    instance_ids = list(dataset.keys())
    if manifest_path is not None:
        manifest_entries = load_manifest_entries(manifest_path)
        instance_ids = [str(entry["instance_id"]) for entry in manifest_entries]
        missing = [instance_id for instance_id in instance_ids if instance_id not in dataset]
        if missing:
            raise ConfigError(
                f"manifest `{manifest_path}` references {len(missing)} missing dataset instances: "
                + ", ".join(missing[:10])
            )

    repos = sorted(
        {
            str(dataset[instance_id]["repo"])
            for instance_id in instance_ids
            if isinstance(dataset[instance_id].get("repo"), str) and dataset[instance_id]["repo"].strip()
        }
    )
    if not repos:
        raise ConfigError("no repos were found for the selected dataset/manifest slice")

    output_dir = output_dir if output_dir.is_absolute() else (ROOT / output_dir).resolve()
    actions = []
    for repo in repos:
        path = _repo_dir(output_dir, repo)
        actions.append(
            {
                "repo": repo,
                "path": str(path),
                "action": _ensure_repo(path, repo, github_host),
            }
        )

    return {
        "dataset": str(dataset_path.resolve()),
        "manifest": str(manifest_path.resolve()) if manifest_path else None,
        "output_dir": str(output_dir),
        "repo_count": len(repos),
        "instance_count": len(instance_ids),
        "repo_template": str(output_dir / "{repo_slug}"),
        "actions": actions,
    }


def main() -> int:
    args = _parse_args()
    result = sync_repo_mirrors(
        args.dataset,
        args.output_dir,
        manifest_path=args.manifest,
        github_host=args.github_host,
    )
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
