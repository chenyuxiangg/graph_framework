#!/usr/bin/env python3
"""V5a e2e fixture：假 `codex` 可执行（PATH 注入；Codex adapter 真链路）。"""

from __future__ import annotations

import json
import os
import sys
import time

PID_FILE: str | None = os.environ.get("ZGRAPH_FAKE_CODEX_PID_FILE")

if PID_FILE:
    with open(PID_FILE, "w", encoding="utf-8") as pid_handle:
        pid_handle.write(str(os.getpid()))
if os.environ.get("ZGRAPH_FAKE_CODEX_MODE") == "sleep":
    time.sleep(60)
    sys.exit(0)

args = sys.argv[1:]
output = None
if "--output-last-message" in args:
    output = args[args.index("--output-last-message") + 1]
for event in [
    {"type": "thread.started", "thread_id": "t1"},
    {"type": "item.completed",
     "item": {"id": "i1", "type": "agent_message", "text": "codex-final"}},
    {"type": "turn.completed",
     "usage": {"input_tokens": 10, "output_tokens": 5,
               "reasoning_output_tokens": 2, "cached_input_tokens": 99}},
]:
    print(json.dumps(event), flush=True)
if output:
    with open(output, "w", encoding="utf-8") as handle:
        handle.write("codex-final")
