# Gate functions used by probe 4. The design declares `gate(output, ctx) -> bool`.
def gate_pass(output, ctx=None):
    return output.get("score", 0) > 0.5


def gate_fail(output, ctx=None):
    return False


def gate_raise(output, ctx=None):
    raise ValueError("gate evaluation exploded")


def gate_truthy(output, ctx=None):
    # Truthy but not a `bool`: should this be accepted as a gate result?
    return "yes"


def gate_int(output, ctx=None):
    return 1


def gate_none(output, ctx=None):
    return None
