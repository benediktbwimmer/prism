#!/usr/bin/env python3

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path
from typing import Any

from benchmark_common import ROOT, instance_id
from benchmark_harness import HarnessError


def source_workspace_dir(instance: dict[str, Any]) -> Path:
    raw = instance.get("workspace_dir")
    if not isinstance(raw, str) or not raw.strip():
        raise HarnessError(
            f"instance `{instance_id(instance)}` is missing `workspace_dir` in the manifest"
        )
    path = Path(raw)
    if not path.is_absolute():
        path = ROOT / path
    if not path.exists():
        raise HarnessError(f"workspace_dir does not exist for `{instance_id(instance)}`: {path}")
    return path.resolve()


def _git_output(repo_dir: Path, *args: str) -> str:
    proc = subprocess.run(
        ["git", "-C", str(repo_dir), *args],
        text=True,
        capture_output=True,
        check=True,
    )
    return proc.stdout.strip()


def _source_repo_layout(instance: dict[str, Any]) -> tuple[Path, Path]:
    workspace_dir = source_workspace_dir(instance)
    repo_root = Path(_git_output(workspace_dir, "rev-parse", "--show-toplevel")).resolve()
    try:
        relative_dir = workspace_dir.relative_to(repo_root)
    except ValueError as exc:
        raise HarnessError(
            f"workspace_dir for `{instance_id(instance)}` is not inside its git repo root: {workspace_dir}"
        ) from exc
    return repo_root, relative_dir


def isolated_repo_dir(
    config: dict[str, Any],
    run_id: str,
    arm_name: str,
    instance_name: str,
) -> Path:
    result_dir = Path(config["output"]["result_abspath"]).parent
    execution = config.get("execution", {})
    template = execution.get("workspace_path")
    if isinstance(template, str) and template.strip():
        path = Path(template.format(run_id=run_id, arm=arm_name, instance_id=instance_name))
        return path if path.is_absolute() else ROOT / path
    return result_dir / arm_name / "workspaces" / instance_name / "repo"


def _remove_existing_isolated_repo(source_repo: Path, dest_repo: Path) -> None:
    if not dest_repo.exists():
        return
    proc = subprocess.run(
        ["git", "-C", str(source_repo), "worktree", "remove", "--force", str(dest_repo)],
        text=True,
        capture_output=True,
    )
    if proc.returncode != 0:
        shutil.rmtree(dest_repo, ignore_errors=True)


def _create_isolated_repo(source_repo: Path, dest_repo: Path) -> None:
    dest_repo.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["git", "-C", str(source_repo), "worktree", "add", "--detach", str(dest_repo), "HEAD"],
        text=True,
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(dest_repo), "reset", "--hard", "HEAD"],
        text=True,
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(dest_repo), "clean", "-fdx"],
        text=True,
        capture_output=True,
        check=True,
    )


def prepare_isolated_workspace(
    config: dict[str, Any],
    run_id: str,
    arm_name: str,
    instance: dict[str, Any],
) -> Path:
    source_repo, relative_dir = _source_repo_layout(instance)
    dest_repo = isolated_repo_dir(config, run_id, arm_name, instance_id(instance))
    _remove_existing_isolated_repo(source_repo, dest_repo)
    _create_isolated_repo(source_repo, dest_repo)
    isolated_dir = dest_repo / relative_dir
    if not isolated_dir.exists():
        raise HarnessError(
            f"isolation created repo `{dest_repo}` but expected working directory is missing: {isolated_dir}"
        )
    return isolated_dir
