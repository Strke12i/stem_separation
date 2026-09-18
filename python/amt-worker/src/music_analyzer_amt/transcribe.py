"""Basic Pitch inference and MIDI creation, isolated from analysis-worker."""

from __future__ import annotations

import contextlib
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any


class TranscriptionError(Exception):
    def __init__(self, code: str, message: str, *, recoverable: bool = False) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.recoverable = recoverable


def transcribe(
    params: dict[str, Any], emit_progress: Callable[[str, float], None]
) -> dict[str, Any]:
    workspace = required_path(params, "workspace_path")
    source = required_path(params, "input_path")
    output = required_path(params, "output_dir")
    assert_within(source, workspace, "input_path")
    assert_within(output, workspace, "output_dir")
    if not source.is_file():
        raise TranscriptionError("MISSING_INPUT", "Normalized source is unavailable.")
    if output.exists():
        raise TranscriptionError(
            "INVALID_OUTPUT", "The AMT temporary output directory already exists."
        )
    try:
        from basic_pitch.inference import predict  # type: ignore[import-untyped]
    except ImportError as error:
        raise TranscriptionError(
            "AMT_RUNTIME_UNAVAILABLE",
            "Basic Pitch is not installed. Install the local AMT worker before transcribing.",
        ) from error

    try:
        output.mkdir(parents=True)
        emit_progress("loading_model", 0.05)
        # Basic Pitch logs prediction status to stdout; protocol output must remain NDJSON-only.
        with contextlib.redirect_stdout(sys.stderr):
            _, midi, events = predict(str(source))
        emit_progress("writing_midi", 0.85)
        midi_path = output / "transcription.mid"
        midi.write(str(midi_path))
        emit_progress("serializing_notes", 0.95)
        return {
            "engine": "basic-pitch",
            "model": "icassp_2022",
            "relative_midi_path": "transcription.mid",
            "notes": [event_note(event) for event in events],
        }
    except TranscriptionError:
        raise
    except MemoryError as error:
        raise TranscriptionError(
            "MODEL_OUT_OF_MEMORY",
            "Basic Pitch ran out of memory while transcribing.",
            recoverable=True,
        ) from error
    except Exception as error:
        raise TranscriptionError(
            "TRANSCRIPTION_FAILED", "Basic Pitch could not transcribe this audio."
        ) from error


def event_note(event: tuple[float, float, int, float, list[int] | None]) -> dict[str, Any]:
    start, end, midi, amplitude, _pitch_bend = event
    return {
        "start": float(start),
        "end": float(end),
        "midi": int(midi),
        "velocity": int(max(1, min(127, round(float(amplitude) * 127)))),
    }


def required_path(params: dict[str, Any], key: str) -> Path:
    value = params.get(key)
    if not isinstance(value, str) or not value:
        raise TranscriptionError("INVALID_REQUEST", f"Missing {key}.")
    return Path(value).resolve(strict=False)


def assert_within(candidate: Path, root: Path, label: str) -> None:
    try:
        candidate.relative_to(root.resolve())
    except ValueError as error:
        raise TranscriptionError(
            "INVALID_WORKSPACE", f"{label} is outside the track workspace."
        ) from error
