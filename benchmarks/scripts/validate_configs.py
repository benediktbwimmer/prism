#!/usr/bin/env python3

from __future__ import annotations

import sys

from benchmark_config import find_config_files, validate_config_file


def main() -> int:
    configs = find_config_files()
    if not configs:
        print("no benchmark configs found", file=sys.stderr)
        return 1

    errors: list[str] = []
    for config in configs:
        errors.extend(validate_config_file(config))

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    for config in configs:
        print(f"ok: {config}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
