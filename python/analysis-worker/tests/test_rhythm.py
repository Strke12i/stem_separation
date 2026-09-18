from __future__ import annotations

from typing import Any

import numpy as np

from music_analyzer_worker.rhythm import analyze_signal, stability_score


def click_track(
    bpm: float, duration_seconds: float, sample_rate: int = 22_050
) -> np.ndarray[Any, np.dtype[np.float64]]:
    samples = np.zeros(int(duration_seconds * sample_rate), dtype=float)
    interval = 60.0 / bpm
    for time in np.arange(0.1, duration_seconds, interval):
        start = int(time * sample_rate)
        length = min(256, len(samples) - start)
        samples[start : start + length] = np.hanning(length)
    return samples


def test_detects_the_tempo_and_ordered_beats_of_a_synthetic_click_track() -> None:
    result = analyze_signal(click_track(120.0, 12.0), 22_050)

    assert 110.0 <= result["bpm"] <= 130.0
    assert len(result["beat_times"]) >= 16
    assert result["beat_times"] == sorted(result["beat_times"])
    assert 0.8 <= result["stability_score"] <= 1.0


def test_stability_is_zero_when_there_are_too_few_beats() -> None:
    assert stability_score([0.5, 1.0]) == 0.0
