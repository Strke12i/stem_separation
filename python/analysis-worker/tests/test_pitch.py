from __future__ import annotations

import librosa
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


def test_notes_that_run_into_each_other_never_overlap() -> None:
    # Real frame times (512-sample hops at 22.05 kHz) are not exact multiples,
    # so `last frame time + hop` can differ from the next frame time by ~1e-15 s.
    frames = 6000
    times = librosa.frames_to_time(np.arange(frames), sr=22050).astype(float)
    frequencies = np.where((np.arange(frames) // 7) % 2 == 0, 110.0, 130.81)

    notes = segment_pitch(
        frequencies,
        np.ones(frames, dtype=bool),
        np.full(frames, 0.9),
        times,
        "bass",
    )

    assert len(notes) > 800
    # Every frame is voiced, so each note hands over to the next with no gap at all.
    assert all(
        later["start"] == earlier["end"] for earlier, later in zip(notes, notes[1:], strict=False)
    )
