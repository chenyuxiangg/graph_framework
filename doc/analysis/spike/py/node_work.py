# Python work units used by probe 2 (GIL / tokio execution model measurements).
import time


def sleep_node(seconds):
    """Blocks the calling thread but RELEASES the GIL during time.sleep."""
    time.sleep(seconds)
    return {"ok": True, "mode": "sleep", "seconds": seconds}


def busy_node(seconds):
    """CPU-bound busy loop: runs until THIS THREAD has consumed `seconds` of CPU
    time. Holding the GIL, so 4 of these need ~4x `seconds` of wall time when
    serialized by the GIL.

    Note: a wall-clock-bounded loop (`while time.perf_counter() < end`) is NOT a
    valid GIL probe: every thread exits at the same wall-clock moment even when
    fully serialized, which would falsely suggest parallelism.
    """
    start = time.thread_time()
    iterations = 0
    while time.thread_time() - start < seconds:
        iterations += 1
    return {"ok": True, "mode": "busy", "iterations": iterations, "cpu_seconds": seconds}
