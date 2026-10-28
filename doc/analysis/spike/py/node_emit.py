# Node that calls back into the Rust `ctx` object (NodeContext.emit equivalent).
def run(inputs, ctx):
    count = int(inputs.get("count", 5))
    for i in range(count):
        ctx.emit({
            "type": "progress",
            "i": i,
            "payload": {"arr": [i, None, True], "label": "节点事件"},
        })
    # `stats` is a Rust #[getter] exposed to Python.
    return {"emitted": count, "stats_seen_by_python": ctx.stats}
