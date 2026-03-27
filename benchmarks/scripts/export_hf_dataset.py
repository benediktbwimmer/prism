#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
from pathlib import Path

from datasets import load_dataset


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Export an official Hugging Face benchmark dataset split to local JSONL and explicit manifest files."
    )
    parser.add_argument("--dataset-name", required=True)
    parser.add_argument("--split", default="test")
    parser.add_argument("--output-jsonl", required=True, type=Path)
    parser.add_argument("--manifest-full", type=Path)
    parser.add_argument("--manifest-pilot", type=Path)
    parser.add_argument("--pilot-size", type=int, default=0)
    parser.add_argument("--repo", action="append", dest="repos")
    parser.add_argument("--repo-file", type=Path)
    parser.add_argument("--overwrite", action="store_true")
    return parser.parse_args()


def _normalize_repo_slug(value: str) -> str:
    return value.strip().lower()


def _load_repo_allowlist(args: argparse.Namespace) -> set[str]:
    repos = {_normalize_repo_slug(repo) for repo in (args.repos or []) if repo.strip()}
    if args.repo_file:
        with args.repo_file.open("r", encoding="utf-8") as handle:
            for raw_line in handle:
                line = raw_line.strip()
                if not line or line.startswith("#"):
                    continue
                repos.add(_normalize_repo_slug(line))
    return repos


def _ensure_writable(path: Path, overwrite: bool) -> None:
    if path.exists() and not overwrite:
        raise SystemExit(f"refusing to overwrite existing file without --overwrite: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)


def _write_jsonl(path: Path, rows: list[dict]) -> None:
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, sort_keys=False))
            handle.write("\n")


def _write_manifest(path: Path, rows: list[dict]) -> None:
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(str(row["instance_id"]))
            handle.write("\n")


def main() -> int:
    args = _parse_args()
    repo_allowlist = _load_repo_allowlist(args)

    if args.output_jsonl:
        _ensure_writable(args.output_jsonl, args.overwrite)
    if args.manifest_full:
        _ensure_writable(args.manifest_full, args.overwrite)
    if args.manifest_pilot:
        _ensure_writable(args.manifest_pilot, args.overwrite)

    dataset = load_dataset(args.dataset_name, split=args.split)
    rows = [dict(row) for row in dataset]
    if repo_allowlist:
        rows = [
            row
            for row in rows
            if isinstance(row.get("repo"), str)
            and _normalize_repo_slug(row["repo"]) in repo_allowlist
        ]

    _write_jsonl(args.output_jsonl, rows)
    if args.manifest_full:
        _write_manifest(args.manifest_full, rows)
    if args.manifest_pilot and args.pilot_size > 0:
        _write_manifest(args.manifest_pilot, rows[: args.pilot_size])

    unique_repos = sorted({row.get("repo", "") for row in rows if row.get("repo")})
    print(
        json.dumps(
            {
                "dataset_name": args.dataset_name,
                "split": args.split,
                "output_jsonl": str(args.output_jsonl.resolve()),
                "row_count": len(rows),
                "repo_count": len(unique_repos),
                "manifest_full": str(args.manifest_full.resolve()) if args.manifest_full else None,
                "manifest_pilot": str(args.manifest_pilot.resolve()) if args.manifest_pilot else None,
                "pilot_size": min(args.pilot_size, len(rows)),
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
