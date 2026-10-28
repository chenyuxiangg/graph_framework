# Happy-path node used by probe 1: consumes a dict, returns a nested dict that
# exercises every JSON-compatible Python type the design needs.
def run(inputs, ctx=None):
    return {
        "sum": inputs["a"] + inputs["b"],
        "echo": inputs,
        "none": None,
        "list": [1, 2.5, "x", None, True],
        "tuple": (1, "two", None),
        "nested": {"deep": {"arr": [[1], [2, None]]}},
        "pi": 3.14159,
        "big": 2**60,
        "unicode": "héllo 世界 🦀",
        "flag": inputs["flag"],
    }
