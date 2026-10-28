# Node that raises a custom exception, used by probe 5.
class NodeError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def run(inputs, ctx=None):
    raise NodeError("E_NODE_BOOM", "intentional node failure from Python")


def run_ok(inputs, ctx=None):
    return {"ok": True, "after": "error", "inputs": inputs}
