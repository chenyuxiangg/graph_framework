"""W4b e2e fixture worker：foreach 动态 N 真链路（确定性、无外部副作用）。"""

from __future__ import annotations

import time
from typing import Any

from zgraph_worker import WorkerContext

SLOW_SECONDS: float = 1.0


def _item(package: Any) -> Any:
    """从整包广播的端口值中取出 item（F/W 输出均为 dict）。"""
    if isinstance(package, dict) and "item" in package:
        return package["item"]
    return package


def fanout(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """F per-item handler：`items` 端口收到 item[i]，原样带出。"""
    return {"item": inputs.get("items")}


def work(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """W：item["fail"] 抛错、item["slow"] 睡眠、item["skip"] go=false。"""
    item: Any = _item(inputs.get("task"))
    if isinstance(item, dict):
        if item.get("fail"):
            raise RuntimeError(f"item boom: node={ctx.node_id}")
        if item.get("slow"):
            time.sleep(SLOW_SECONDS)
        if isinstance(item.get("delay_ms"), (int, float)):
            time.sleep(float(item["delay_ms"]) / 1000.0)
        if item.get("skip"):
            return {"go": False, "item": item}
    return {"go": True, "item": item}


def exit_node(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """E（region 出口）：输出 join 数组的元素。"""
    item: Any = _item(inputs.get("work"))
    return {"item": item, "ok": True}


def join(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """J：把有序数组包成 {"count","results"}。"""
    results: Any = inputs.get("results")
    count: int = len(results) if isinstance(results, list) else -1
    return {"count": count, "results": results}


def list_source(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """上游 python 节点：把 entry seed 列表包装成 {"items": [...]}。"""
    seed: Any = inputs.get("seed")
    items: Any = seed.get("seed") if isinstance(seed, dict) else seed
    return {"items": list(items or [])}
