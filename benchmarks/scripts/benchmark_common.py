#!/usr/bin/env python3

from __future__ import annotations

import hashlib
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
BENCHMARKS = ROOT / "benchmarks"
TRACKS = BENCHMARKS / "tracks"


def load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def dump_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2, sort_keys=False)
        handle.write("\n")


def read_manifest(path: Path) -> list[str]:
    items: list[str] = []
    with path.open("r", encoding="utf-8") as handle:
        for raw_line in handle:
            line = raw_line.strip()
            if not line or line.startswith("#"):
                continue
            items.append(line)
    return items


def load_manifest_entries(path: Path) -> list[dict[str, Any]]:
    if path.suffix == ".jsonl":
        items: list[dict[str, Any]] = []
        with path.open("r", encoding="utf-8") as handle:
            for raw_line in handle:
                line = raw_line.strip()
                if not line or line.startswith("#"):
                    continue
                payload = json.loads(line)
                if not isinstance(payload, dict):
                    raise ValueError(f"manifest line is not an object: {line}")
                if "instance_id" not in payload:
                    raise ValueError(f"manifest object missing `instance_id`: {line}")
                items.append(payload)
        return items

    return [{"instance_id": instance_id} for instance_id in read_manifest(path)]


def instance_id(instance: dict[str, Any]) -> str:
    return str(instance["instance_id"])


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def utc_now_stamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def git_commit() -> str:
    proc = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        check=True,
        text=True,
        capture_output=True,
    )
    return proc.stdout.strip()


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        digest.update(handle.read())
    return digest.hexdigest()


def make_run_id(track: str, cohort: str) -> str:
    return f"{track}-{cohort}-{utc_now_stamp()}"
