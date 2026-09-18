from __future__ import annotations

import json
import logging
import sys
from dataclasses import asdict, dataclass
from typing import Any, TextIO

from music_analyzer_worker import PROTOCOL_VERSION, WORKER_VERSION
from music_analyzer_worker.harmony import HarmonyError
from music_analyzer_worker.harmony import analyze as analyze_harmony
from music_analyzer_worker.pitch import PitchError
from music_analyzer_worker.pitch import analyze as analyze_pitch
from music_analyzer_worker.rhythm import RhythmError, analyze
from music_analyzer_worker.separation import SeparationError, separate

LOGGER = logging.getLogger(__name__)
CAPABILITIES = [
    "ping",
    "inspect_capabilities",
    "separate",
    "analyze_rhythm",
    "analyze_harmony",
    "analyze_pitch",
    "shutdown",
]


@dataclass(frozen=True)
class ProtocolError:
    code: str
    message: str
    stage: str = "boundary"
    recoverable: bool = False
    technical_detail: str | None = None


def emit(payload: dict[str, Any], output: TextIO = sys.stdout) -> None:
    """Emit exactly one protocol message to stdout."""
    output.write(json.dumps(payload, separators=(",", ":"), allow_nan=False) + "\n")
    output.flush()


def error_response(
    request_id: str, error: ProtocolError, job_id: str | None = None
) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "protocol_version": PROTOCOL_VERSION,
        "type": "response",
        "request_id": request_id,
        "ok": False,
        "error": asdict(error),
    }
    if job_id is not None:
        payload["job_id"] = job_id
    return payload


def response(request_id: str, result: dict[str, Any], job_id: str | None = None) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "protocol_version": PROTOCOL_VERSION,
        "type": "response",
        "request_id": request_id,
        "ok": True,
        "result": result,
    }
    if job_id is not None:
        payload["job_id"] = job_id
    return payload


def hello() -> dict[str, Any]:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "type": "hello",
        "worker": "analysis",
        "worker_version": WORKER_VERSION,
        "capabilities": CAPABILITIES,
    }


def parse_request(raw: str) -> tuple[dict[str, Any] | None, ProtocolError | None]:
    try:
        message = json.loads(raw)
    except json.JSONDecodeError as error:
        return None, ProtocolError(
            "MALFORMED_REQUEST",
            "Request is not valid JSON.",
            technical_detail=str(error),
        )

    if not isinstance(message, dict):
        return None, ProtocolError("MALFORMED_REQUEST", "Request must be a JSON object.")
    # Check request_id before any other field: once it is known valid, every
    # error below can echo it back so Rust can still correlate the failure
    # with the in-flight request instead of timing out.
    if not isinstance(message.get("request_id"), str):
        return None, ProtocolError("MALFORMED_REQUEST", "Request must include a string request_id.")
    if message.get("type") != "request":
        return message, ProtocolError("MALFORMED_REQUEST", "Message type must be 'request'.")
    if message.get("protocol_version") != PROTOCOL_VERSION:
        return message, ProtocolError(
            "PROTOCOL_MISMATCH",
            f"Unsupported protocol version {message.get('protocol_version')!r}.",
        )
    if not isinstance(message.get("method"), str):
        return message, ProtocolError("MALFORMED_REQUEST", "Request must include a string method.")
    return message, None


def handle_request(message: dict[str, Any], output: TextIO) -> tuple[dict[str, Any], bool]:
    request_id = str(message["request_id"])
    method = message["method"]
    job_id = message.get("job_id")
    job_id_str = job_id if isinstance(job_id, str) else None
    LOGGER.info("request_id=%s method=%s", request_id, method)
    if method == "ping":
        return response(request_id, {"pong": True}, job_id_str), False
    if method == "inspect_capabilities":
        return response(request_id, {"capabilities": CAPABILITIES}, job_id_str), False
    if method == "shutdown":
        return response(request_id, {"shutting_down": True}, job_id_str), True
    if method == "separate":
        if not isinstance(job_id, str):
            return error_response(
                request_id, ProtocolError("MALFORMED_REQUEST", "Separation requires a job_id.")
            ), False

        def progress(stage: str, percent: float) -> None:
            emit(
                {
                    "protocol_version": PROTOCOL_VERSION,
                    "type": "event",
                    "job_id": job_id,
                    "event": "progress",
                    "data": {"stage": stage, "percent": percent},
                },
                output,
            )

        try:
            result = separate(message.get("params", {}), progress, job_id)
        except SeparationError as error:
            return error_response(
                request_id,
                ProtocolError(error.code, error.message, "separation", error.recoverable),
                job_id,
            ), False
        return response(request_id, result, job_id), False
    if method == "analyze_rhythm":
        if not isinstance(job_id, str):
            return error_response(
                request_id, ProtocolError("MALFORMED_REQUEST", "Rhythm analysis requires a job_id.")
            ), False

        def rhythm_progress(stage: str, percent: float) -> None:
            emit(
                {
                    "protocol_version": PROTOCOL_VERSION,
                    "type": "event",
                    "job_id": job_id,
                    "event": "progress",
                    "data": {"stage": stage, "percent": percent},
                },
                output,
            )

        try:
            result = analyze(message.get("params", {}), rhythm_progress)
        except RhythmError as error:
            return error_response(
                request_id,
                ProtocolError(error.code, error.message, "rhythm", error.recoverable),
                job_id,
            ), False
        return response(request_id, result, job_id), False
    if method == "analyze_harmony":
        if not isinstance(job_id, str):
            return error_response(
                request_id,
                ProtocolError("MALFORMED_REQUEST", "Harmony analysis requires a job_id."),
            ), False

        def harmony_progress(stage: str, percent: float) -> None:
            emit(
                {
                    "protocol_version": PROTOCOL_VERSION,
                    "type": "event",
                    "job_id": job_id,
                    "event": "progress",
                    "data": {"stage": stage, "percent": percent},
                },
                output,
            )

        try:
            result = analyze_harmony(message.get("params", {}), harmony_progress)
        except HarmonyError as error:
            return error_response(
                request_id, ProtocolError(error.code, error.message, "harmony", False), job_id
            ), False
        return response(request_id, result, job_id), False
    if method == "analyze_pitch":
        if not isinstance(job_id, str):
            return error_response(
                request_id,
                ProtocolError("MALFORMED_REQUEST", "Pitch analysis requires a job_id."),
            ), False

        def pitch_progress(stage: str, percent: float) -> None:
            emit(
                {
                    "protocol_version": PROTOCOL_VERSION,
                    "type": "event",
                    "job_id": job_id,
                    "event": "progress",
                    "data": {"stage": stage, "percent": percent},
                },
                output,
            )

        try:
            result = analyze_pitch(message.get("params", {}), pitch_progress)
        except PitchError as error:
            return error_response(
                request_id, ProtocolError(error.code, error.message, "pitch", False), job_id
            ), False
        return response(request_id, result, job_id), False
    return (
        error_response(
            request_id,
            ProtocolError(
                "UNSUPPORTED_METHOD",
                f"Unsupported method: {method}.",
                recoverable=False,
            ),
            job_id_str,
        ),
        False,
    )


def serve(input_stream: TextIO = sys.stdin, output: TextIO = sys.stdout) -> None:
    """Run the worker until EOF or a successful shutdown request."""
    emit(hello(), output)
    for raw_line in input_stream:
        request, parse_error = parse_request(raw_line)
        if parse_error is not None:
            request_id = request.get("request_id") if isinstance(request, dict) else None
            known_id = request_id if isinstance(request_id, str) else "unknown"
            emit(error_response(known_id, parse_error), output)
            continue
        if request is None:
            continue

        try:
            payload, should_stop = handle_request(request, output)
        except Exception:
            # A single job must never take the whole worker down: an
            # unclassified exception here (OOM, an unstable library error)
            # would otherwise unwind through `serve` and exit the process,
            # silently dropping this request and desynchronizing Rust.
            LOGGER.exception("unhandled error handling request_id=%s", request.get("request_id"))
            request_id = request.get("request_id")
            job_id = request.get("job_id")
            emit(
                error_response(
                    request_id if isinstance(request_id, str) else "unknown",
                    ProtocolError(
                        "INTERNAL_ERROR",
                        "The worker encountered an unexpected internal error.",
                        recoverable=False,
                    ),
                    job_id if isinstance(job_id, str) else None,
                ),
                output,
            )
            continue
        emit(payload, output)
        if should_stop:
            return


def main() -> None:
    logging.basicConfig(stream=sys.stderr, level=logging.INFO, format="%(levelname)s %(message)s")
    LOGGER.info("analysis worker starting; protocol=%s", PROTOCOL_VERSION)
    serve()
    LOGGER.info("analysis worker stopped")


if __name__ == "__main__":
    main()
