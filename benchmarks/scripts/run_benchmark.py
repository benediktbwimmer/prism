#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
from pathlib import Path

from benchmark_common import dump_json
from benchmark_compare import run_codex_comparison
from benchmark_codex import run_codex_batch, run_codex_instance
from benchmark_config import load_normalized_config
from benchmark_harness import ingest_report, prepare_predictions, render_harness_commands, run_harness_command
from benchmark_runner import build_plan, finalize_run, materialize_run, record_instance


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="PRISM benchmark runner helper")
    subparsers = parser.add_subparsers(dest="command", required=True)

    plan_parser = subparsers.add_parser("plan", help="print or write a normalized run plan")
    plan_parser.add_argument("--config", required=True, type=Path)
    plan_parser.add_argument("--output", type=Path)

    materialize_parser = subparsers.add_parser("materialize", help="create plan, result, and telemetry artifacts")
    materialize_parser.add_argument("--config", required=True, type=Path)
    materialize_parser.add_argument("--force", action="store_true")

    prepare_predictions_parser = subparsers.add_parser(
        "prepare-predictions",
        help="create empty predictions templates for both arms",
    )
    prepare_predictions_parser.add_argument("--config", required=True, type=Path)
    prepare_predictions_parser.add_argument("--force", action="store_true")

    render_harness_parser = subparsers.add_parser(
        "render-harness",
        help="render external harness commands for one arm without executing them",
    )
    render_harness_parser.add_argument("--config", required=True, type=Path)
    render_harness_parser.add_argument("--arm", required=True, choices=["control", "prism"])

    run_harness_parser = subparsers.add_parser(
        "run-harness",
        help="execute one external harness command step for one arm",
    )
    run_harness_parser.add_argument("--config", required=True, type=Path)
    run_harness_parser.add_argument("--arm", required=True, choices=["control", "prism"])
    run_harness_parser.add_argument("--step", required=True, choices=["submit", "get_report"])
    run_harness_parser.add_argument("--dry-run", action="store_true")

    ingest_report_parser = subparsers.add_parser(
        "ingest-report",
        help="ingest a harness report into the local benchmark result artifact",
    )
    ingest_report_parser.add_argument("--config", required=True, type=Path)
    ingest_report_parser.add_argument("--arm", required=True, choices=["control", "prism"])
    ingest_report_parser.add_argument("--report", type=Path)

    run_codex_instance_parser = subparsers.add_parser(
        "run-codex-instance",
        help="run Codex for one manifest instance and write its patch into the predictions file",
    )
    run_codex_instance_parser.add_argument("--config", required=True, type=Path)
    run_codex_instance_parser.add_argument("--arm", required=True, choices=["control", "prism"])
    run_codex_instance_parser.add_argument("--instance", required=True)
    run_codex_instance_parser.add_argument("--force", action="store_true")

    run_codex_batch_parser = subparsers.add_parser(
        "run-codex-batch",
        help="run Codex for all or selected manifest instances and write patches into the predictions file",
    )
    run_codex_batch_parser.add_argument("--config", required=True, type=Path)
    run_codex_batch_parser.add_argument("--arm", required=True, choices=["control", "prism"])
    run_codex_batch_parser.add_argument("--instance", action="append", dest="instances")
    run_codex_batch_parser.add_argument("--force", action="store_true")
    run_codex_batch_parser.add_argument("--continue-on-error", action="store_true")

    run_comparison_parser = subparsers.add_parser(
        "run-comparison",
        help="materialize artifacts if needed, prepare predictions, then run both control and prism Codex arms",
    )
    run_comparison_parser.add_argument("--config", required=True, type=Path)
    run_comparison_parser.add_argument("--instance", action="append", dest="instances")
    run_comparison_parser.add_argument("--force", action="store_true")
    run_comparison_parser.add_argument("--continue-on-error", action="store_true")

    record_parser = subparsers.add_parser("record-instance", help="record one instance outcome and telemetry")
    record_parser.add_argument("--result", required=True, type=Path)
    record_parser.add_argument("--telemetry", required=True, type=Path)
    record_parser.add_argument("--arm", required=True, choices=["control", "prism"])
    record_parser.add_argument("--instance", required=True)
    record_parser.add_argument("--status", required=True, choices=["resolved", "unresolved", "error"])
    record_parser.add_argument("--prompt-tokens", type=int, default=0)
    record_parser.add_argument("--completion-tokens", type=int, default=0)
    record_parser.add_argument("--tool-calls", type=int, default=0)
    record_parser.add_argument("--prism-queries", type=int, default=0)
    record_parser.add_argument("--shell-commands", type=int, default=0)
    record_parser.add_argument("--shell-read-commands", type=int, default=0)
    record_parser.add_argument("--repeated-reads", type=int, default=0)
    record_parser.add_argument("--patch-attempts", type=int, default=0)
    record_parser.add_argument("--wall-time-seconds", type=float, default=0.0)

    finalize_parser = subparsers.add_parser("finalize", help="mark a run completed and refresh summaries")
    finalize_parser.add_argument("--result", required=True, type=Path)
    finalize_parser.add_argument("--telemetry", required=True, type=Path)

    return parser.parse_args()


def main() -> int:
    args = _parse_args()
    if args.command == "plan":
        plan = build_plan(args.config)
        if args.output:
            dump_json(args.output, plan)
        else:
            print(json.dumps(plan, indent=2))
        return 0

    if args.command == "materialize":
        created = materialize_run(args.config, force=args.force)
        print(json.dumps(created, indent=2))
        return 0

    if args.command == "prepare-predictions":
        config = load_normalized_config(args.config)
        created = prepare_predictions(config, force=args.force)
        print(json.dumps(created, indent=2))
        return 0

    if args.command == "render-harness":
        config = load_normalized_config(args.config)
        rendered = render_harness_commands(config, args.arm)
        print(json.dumps(rendered, indent=2))
        return 0

    if args.command == "run-harness":
        config = load_normalized_config(args.config)
        outcome = run_harness_command(config, args.arm, step=args.step, dry_run=args.dry_run)
        print(json.dumps(outcome, indent=2))
        return 0

    if args.command == "ingest-report":
        config = load_normalized_config(args.config)
        ingested = ingest_report(config, args.arm, report_path=args.report)
        print(json.dumps(ingested, indent=2))
        return 0

    if args.command == "run-codex-instance":
        config = load_normalized_config(args.config)
        outcome = run_codex_instance(config, args.arm, args.instance, force=args.force)
        print(json.dumps(outcome, indent=2))
        return 0

    if args.command == "run-codex-batch":
        config = load_normalized_config(args.config)
        outcome = run_codex_batch(
            config,
            args.arm,
            force=args.force,
            continue_on_error=args.continue_on_error,
            instance_names=args.instances,
        )
        print(json.dumps(outcome, indent=2))
        return 0

    if args.command == "run-comparison":
        outcome = run_codex_comparison(
            args.config,
            force=args.force,
            continue_on_error=args.continue_on_error,
            instance_names=args.instances,
        )
        print(json.dumps(outcome, indent=2))
        return 0

    if args.command == "record-instance":
        record_instance(
            result_path=args.result,
            telemetry_path=args.telemetry,
            arm_name=args.arm,
            instance_id=args.instance,
            status=args.status,
            prompt_tokens=args.prompt_tokens,
            completion_tokens=args.completion_tokens,
            tool_calls=args.tool_calls,
            prism_queries=args.prism_queries,
            shell_commands=args.shell_commands,
            shell_read_commands=args.shell_read_commands,
            repeated_reads=args.repeated_reads,
            patch_attempts=args.patch_attempts,
            wall_time_seconds=args.wall_time_seconds,
        )
        return 0

    finalize_run(args.result, args.telemetry)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
