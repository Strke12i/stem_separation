"""Offline-only adapter around audio-separator for Rust-owned separation jobs."""

from __future__ import annotations

import json
import os
import shutil
import socket
from collections.abc import Callable, Iterator
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Any, cast


class SeparationError(Exception):
    """A controlled error returned through the worker protocol."""

    def __init__(self, code: str, message: str, *, recoverable: bool = False) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.recoverable = recoverable


@dataclass(frozen=True)
class SeparationProfile:
    model_id: str
    filename: str
    stems: tuple[str, ...]


PROFILES = {
    "demucs-4": SeparationProfile(
        model_id="demucs-4", filename="htdemucs.yaml", stems=("vocals", "drums", "bass", "other")
    ),
    "demucs-6-experimental": SeparationProfile(
        model_id="demucs-6-experimental",
        filename="htdemucs_6s.yaml",
        stems=("vocals", "drums", "bass", "other", "guitar", "piano"),
    ),
}


def separate(
    params: dict[str, Any], emit_progress: Callable[[str, float], None], job_id: str
) -> dict[str, Any]:
    """Separate strictly inside a Rust-created temporary workspace."""
    profile = profile_from(params)
    workspace = required_path(params, "workspace_path")
    source = required_path(params, "input_path")
    output = required_path(params, "output_dir")
    model_dir = required_path(params, "model_dir")
    assert_within(source, workspace, "input_path")
    assert_within(output, workspace, "output_dir")
    if output.parent.name != "tmp" or output.name != job_id:
        raise SeparationError("INVALID_WORKSPACE", "Output directory does not match this job.")
    if not source.is_file():
        raise SeparationError("MISSING_INPUT", "Normalized source is unavailable.")
    if not model_dir.is_dir() or not (model_dir / ".lma-model.json").is_file():
        raise SeparationError(
            "MODEL_NOT_INSTALLED", f"Model '{profile.model_id}' is not installed.", recoverable=True
        )
    verify_model_marker(model_dir, profile)

    # ``audio_separator`` imports requests and ssl.  The offline guard below
    # intentionally replaces ``socket.socket`` during model loading/inference,
    # so that import must complete before the guard is active. Importing a Python
    # module does not contact the network; model work remains inside the guard.
    separator_type = load_separator_type()
    try:
        output.mkdir(parents=True, exist_ok=False)
    except OSError as error:
        raise SeparationError(
            "INVALID_WORKSPACE", "Could not create the job output directory."
        ) from error
    emit_progress("loading_model", 0.05)
    if os.environ.get("LOCAL_MUSIC_ANALYZER_TEST_SEPARATOR") == "copy":
        return copy_for_test(source, output, profile, emit_progress)

    try:
        with offline_network():
            separator = separator_type(
                model_file_dir=str(model_dir),
                output_dir=str(output),
                output_format="WAV",
                log_level=30,
            )
            separator.load_model(model_filename=profile.filename)
            emit_progress("separating", 0.2)
            names = {stem.title(): f"lma-{stem}" for stem in profile.stems}
            produced = [Path(path) for path in separator.separate(str(source), names)]
    except SeparationError:
        raise
    except _OfflineNetworkBlocked as error:
        raise SeparationError(
            "MODEL_NOT_INSTALLED",
            "The installed model bundle is incomplete; online retrieval is disabled.",
            recoverable=True,
        ) from error
    except Exception as error:  # audio-separator errors do not expose stable public types
        raise SeparationError(
            "SEPARATION_FAILED", "audio-separator could not complete separation."
        ) from error

    artifacts = normalize_outputs(produced, output, profile)
    emit_progress("validating", 0.92)
    return {"engine": "audio-separator", "model_id": profile.model_id, "stems": artifacts}


def load_separator_type() -> Any:
    """Load the optional runtime before installing the socket-level offline guard."""
    try:
        from audio_separator.separator import Separator  # type: ignore[import-untyped]
    except Exception as error:  # import failures vary across PyTorch/ONNX installations
        raise SeparationError(
            "MODEL_RUNTIME_UNAVAILABLE",
            "The local separation runtime could not start. Reinstall the analysis worker.",
            recoverable=True,
        ) from error
    return Separator


def profile_from(params: dict[str, Any]) -> SeparationProfile:
    model_id = params.get("model_id")
    if not isinstance(model_id, str) or model_id not in PROFILES:
        raise SeparationError("UNKNOWN_MODEL", "The requested separation model is not registered.")
    return PROFILES[model_id]


def required_path(params: dict[str, Any], key: str) -> Path:
    value = params.get(key)
    if not isinstance(value, str) or not value:
        raise SeparationError("INVALID_REQUEST", f"Missing {key}.")
    return Path(value).resolve(strict=False)


def assert_within(candidate: Path, root: Path, label: str) -> None:
    try:
        candidate.relative_to(root.resolve())
    except ValueError as error:
        raise SeparationError(
            "INVALID_WORKSPACE", f"{label} is outside the track workspace."
        ) from error


def verify_model_marker(model_dir: Path, profile: SeparationProfile) -> None:
    try:
        marker = json.loads((model_dir / ".lma-model.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SeparationError(
            "MODEL_NOT_INSTALLED", "Model installation metadata is invalid."
        ) from error
    if marker != {"model_id": profile.model_id, "model_filename": profile.filename}:
        raise SeparationError(
            "MODEL_NOT_INSTALLED", "Model installation metadata does not match the registry."
        )
    if not (model_dir / profile.filename).is_file():
        raise SeparationError("MODEL_NOT_INSTALLED", "Model configuration file is missing.")


class _OfflineNetworkBlocked(OSError):
    """Raised by the socket guard below; audio-separator attempted network access.

    A distinct type (rather than a bare ``OSError``) so ``separate()`` can
    tell "the offline guard fired" apart from a real disk/permission error
    while writing stem WAVs, which must not be reported as a recoverable
    model-installation problem.
    """


@contextmanager
def offline_network() -> Iterator[None]:
    """Prevent audio-separator's convenience downloader from making a network request."""
    socket_module = cast(Any, socket)
    original_socket = socket_module.socket

    def denied_socket(*args: Any, **kwargs: Any) -> socket.socket:
        raise _OfflineNetworkBlocked("Local Music Analyzer runs audio-separator in offline mode")

    socket_module.socket = denied_socket
    try:
        yield
    finally:
        socket_module.socket = original_socket


def copy_for_test(
    source: Path,
    output: Path,
    profile: SeparationProfile,
    emit_progress: Callable[[str, float], None],
) -> dict[str, Any]:
    artifacts: list[dict[str, str]] = []
    for index, stem in enumerate(profile.stems, start=1):
        destination = output / f"{stem}.wav"
        shutil.copyfile(source, destination)
        artifacts.append({"stem": stem, "relative_path": destination.name})
        emit_progress("separating", 0.1 + (0.75 * index / len(profile.stems)))
    return {"engine": "test-copy", "model_id": profile.model_id, "stems": artifacts}


def normalize_outputs(
    produced: list[Path], output: Path, profile: SeparationProfile
) -> list[dict[str, str]]:
    output_root = output.resolve()
    resolved_outputs: list[Path] = []
    for produced_path in produced:
        candidate = produced_path if produced_path.is_absolute() else output_root / produced_path
        candidate = candidate.resolve(strict=False)
        try:
            candidate.relative_to(output_root)
        except ValueError as error:
            raise SeparationError(
                "INVALID_OUTPUT", "Model returned an output outside the job directory."
            ) from error
        resolved_outputs.append(candidate)

    artifacts: list[dict[str, str]] = []
    for stem in profile.stems:
        matching = next(
            (path for path in resolved_outputs if f"lma-{stem}" in path.name.lower()), None
        )
        if matching is None or not matching.is_file():
            raise SeparationError("INVALID_OUTPUT", f"Model did not produce the {stem} stem.")
        destination = output / f"{stem}.wav"
        matching.replace(destination)
        artifacts.append({"stem": stem, "relative_path": destination.name})
    return artifacts
