from __future__ import annotations

import numpy as np

from music_analyzer_worker.pitch import segment_pitch


def test_segments_synthetic_110_220_and_440_hz_notes() -> None:
    frequencies = np.repeat(np.array([110.0, 220.0, 440.0]), 10)
    times = np.arange(len(frequencies), dtype=float) * 0.01

    notes = segment_pitch(
        frequencies,
        np.ones(len(frequencies), dtype=bool),
        np.full(len(frequencies), 0.9),
        times,
        "bass",
    )

    assert [(note["midi"], note["note"]) for note in notes] == [(45, "A2"), (57, "A3"), (69, "A4")]
    assert all(note["confidence"] == 0.9 and note["stem"] == "bass" for note in notes)


def test_ignores_short_or_unvoiced_regions() -> None:
    notes = segment_pitch(
        np.array([220.0, 220.0, 220.0, np.nan, 220.0, 220.0]),
        np.array([True, True, True, False, True, True]),
        np.full(6, 0.8),
        np.arange(6, dtype=float) * 0.01,
        "vocals",
    )

    assert notes == []
