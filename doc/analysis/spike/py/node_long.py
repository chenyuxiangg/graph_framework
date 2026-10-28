# Long-running node used by the timeout / cancellation probes.
import time


def busy(seconds):
    end = time.perf_counter() + seconds
    iterations = 0
    while time.perf_counter() < end:
        iterations += 1
    return {"ok": True, "iterations": iterations}


def sleep(seconds):
    time.sleep(seconds)
    return {"ok": True, "mode": "sleep"}
