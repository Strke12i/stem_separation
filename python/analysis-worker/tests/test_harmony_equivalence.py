"""Pins the vectorized harmony post-processing to the original per-frame loops.

The reference functions below are the pre-optimization implementations, kept
verbatim so the fast versions in `music_analyzer_worker.harmony` can be checked
against them on random, sparse (tie-prone) and silent input.
"""

from __future__ import annotations

from typing import Any

import numpy as np
import pytest

from music_analyzer_worker import harmony

NAMES = harmony.NAMES


def reference_chord_frames(chroma: np.ndarray[Any, Any]) -> tuple[list[str], list[float]]:
    templates: list[tuple[str, np.ndarray[Any, Any]]] = []
    for root, name in enumerate(NAMES):
        for quality, intervals in (("major", (0, 4, 7)), ("minor", (0, 3, 7))):
            template = np.zeros(12)
            template[[(root + i) % 12 for i in intervals]] = 1.0
            templates.append(
                (name if quality == "major" else f"{name}m", template / np.linalg.norm(template))
            )
    labels: list[str] = []
    scores: list[float] = []
    for frame in chroma.T:
        unit = frame / max(float(np.linalg.norm(frame)), 1e-9)
        score, label = max((float(np.dot(unit, template)), label) for label, template in templates)
        labels.append(label if score >= 0.45 else "N")
        scores.append(score)
    return labels, scores


def reference_nearest_beat(value: float, beats: list[float]) -> float | None:
    if not beats:
        return None
    beat = min(beats, key=lambda candidate: abs(candidate - value))
    return beat if abs(beat - value) <= 0.12 else None


def reference_segment(
    labels: list[str], scores: list[float], times: list[float], beats: list[float]
) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    start = 0
    for index in range(1, len(labels) + 1):
        if index == len(labels) or labels[index] != labels[start]:
            begin = times[start]
            end = (
                times[index]
                if index < len(times)
                else times[-1] + (times[-1] - times[-2] if len(times) > 1 else 0.1)
            )
            aligned = reference_nearest_beat(begin, beats)
            begin = aligned if aligned is not None else begin
            label = labels[start]
            root = None if label == "N" else label.rstrip("m")
            quality = None if label == "N" else ("minor" if label.endswith("m") else "major")
            out.append(
                {
                    "start": begin,
                    "end": max(end, begin + 0.001),
                    "label": label,
                    "root": root,
                    "quality": quality,
                    "score": float(np.mean(scores[start:index])),
                    "beat_aligned": aligned is not None,
                }
            )
            start = index
    return out


def tie_prone_chroma() -> np.ndarray[Any, Any]:
    """Single notes, dyads, triads and silence: many frames score several templates equally."""
    frames: list[np.ndarray[Any, Any]] = [np.zeros(12)]
    for note in range(12):
        single = np.zeros(12)
        single[note] = 1.0
        frames.append(single)
        dyad = np.zeros(12)
        dyad[[note, (note + 7) % 12]] = 1.0
        frames.append(dyad)
        triad = np.zeros(12)
        triad[[note, (note + 4) % 12, (note + 7) % 12]] = [1.0, 0.5, 0.8]
        frames.append(triad)
    return np.stack(frames, axis=1)


@pytest.mark.parametrize("seed", [0, 1, 2])
def test_chord_frames_matches_reference_on_random_chroma(seed: int) -> None:
    chroma = np.random.default_rng(seed).random((12, 1500))

    labels, scores = harmony.chord_frames(chroma)
    reference_labels, reference_scores = reference_chord_frames(chroma)

    assert labels == reference_labels
    assert scores == pytest.approx(reference_scores, abs=1e-12)


def test_chord_frames_matches_reference_on_tie_prone_chroma() -> None:
    chroma = tie_prone_chroma()

    labels, scores = harmony.chord_frames(chroma)
    reference_labels, reference_scores = reference_chord_frames(chroma)

    assert labels == reference_labels
    assert scores == pytest.approx(reference_scores, abs=1e-12)


def test_chord_frames_reports_silence_as_no_chord() -> None:
    labels, scores = harmony.chord_frames(np.zeros((12, 4)))

    assert labels == ["N"] * 4
    assert scores == [0.0] * 4


@pytest.mark.parametrize("seed", [0, 1])
def test_segment_matches_reference(seed: int) -> None:
    rng = np.random.default_rng(seed)
    frames = 800
    pool = ["N", "C", "Am", "F", "G", "Em"]
    labels = [pool[int(i)] for i in rng.integers(0, len(pool), size=frames // 8).repeat(8)]
    scores = rng.random(frames).tolist()
    times = (np.arange(frames) * 0.0464399).tolist()
    beats = np.arange(0.25, times[-1], 0.5).tolist()

    fast = harmony.segment(labels, scores, times, beats)
    slow = reference_segment(labels, scores, times, beats)
    assert len(fast) == len(slow)
    for got, want in zip(fast, slow, strict=True):
        assert got == pytest.approx(want, abs=1e-12)


def test_segment_matches_reference_without_beats() -> None:
    labels = ["C", "C", "G", "G", "N"]
    scores = [0.9, 0.8, 0.7, 0.6, 0.1]
    times = [0.0, 0.1, 0.2, 0.3, 0.4]

    assert harmony.segment(labels, scores, times, []) == reference_segment(
        labels, scores, times, []
    )


def test_segment_accepts_unsorted_beats() -> None:
    labels = ["C", "C", "G", "G", "Am", "Am"]
    scores = [0.9] * 6
    times = [0.0, 0.11, 0.5, 0.61, 1.02, 1.13]
    beats = [1.0, 0.0, 0.52, 0.3]

    fast = harmony.segment(labels, scores, times, beats)
    slow = reference_segment(labels, scores, times, beats)

    assert [chord["start"] for chord in fast] == [chord["start"] for chord in slow]
    assert [chord["beat_aligned"] for chord in fast] == [chord["beat_aligned"] for chord in slow]
