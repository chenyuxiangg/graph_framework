"""DSH ACP adapter 骨架（`python/zgraph_worker/adapters/dsh_acp.py`）单元测试。"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
ADAPTER = REPO_ROOT / "python" / "zgraph_worker" / "adapters" / "dsh_acp.py"


def _run(frames: list[dict[str, object]]) -> list[dict[str, object]]:
    payload = "".join(json.dumps(frame) + "\n" for frame in frames)
    result = subprocess.run(
        [sys.executable, str(ADAPTER)],
        input=payload,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    return [json.loads(line) for line in result.stdout.splitlines() if line.strip()]


def test_skeleton_handshakes_with_committed_message_granularity() -> None:
    out = _run([{"v": 1, "type": "hello", "protocol": "zgraph-harness/1"}])
    assert out == [
        {
            "v": 1,
            "type": "hello_ok",
            "protocol": "zgraph-harness/1",
            "provider": "acp",
            "capabilities": {"streaming": True, "reasoning": False},
        }
    ]


def test_skeleton_returns_structured_protocol_error_for_run() -> None:
    out = _run(
        [
            {"v": 1, "type": "hello", "protocol": "zgraph-harness/1"},
            {"v": 1, "type": "run", "inputs": {}},
        ]
    )
    assert out[1]["type"] == "error"
    assert out[1]["code"] == "HARNESS_PLUGIN_ERROR"
    assert out[1]["reason"] == "protocol"
    assert "skeleton" in out[1]["message"]


def test_skeleton_rejects_wrong_protocol() -> None:
    assert _run([{"v": 1, "type": "hello", "protocol": "zgraph-harness/2"}]) == []
