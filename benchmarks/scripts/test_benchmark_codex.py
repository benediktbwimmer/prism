#!/usr/bin/env python3

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from benchmark_codex import (
    _compose_prompt,
    _parse_exec_jsonl,
    _parse_workspace_refresh_metrics,
)


class BenchmarkCodexTests(unittest.TestCase):
    def test_compose_prompt_includes_adaptive_preview_guidance(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            prompt_path = root / "prism.md"
            prompt_path.write_text("PRISM arm prompt", encoding="utf-8")
            workspace_dir = root / "workspace"
            workspace_dir.mkdir()
            config = {
                "arms": [
                    {
                        "name": "prism",
                        "prism_enabled": True,
                        "prompt_path": str(prompt_path),
                        "prompt_abspath": str(prompt_path),
                        "tool_profile": "codex-prism",
                        "compact_preview_policy": "adaptive",
                    }
                ]
            }
            instance = {
                "instance_id": "demo__repo-1",
                "repo": "demo/repo",
                "base_commit": "deadbeef",
                "problem_statement": "Fix the thing.",
                "prompt": "Solve the benchmark task.",
            }

            prompt = _compose_prompt(config, "prism", instance, workspace_dir)

            self.assertIn("Adaptive preview policy", prompt)
            self.assertIn("includeTopPreview: true", prompt)

    def test_parse_exec_jsonl_tracks_preview_requests_hits_and_followups(self) -> None:
        events = [
            {
                "type": "item.completed",
                "item": {
                    "type": "mcp_tool_call",
                    "server": "prism",
                    "tool": "prism_locate",
                    "arguments": {"query": "compact_open", "includeTopPreview": True},
                    "result": {
                        "structured_content": {
                            "candidates": [{"handle": "handle:1"}],
                            "topPreview": {"handle": "handle:1", "text": "pub fn compact_open()"},
                        }
                    },
                },
            },
            {
                "type": "item.completed",
                "item": {
                    "type": "mcp_tool_call",
                    "server": "prism",
                    "tool": "prism_workset",
                    "arguments": {"handle": "handle:1"},
                    "result": {"structured_content": {"handle": "handle:1"}},
                },
            },
            {
                "type": "item.completed",
                "item": {
                    "type": "mcp_tool_call",
                    "server": "prism",
                    "tool": "prism_expand",
                    "arguments": {"handle": "handle:2", "kind": "neighbors", "includeTopPreview": True},
                    "result": {
                        "structured_content": {
                            "neighbors": [{"handle": "handle:9"}],
                            "topPreview": {"handle": "handle:9", "text": "neighbor preview"},
                        }
                    },
                },
            },
            {
                "type": "item.completed",
                "item": {
                    "type": "mcp_tool_call",
                    "server": "prism",
                    "tool": "prism_open",
                    "arguments": {"handle": "handle:9", "mode": "focus"},
                    "result": {"structured_content": {"handle": "handle:9"}},
                },
            },
            {
                "type": "turn.completed",
                "usage": {"input_tokens": 11, "output_tokens": 7},
            },
        ]

        parsed = _parse_exec_jsonl("\n".join(json.dumps(event) for event in events))

        self.assertEqual(parsed["prism_compact_tool_calls"], 4)
        self.assertEqual(parsed["locate_preview_requests"], 1)
        self.assertEqual(parsed["locate_preview_hits"], 1)
        self.assertGreater(parsed["locate_preview_bytes"], 0)
        self.assertEqual(parsed["locate_preview_direct_progressions"], 1)
        self.assertEqual(parsed["locate_preview_direct_opens"], 0)
        self.assertEqual(parsed["expand_preview_requests"], 1)
        self.assertEqual(parsed["expand_preview_hits"], 1)
        self.assertGreater(parsed["expand_preview_bytes"], 0)
        self.assertEqual(parsed["expand_preview_direct_opens"], 1)
        self.assertEqual(parsed["expand_preview_direct_progressions"], 0)
        self.assertEqual(parsed["prompt_tokens"], 11)
        self.assertEqual(parsed["completion_tokens"], 7)

    def test_parse_workspace_refresh_metrics_sums_internal_breakdown(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            workspace_dir = Path(temp_dir)
            prism_dir = workspace_dir / ".prism"
            prism_dir.mkdir()
            log_path = prism_dir / "prism-mcp-daemon.log"
            log_path.write_text(
                "\n".join(
                    [
                        json.dumps(
                            {
                                "message": "prism-mcp workspace refresh",
                                "refresh_path": "auxiliary",
                                "duration_ms": 15,
                                "lock_wait_ms": 1,
                                "lock_hold_ms": 15,
                                "snapshot_revisions_ms": 3,
                                "load_episodic_ms": 4,
                                "load_inference_ms": 0,
                                "load_coordination_ms": 2,
                            }
                        ),
                        json.dumps(
                            {
                                "message": "prism-mcp workspace refresh",
                                "refresh_path": "full",
                                "duration_ms": 40,
                                "lock_wait_ms": 2,
                                "lock_hold_ms": 40,
                                "snapshot_revisions_ms": 5,
                                "load_episodic_ms": 6,
                                "load_inference_ms": 7,
                                "load_coordination_ms": 8,
                            }
                        ),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            parsed = _parse_workspace_refresh_metrics(workspace_dir, 0)

            self.assertEqual(parsed["workspace_refresh_duration_ms"], 55)
            self.assertEqual(parsed["workspace_refresh_lock_wait_ms"], 3)
            self.assertEqual(parsed["workspace_refresh_lock_hold_ms"], 55)
            self.assertEqual(parsed["workspace_refresh_snapshot_revisions_ms"], 8)
            self.assertEqual(parsed["workspace_refresh_load_episodic_ms"], 10)
            self.assertEqual(parsed["workspace_refresh_load_inference_ms"], 7)
            self.assertEqual(parsed["workspace_refresh_load_coordination_ms"], 10)
            self.assertEqual(parsed["workspace_full_reload_count"], 1)
            self.assertEqual(parsed["workspace_auxiliary_reload_count"], 1)


if __name__ == "__main__":
    unittest.main()
