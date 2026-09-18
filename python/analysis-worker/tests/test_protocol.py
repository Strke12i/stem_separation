from __future__ import annotations

import hashlib
import json
from io import StringIO
from pathlib import Path
from typing import Any

from music_analyzer_worker import PROTOCOL_VERSION
from music_analyzer_worker.__main__ import serve
from music_analyzer_worker.separation import (
    PROFILES,
    load_separator_type,
    normalize_outputs,
    offline_network,
)


def messages_for(input_text: str) -> list[dict[str, Any]]:
    output = StringIO()
    serve(StringIO(input_text), output)
    return [json.loads(line) for line in output.getvalue().splitlines()]


def request(request_id: str, method: str, version: int = PROTOCOL_VERSION) -> str:
    return json.dumps(
        {
            "protocol_version": version,
            "type": "request",
            "request_id": request_id,
            "method": method,
            "params": {},
        }
    )


def test_valid_ping() -> None:
    messages = messages_for(request("req-ping", "ping") + "\n")

    assert messages[0]["type"] == "hello"
    assert messages[0]["protocol_version"] == PROTOCOL_VERSION
    assert messages[1] == {
        "protocol_version": PROTOCOL_VERSION,
        "type": "response",
        "request_id": "req-ping",
        "ok": True,
        "result": {"pong": True},
    }


def test_malformed_request_returns_structured_error() -> None:
    messages = messages_for("not json\n")

    assert messages[1]["error"]["code"] == "MALFORMED_REQUEST"
    assert messages[1]["request_id"] == "unknown"


def test_unsupported_method_returns_structured_error() -> None:
    messages = messages_for(request("req-unknown", "not_a_method") + "\n")

    assert messages[1]["error"]["code"] == "UNSUPPORTED_METHOD"
    assert messages[1]["ok"] is False


def test_shutdown_stops_the_worker() -> None:
    input_text = request("req-stop", "shutdown") + "\n" + request("req-late", "ping") + "\n"
    messages = messages_for(input_text)

    assert len(messages) == 2
    assert messages[1]["result"] == {"shutting_down": True}


def test_preloaded_separator_runtime_survives_the_offline_socket_guard() -> None:
    """The guard must not prevent ssl from initializing during a lazy import."""
    separator_type = load_separator_type()

    with offline_network():
        assert separator_type.__name__ == "Separator"


def test_normalize_outputs_accepts_relative_paths_returned_by_audio_separator(
    tmp_path: Path,
) -> None:
    output = tmp_path / "job-output"
    output.mkdir()
    produced = []
    for stem in PROFILES["demucs-4"].stems:
        filename = f"lma-{stem}.wav"
        (output / filename).write_bytes(b"RIFF" + b"\x00" * 40 + b"WAVE")
        produced.append(Path(filename))

    artifacts = normalize_outputs(produced, output, PROFILES["demucs-4"])

    assert {artifact["stem"] for artifact in artifacts} == set(PROFILES["demucs-4"].stems)
    assert {artifact["relative_path"] for artifact in artifacts} == {
        "bass.wav",
        "drums.wav",
        "other.wav",
        "vocals.wav",
    }


def test_separation_emits_progress_and_only_relative_stems(
    tmp_path: Path, monkeypatch: Any
) -> None:
    workspace = tmp_path / "track-test"
    normalized = workspace / "normalized" / "source.wav"
    normalized.parent.mkdir(parents=True)
    normalized.write_bytes(b"RIFF" + b"\x00" * 40 + b"WAVE")
    model_dir = tmp_path / "models" / "demucs-4"
    model_dir.mkdir(parents=True)
    (model_dir / "htdemucs.yaml").write_text("local test config", encoding="utf-8")
    (model_dir / ".lma-model.json").write_text(
        '{"model_id":"demucs-4","model_filename":"htdemucs.yaml"}', encoding="utf-8"
    )
    config_hash = hashlib.sha256(b"local test config").hexdigest()
    (model_dir / ".lma-bundle.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "model_id": "demucs-4",
                "model_filename": "htdemucs.yaml",
                "installed_at": "2024-01-01T00:00:00Z",
                "files": [
                    {
                        "relative_path": "htdemucs.yaml",
                        "size_bytes": len(b"local test config"),
                        "sha256": config_hash,
                    }
                ],
            }
        ),
        encoding="utf-8",
    )
    output = workspace / "tmp" / "job-test"
    monkeypatch.setenv("LOCAL_MUSIC_ANALYZER_TEST_SEPARATOR", "copy")
    input_text = json.dumps(
        {
            "protocol_version": PROTOCOL_VERSION,
            "type": "request",
            "request_id": "req-separate",
            "job_id": "job-test",
            "method": "separate",
            "params": {
                "workspace_path": str(workspace),
                "input_path": str(normalized),
                "output_dir": str(output),
                "model_id": "demucs-4",
                "model_dir": str(model_dir),
            },
        }
    )

    messages = messages_for(input_text + "\n")

    assert any(message.get("type") == "event" for message in messages)
    result = messages[-1]
    assert result["ok"] is True
    assert result["job_id"] == "job-test"
    assert {stem["stem"] for stem in result["result"]["stems"]} == {
        "vocals",
        "drums",
        "bass",
        "other",
    }
    assert all("/" not in stem["relative_path"] for stem in result["result"]["stems"])


def test_separation_rejects_a_model_bundle_whose_checksum_no_longer_matches(
    tmp_path: Path, monkeypatch: Any
) -> None:
    workspace = tmp_path / "track-test"
    normalized = workspace / "normalized" / "source.wav"
    normalized.parent.mkdir(parents=True)
    normalized.write_bytes(b"RIFF" + b"\x00" * 40 + b"WAVE")
    model_dir = tmp_path / "models" / "demucs-4"
    model_dir.mkdir(parents=True)
    (model_dir / "htdemucs.yaml").write_text("local test config", encoding="utf-8")
    (model_dir / ".lma-model.json").write_text(
        '{"model_id":"demucs-4","model_filename":"htdemucs.yaml"}', encoding="utf-8"
    )
    # The inventory still claims the original checksum, but the file on disk
    # has changed since install - simulating corruption or tampering.
    stale_hash = hashlib.sha256(b"a different original config").hexdigest()
    (model_dir / ".lma-bundle.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "model_id": "demucs-4",
                "model_filename": "htdemucs.yaml",
                "installed_at": "2024-01-01T00:00:00Z",
                "files": [
                    {
                        "relative_path": "htdemucs.yaml",
                        "size_bytes": len(b"a different original config"),
                        "sha256": stale_hash,
                    }
                ],
            }
        ),
        encoding="utf-8",
    )
    output = workspace / "tmp" / "job-test"
    monkeypatch.setenv("LOCAL_MUSIC_ANALYZER_TEST_SEPARATOR", "copy")
    input_text = json.dumps(
        {
            "protocol_version": PROTOCOL_VERSION,
            "type": "request",
            "request_id": "req-separate",
            "job_id": "job-test",
            "method": "separate",
            "params": {
                "workspace_path": str(workspace),
                "input_path": str(normalized),
                "output_dir": str(output),
                "model_id": "demucs-4",
                "model_dir": str(model_dir),
            },
        }
    )

    messages = messages_for(input_text + "\n")

    result = messages[-1]
    assert result["ok"] is False
    assert result["error"]["code"] == "MODEL_INTEGRITY_ERROR"
    assert result["error"]["recoverable"] is False
    assert not output.exists()
