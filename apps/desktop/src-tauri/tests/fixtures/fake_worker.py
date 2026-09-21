"""Small deterministic fake used by Rust supervisor integration tests."""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

PROTOCOL_VERSION = 1


def emit(value: object) -> None:
    print(json.dumps(value), flush=True)


def hello(version: int = PROTOCOL_VERSION) -> None:
    emit(
        {
            "protocol_version": version,
            "type": "hello",
            "worker": "fake",
            "worker_version": "0.1.0",
            "capabilities": ["ping"],
        }
    )


def respond(request: dict[str, object]) -> None:
    emit(
        {
            "protocol_version": PROTOCOL_VERSION,
            "type": "response",
            "request_id": request["request_id"],
            "ok": True,
            "result": {"pong": True} if request.get("method") == "ping" else {},
        }
    )


def respond_separation(request: dict[str, object]) -> None:
    params = request["params"]
    if not isinstance(params, dict):
        return
    output = Path(str(params["output_dir"]))
    output.mkdir(parents=True)
    source = Path(str(params["input_path"]))
    stems = ["vocals", "drums", "bass", "other"]
    for stem in stems:
        (output / f"{stem}.wav").write_bytes(source.read_bytes())
    emit({"protocol_version": PROTOCOL_VERSION, "type": "event", "job_id": request["job_id"], "event": "progress", "data": {"stage": "separating", "percent": 0.5}})
    if sys.argv[1] == "separate_slow":
        time.sleep(0.3)
    if sys.argv[1] == "separate_hang":
        # Stands in for a Demucs run that would take minutes.
        time.sleep(60)
    emit({"protocol_version": PROTOCOL_VERSION, "type": "response", "request_id": request["request_id"], "job_id": request["job_id"], "ok": True, "result": {"engine": "test-copy", "model_id": "demucs-4", "stems": [{"stem": stem, "relative_path": f"{stem}.wav"} for stem in stems]}})


def respond_rhythm(request: dict[str, object]) -> None:
    emit({"protocol_version": PROTOCOL_VERSION, "type": "event", "job_id": request["job_id"], "event": "progress", "data": {"stage": "estimating_onsets", "percent": 0.5}})
    emit({"protocol_version": PROTOCOL_VERSION, "type": "response", "request_id": request["request_id"], "job_id": request["job_id"], "ok": True, "result": {"bpm": 120.0, "beat_times": [0.5, 1.0, 1.5, 2.0], "algorithm": "librosa.beat", "stability_score": 1.0}})


def respond_amt(request: dict[str, object]) -> None:
    params = request["params"]
    if not isinstance(params, dict):
        return
    output = Path(str(params["output_dir"]))
    output.mkdir(parents=True)
    midi = b"not a midi file" if sys.argv[1] == "amt_bad_midi" else b"MThd" + b"\x00" * 10
    (output / "transcription.mid").write_bytes(midi)
    emit({"protocol_version": PROTOCOL_VERSION, "type": "event", "job_id": request["job_id"], "event": "progress", "data": {"stage": "writing_midi", "percent": 0.85}})
    emit({"protocol_version": PROTOCOL_VERSION, "type": "response", "request_id": request["request_id"], "job_id": request["job_id"], "ok": True, "result": {"engine": "basic-pitch", "model": "icassp_2022", "relative_midi_path": "transcription.mid", "notes": [{"start": 0.0, "end": 1.0, "midi": 60, "velocity": 90}]}})


def respond_harmony(request: dict[str, object]) -> None:
    emit({"protocol_version": PROTOCOL_VERSION, "type": "event", "job_id": request["job_id"], "event": "progress", "data": {"stage": "extracting_chroma", "percent": 0.3}})
    key = {"tonic": "A", "mode": "minor", "label": "A minor", "score": 0.8, "second_best": "C major", "margin": 0.1}
    chords = [{"start": 0.0, "end": 2.0, "label": "Am", "root": "A", "quality": "minor", "score": 0.9, "beat_aligned": False}]
    emit({"protocol_version": PROTOCOL_VERSION, "type": "response", "request_id": request["request_id"], "job_id": request["job_id"], "ok": True, "result": {"key": key, "chords": chords, "algorithm": "librosa.chroma_cqt+templates"}})


def respond_pitch(request: dict[str, object]) -> None:
    params = request["params"]
    stem = params.get("stem") if isinstance(params, dict) else "bass"
    emit({"protocol_version": PROTOCOL_VERSION, "type": "event", "job_id": request["job_id"], "event": "progress", "data": {"stage": "segmenting_notes", "percent": 0.8}})
    emit({"protocol_version": PROTOCOL_VERSION, "type": "response", "request_id": request["request_id"], "job_id": request["job_id"], "ok": True, "result": {"stem": stem, "engine": "pyin", "notes": [{"start": 0.0, "end": 1.0, "midi": 45, "note": "A2", "confidence": 0.9, "stem": stem, "engine": "pyin"}]}})


def main() -> None:
    mode = sys.argv[1]
    if mode == "exit_before_hello":
        return
    if mode == "invalid_json":
        print("not-json", flush=True)
        return
    if mode == "invalid_version":
        hello(999)
        return
    if mode == "hang":
        time.sleep(30)
        return
    if mode == "amt_crash_once":
        # First process: complete the handshake, then die. A later process
        # (the marker file exists) behaves like a healthy AMT worker.
        marker = Path(sys.argv[2])
        if not marker.exists():
            marker.write_text("crashed")
            hello()
            return
        mode = "amt"

    hello()
    if mode == "die_after_healthy":
        return

    line = sys.stdin.readline()
    if not line:
        return
    request = json.loads(line)
    respond(request)
    if request.get("method") == "shutdown":
        return

    line = sys.stdin.readline()
    if line:
        request = json.loads(line)
        if mode.startswith("slow_"):
            # An analysis that outlasts the short handshake timeout.
            time.sleep(1.2)
            mode = mode.removeprefix("slow_")
        if (
            mode in {"separate", "separate_slow", "separate_hang"}
            and request.get("method") == "separate"
        ):
            respond_separation(request)
        elif mode == "rhythm" and request.get("method") == "analyze_rhythm":
            respond_rhythm(request)
        elif mode == "harmony" and request.get("method") == "analyze_harmony":
            respond_harmony(request)
        elif mode == "pitch" and request.get("method") == "analyze_pitch":
            respond_pitch(request)
        elif mode in {"amt", "amt_bad_midi"} and request.get("method") == "transcribe":
            respond_amt(request)
        else:
            respond(request)


if __name__ == "__main__":
    main()
