"""Monophonic pYIN pitch analysis for locally generated bass and vocal stems."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path
from typing import Any

import librosa
import numpy as np

ENGINE = "pyin"
SUPPORTED_STEMS = {"bass", "vocals"}
MIN_NOTE_SECONDS = 0.06


class PitchError(Exception):
    """Controlled pitch failure that is safe to expose over IPC."""

    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code
        self.message = message


def analyze(params: dict[str, Any], emit_progress: Callable[[str, float], None]) -> dict[str, Any]:
    workspace = required_path(params, "workspace_path")
    source = required_path(params, "input_path")
    assert_within(source, workspace)
    stem = params.get("stem")
    if not isinstance(stem, str) or stem not in SUPPORTED_STEMS:
        raise PitchError("UNSUPPORTED_STEM", "Pitch analysis currently supports bass and vocals.")
    if not source.is_file():
        raise PitchError("MISSING_INPUT", "The requested stem audio is unavailable.")

    try:
        emit_progress("loading_stem", 0.05)
        samples, sample_rate = librosa.load(source, sr=None, mono=True)
        if samples.size == 0 or sample_rate <= 0:
            raise PitchError("INVALID_AUDIO", "The stem contains no decodable audio.")
        emit_progress("estimating_pitch", 0.25)
        fmin, fmax = frequency_bounds(stem)
        f0, voiced, probabilities = librosa.pyin(
            samples,
            fmin=fmin,
            fmax=fmax,
            sr=int(sample_rate),
        )
        times = librosa.frames_to_time(np.arange(len(f0)), sr=int(sample_rate)).astype(float)
        emit_progress("segmenting_notes", 0.8)
        notes = segment_pitch(f0, voiced, probabilities, times, stem)
        emit_progress("validating_notes", 0.95)
        return {"stem": stem, "engine": ENGINE, "notes": notes}
    except PitchError:
        raise
    except Exception as error:  # librosa exposes no stable analysis exception hierarchy
        raise PitchError(
            "PITCH_FAILED", "Could not estimate monophonic pitch for this stem."
        ) from error


def frequency_bounds(stem: str) -> tuple[float, float]:
    if stem == "bass":
        return float(librosa.note_to_hz("E1")), float(librosa.note_to_hz("C5"))
    return float(librosa.note_to_hz("C2")), float(librosa.note_to_hz("C6"))


def segment_pitch(
    f0: np.ndarray[Any, Any],
    voiced: np.ndarray[Any, Any],
    probabilities: np.ndarray[Any, Any],
    times: np.ndarray[Any, Any],
    stem: str,
) -> list[dict[str, Any]]:
    """Turn voiced pYIN frames into deterministic, MIDI-rounded note spans."""
    if not (len(f0) == len(voiced) == len(probabilities) == len(times)):
        raise PitchError("INVALID_PITCH", "Pitch frame arrays have inconsistent lengths.")
    if len(times) == 0:
        return []

    hop_seconds = float(np.median(np.diff(times))) if len(times) > 1 else 0.01
    hop_seconds = max(hop_seconds, 0.001)
    notes: list[dict[str, Any]] = []
    start: int | None = None
    midi: int | None = None

    def finish(end: int) -> None:
        nonlocal start, midi
        if start is None or midi is None:
            return
        begin_seconds = float(times[start])
        end_seconds = float(times[end - 1]) + hop_seconds
        if end_seconds - begin_seconds >= MIN_NOTE_SECONDS:
            confidence = float(np.nanmean(probabilities[start:end]))
            if not np.isfinite(confidence):
                confidence = 0.0
            notes.append(
                {
                    "start": begin_seconds,
                    "end": end_seconds,
                    "midi": midi,
                    "note": librosa.midi_to_note(midi, octave=True),
                    "confidence": float(np.clip(confidence, 0.0, 1.0)),
                    "stem": stem,
                    "engine": ENGINE,
                }
            )
        start = None
        midi = None

    for index, value in enumerate(f0):
        current = (
            int(round(float(librosa.hz_to_midi(value))))
            if voiced[index] and np.isfinite(value)
            else None
        )
        if current is None:
            finish(index)
        elif midi == current:
            continue
        else:
            finish(index)
            start = index
            midi = current
    finish(len(f0))
    return notes


def required_path(params: dict[str, Any], key: str) -> Path:
    value = params.get(key)
    if not isinstance(value, str) or not value:
        raise PitchError("INVALID_REQUEST", f"Missing {key}.")
    return Path(value).resolve(strict=False)


def assert_within(candidate: Path, root: Path) -> None:
    try:
        candidate.relative_to(root.resolve())
    except ValueError as error:
        raise PitchError("INVALID_WORKSPACE", "Input is outside the track workspace.") from error
