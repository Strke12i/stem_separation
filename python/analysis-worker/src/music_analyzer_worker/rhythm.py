"""Local rhythm estimation over the Rust-normalized WAV artifact."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path
from typing import Any

import librosa
import numpy as np


class RhythmError(Exception):
    """Controlled rhythm failure that is safe to expose over IPC."""

    def __init__(self, code: str, message: str, *, recoverable: bool = False) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.recoverable = recoverable


def analyze(params: dict[str, Any], emit_progress: Callable[[str, float], None]) -> dict[str, Any]:
    workspace = required_path(params, "workspace_path")
    source = required_path(params, "input_path")
    assert_within(source, workspace, "input_path")
    if not source.is_file():
        raise RhythmError("MISSING_INPUT", "Normalized source is unavailable.")

    try:
        emit_progress("loading_audio", 0.05)
        samples, sample_rate = librosa.load(source, sr=None, mono=True)
        if samples.size == 0 or sample_rate <= 0:
            raise RhythmError("INVALID_AUDIO", "Normalized source contains no decodable samples.")
        emit_progress("estimating_onsets", 0.25)
        result = analyze_signal(samples, int(sample_rate))
        emit_progress("validating_beats", 0.9)
        return result
    except RhythmError:
        raise
    except Exception as error:  # librosa does not provide a stable public error hierarchy
        raise RhythmError(
            "RHYTHM_FAILED", "Could not estimate BPM and beats from this audio."
        ) from error


def analyze_signal(samples: np.ndarray[Any, Any], sample_rate: int) -> dict[str, Any]:
    onset_envelope = librosa.onset.onset_strength(y=samples, sr=sample_rate)
    tempo, beat_frames = librosa.beat.beat_track(onset_envelope=onset_envelope, sr=sample_rate)
    bpm = float(np.asarray(tempo).reshape(-1)[0])
    beat_times = librosa.frames_to_time(beat_frames, sr=sample_rate).astype(float).tolist()
    if not np.isfinite(bpm) or bpm <= 0.0 or not all(np.isfinite(beat_times)):
        raise RhythmError("INVALID_RHYTHM", "Rhythm estimation produced invalid numeric values.")
    if any(later <= earlier for earlier, later in zip(beat_times, beat_times[1:], strict=False)):
        raise RhythmError("INVALID_RHYTHM", "Rhythm estimation produced unordered beats.")
    return {
        "bpm": bpm,
        "beat_times": beat_times,
        "algorithm": "librosa.beat",
        "stability_score": stability_score(beat_times),
    }


def stability_score(beat_times: list[float]) -> float:
    if len(beat_times) < 3:
        return 0.0
    intervals = np.diff(np.asarray(beat_times, dtype=float))
    mean = float(np.mean(intervals))
    if mean <= 0.0:
        return 0.0
    coefficient_of_variation = float(np.std(intervals) / mean)
    return float(np.clip(1.0 - coefficient_of_variation, 0.0, 1.0))


def required_path(params: dict[str, Any], key: str) -> Path:
    value = params.get(key)
    if not isinstance(value, str) or not value:
        raise RhythmError("INVALID_REQUEST", f"Missing {key}.")
    return Path(value).resolve(strict=False)


def assert_within(candidate: Path, root: Path, label: str) -> None:
    try:
        candidate.relative_to(root.resolve())
    except ValueError as error:
        raise RhythmError(
            "INVALID_WORKSPACE", f"{label} is outside the track workspace."
        ) from error
