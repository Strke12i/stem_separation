"""Compare the Rust `smooth_labels` with `music_analyzer_worker.harmony.smooth`.

Run inside the analysis-worker environment with the lab wheel added:

    uv run --project python/analysis-worker --with <wheel> python labs/analyzer-native/bench.py
"""

from __future__ import annotations

import random
import statistics
import time
from collections.abc import Callable

import analyzer_native
from music_analyzer_worker.harmony import smooth

POOL = ["N", "C", "Am", "F", "G", "Em", "Dm", "G#m", "A#"]


def synthetic_labels(frames: int, seed: int) -> list[str]:
    """Chord runs of 5-40 frames with ~15% single-frame noise, like real chroma output."""
    rng = random.Random(seed)
    labels: list[str] = []
    while len(labels) < frames:
        labels.extend([rng.choice(POOL)] * rng.randint(5, 40))
    labels = labels[:frames]
    for index in range(frames):
        if rng.random() < 0.15:
            labels[index] = rng.choice(POOL)
    return labels


def median_ms(work: Callable[[], object], runs: int) -> float:
    values = []
    for _ in range(runs):
        started = time.perf_counter()
        work()
        values.append((time.perf_counter() - started) * 1000)
    return statistics.median(values)


def main() -> None:
    for seed in range(5):
        for width in (1, 3, 6):
            labels = synthetic_labels(3000, seed)
            assert analyzer_native.smooth_labels(labels, width) == smooth(labels, width), (
                seed,
                width,
            )
    print("equivalence: identical to harmony.smooth for 15 (seed, width) cases")

    for frames, runs in ((100, 200), (22_161, 15)):
        labels = synthetic_labels(frames, 7)
        python_ms = median_ms(lambda labels=labels: smooth(labels, 3), runs)
        rust_ms = median_ms(lambda labels=labels: analyzer_native.smooth_labels(labels, 3), runs)
        print(
            f"frames={frames:6d}  python={python_ms:8.3f} ms  rust={rust_ms:8.3f} ms  "
            f"speedup={python_ms / rust_ms:6.1f}x"
        )


if __name__ == "__main__":
    main()
