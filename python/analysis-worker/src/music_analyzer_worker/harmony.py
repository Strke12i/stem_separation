"""Local key and chord estimation with a deliberately small initial vocabulary."""

from __future__ import annotations

from bisect import bisect_left
from collections import Counter
from collections.abc import Callable
from pathlib import Path
from typing import Any

import librosa
import numpy as np

NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
MAJOR_PROFILE = np.array([6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88])
MINOR_PROFILE = np.array([6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17])


class HarmonyError(Exception):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code
        self.message = message


def analyze(params: dict[str, Any], emit_progress: Callable[[str, float], None]) -> dict[str, Any]:
    workspace = required_path(params, "workspace_path")
    source = required_path(params, "input_path")
    assert_within(source, workspace)
    if not source.is_file():
        raise HarmonyError("MISSING_INPUT", "Normalized source is unavailable.")
    beats = params.get("beat_times", [])
    if not isinstance(beats, list) or any(not isinstance(value, (int, float)) for value in beats):
        raise HarmonyError("INVALID_REQUEST", "Beat times must be numeric.")
    try:
        emit_progress("loading_audio", 0.05)
        samples, rate = librosa.load(source, sr=None, mono=True)
        emit_progress("extracting_chroma", 0.3)
        chroma = librosa.feature.chroma_cqt(y=samples, sr=int(rate))
        times = librosa.frames_to_time(np.arange(chroma.shape[1]), sr=int(rate))
        emit_progress("estimating_key_and_chords", 0.65)
        return infer_harmony(
            chroma, times.astype(float).tolist(), [float(value) for value in beats]
        )
    except HarmonyError:
        raise
    except Exception as error:
        raise HarmonyError(
            "HARMONY_FAILED", "Could not estimate key and chords from this audio."
        ) from error


def infer_harmony(
    chroma: np.ndarray[Any, Any], times: list[float], beats: list[float]
) -> dict[str, Any]:
    if chroma.shape[0] != 12 or chroma.shape[1] == 0:
        raise HarmonyError("INVALID_HARMONY", "Chroma features are unavailable.")
    aggregate = np.mean(chroma, axis=1)
    key = infer_key(aggregate)
    labels, scores = chord_frames(chroma)
    labels = smooth(labels, 3)
    chords = segment(labels, scores, times, beats)
    return {"key": key, "chords": chords, "algorithm": "librosa.chroma_cqt+templates"}


def infer_key(chroma: np.ndarray[Any, Any]) -> dict[str, Any]:
    values: list[tuple[float, int, str]] = []
    normalized = chroma / max(float(np.linalg.norm(chroma)), 1e-9)
    for mode, profile in (("major", MAJOR_PROFILE), ("minor", MINOR_PROFILE)):
        for tonic in range(12):
            candidate = np.roll(profile, tonic)
            candidate = candidate / np.linalg.norm(candidate)
            values.append((float(np.dot(normalized, candidate)), tonic, mode))
    values.sort(reverse=True)
    best, second = values[:2]
    return {
        "tonic": NAMES[best[1]],
        "mode": best[2],
        "label": f"{NAMES[best[1]]} {best[2]}",
        "score": best[0],
        "second_best": f"{NAMES[second[1]]} {second[2]}",
        "margin": best[0] - second[0],
    }


def _build_chord_templates() -> tuple[list[str], np.ndarray[Any, Any], np.ndarray[Any, Any]]:
    """Unit-norm major/minor triad templates, plus each label's string-order rank."""
    labels: list[str] = []
    rows: list[np.ndarray[Any, Any]] = []
    for root, name in enumerate(NAMES):
        for quality, intervals in (("major", (0, 4, 7)), ("minor", (0, 3, 7))):
            template = np.zeros(12)
            template[[(root + i) % 12 for i in intervals]] = 1.0
            labels.append(name if quality == "major" else f"{name}m")
            rows.append(template / np.linalg.norm(template))
    order = sorted(range(len(labels)), key=labels.__getitem__)
    rank = np.empty(len(labels), dtype=np.int64)
    rank[order] = np.arange(len(labels))
    return labels, np.stack(rows), rank


CHORD_LABELS, CHORD_TEMPLATES, _CHORD_RANK = _build_chord_templates()
# Templates whose cosine is this close to the best one count as tied. Frames with a
# single sounding note score several triads identically, and the tie is broken by
# label order; a tolerance keeps that decision stable across BLAS builds.
_TIE_TOLERANCE = 1e-9
_CHORD_FLOOR = 0.45
_CHORD_CHUNK_FRAMES = 8192


def chord_frames(chroma: np.ndarray[Any, Any]) -> tuple[list[str], list[float]]:
    frames = np.asarray(chroma, dtype=np.float64)
    labels: list[str] = []
    scores: list[float] = []
    # Chunked so the (templates x frames) score matrix stays small on long audio.
    for begin in range(0, frames.shape[1], _CHORD_CHUNK_FRAMES):
        block = frames[:, begin : begin + _CHORD_CHUNK_FRAMES]
        units = block / np.maximum(np.linalg.norm(block, axis=0), 1e-9)
        similarity = CHORD_TEMPLATES @ units
        best = similarity.max(axis=0)
        tied = similarity >= best - _TIE_TOLERANCE
        winners = np.argmax(np.where(tied, _CHORD_RANK[:, None], -1), axis=0)
        labels.extend(
            CHORD_LABELS[winner] if score >= _CHORD_FLOOR else "N"
            for winner, score in zip(winners.tolist(), best.tolist(), strict=True)
        )
        scores.extend(best.tolist())
    return labels, scores


def smooth(labels: list[str], width: int) -> list[str]:
    smoothed: list[str] = []
    for i in range(len(labels)):
        window = labels[max(0, i - width) : min(len(labels), i + width + 1)]
        counts = Counter(window)
        # Deterministic tiebreak: highest count wins; ties break by earliest
        # position in the window. `set`/dict iteration order for str keys
        # depends on PYTHONHASHSEED, which CPython randomizes per process, so
        # picking a tied winner from set order let identical audio produce
        # different chord boundaries across runs and silently broke the
        # Rust-side cache-key assumption that the same input always yields
        # the same persisted result.
        best_label = min(counts, key=lambda label: (-counts[label], window.index(label)))
        smoothed.append(best_label)
    return smoothed


def segment(
    labels: list[str], scores: list[float], times: list[float], beats: list[float]
) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    score_values = np.asarray(scores, dtype=np.float64)
    ordered_beats = sorted(beats)
    start = 0
    for index in range(1, len(labels) + 1):
        if index == len(labels) or labels[index] != labels[start]:
            begin = times[start]
            end = (
                times[index]
                if index < len(times)
                else times[-1] + (times[-1] - times[-2] if len(times) > 1 else 0.1)
            )
            aligned = nearest_beat(begin, ordered_beats)
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
                    "score": float(np.mean(score_values[start:index])),
                    "beat_aligned": aligned is not None,
                }
            )
            start = index
    return out


def nearest_beat(value: float, beats: list[float]) -> float | None:
    """Closest beat within 120 ms of `value`; `beats` must be sorted ascending."""
    if not beats:
        return None
    position = bisect_left(beats, value)
    # Left neighbour first so an exact tie resolves to the earlier beat.
    beat = min(beats[max(0, position - 1) : position + 1], key=lambda c: abs(c - value))
    return beat if abs(beat - value) <= 0.12 else None


def required_path(params: dict[str, Any], key: str) -> Path:
    value = params.get(key)
    if not isinstance(value, str) or not value:
        raise HarmonyError("INVALID_REQUEST", f"Missing {key}.")
    return Path(value).resolve(strict=False)


def assert_within(candidate: Path, root: Path) -> None:
    try:
        candidate.relative_to(root.resolve())
    except ValueError as error:
        raise HarmonyError("INVALID_WORKSPACE", "Input is outside the track workspace.") from error
