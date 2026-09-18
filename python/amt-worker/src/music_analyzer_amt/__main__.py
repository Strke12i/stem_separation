from __future__ import annotations

import json
import logging
import sys
from dataclasses import asdict, dataclass
from typing import Any, TextIO

from music_analyzer_amt import PROTOCOL_VERSION, WORKER_VERSION
from music_analyzer_amt.transcribe import TranscriptionError, transcribe

LOGGER = logging.getLogger(__name__)

CAPABILITIES = ["ping", "inspect_capabilities", "transcribe", "shutdown"]


@dataclass(frozen=True)
class ProtocolError:
    code: str
    message: str
    stage: str = "boundary"
    recoverable: bool = False


def emit(payload: dict[str, Any], output: TextIO = sys.stdout) -> None:
    output.write(json.dumps(payload, separators=(",", ":"), allow_nan=False) + "\n")
    output.flush()


def answer(
    request_id: str,
    *,
    result: dict[str, Any] | None = None,
    error: ProtocolError | None = None,
    job_id: str | None = None,
) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "protocol_version": PROTOCOL_VERSION,
        "type": "response",
        "request_id": request_id,
        "ok": error is None,
    }
    if result is not None:
        payload["result"] = result
    if error is not None:
        payload["error"] = asdict(error)
    if job_id is not None:
        payload["job_id"] = job_id
    return payload


def hello() -> dict[str, Any]:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "type": "hello",
        "worker": "amt",
        "worker_version": WORKER_VERSION,
        "capabilities": CAPABILITIES,
    }


def handle(message: dict[str, Any], output: TextIO) -> tuple[dict[str, Any], bool]:
    request_id = message.get("request_id")
    method = message.get("method")
    job_id = message.get("job_id")
    job_id_str = job_id if isinstance(job_id, str) else None
    # Check request_id first: once it is known valid, every error below can
    # echo it back so Rust can still correlate the failure with the
    # in-flight request instead of timing out.
    if not isinstance(request_id, str):
        return answer(
            "unknown", error=ProtocolError("MALFORMED_REQUEST", "Invalid AMT request.")
        ), False
    if message.get("protocol_version") != PROTOCOL_VERSION:
        return answer(
            request_id,
            error=ProtocolError(
                "PROTOCOL_MISMATCH",
                f"Unsupported protocol version {message.get('protocol_version')!r}.",
            ),
        ), False
    if method == "ping":
        return answer(request_id, result={"pong": True}, job_id=job_id_str), False
    if method == "inspect_capabilities":
        return answer(request_id, result={"capabilities": CAPABILITIES}, job_id=job_id_str), False
    if method == "shutdown":
        return answer(request_id, result={"shutting_down": True}, job_id=job_id_str), True
    if method != "transcribe" or not isinstance(job_id, str):
        return answer(
            request_id,
            error=ProtocolError("UNSUPPORTED_METHOD", "Unsupported AMT method."),
            job_id=job_id_str,
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
        return answer(
            request_id,
            result=transcribe(message.get("params", {}), progress, job_id),
            job_id=job_id,
        ), False
    except TranscriptionError as error:
        return answer(
            request_id,
            error=ProtocolError(error.code, error.message, "amt", error.recoverable),
            job_id=job_id,
        ), False


def serve(input_stream: TextIO = sys.stdin, output: TextIO = sys.stdout) -> None:
    emit(hello(), output)
    for line in input_stream:
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            emit(
                answer(
                    "unknown",
                    error=ProtocolError("MALFORMED_REQUEST", "Request is not valid JSON."),
                ),
                output,
            )
            continue
        if not isinstance(message, dict):
            emit(
                answer(
                    "unknown",
                    error=ProtocolError("MALFORMED_REQUEST", "Request must be an object."),
                ),
                output,
            )
            continue
        try:
            response, stop = handle(message, output)
        except Exception:
            # A single job must never take the whole worker down: an
            # unclassified exception here would otherwise unwind through
            # `serve` and exit the process, silently dropping this request
            # and desynchronizing Rust.
            LOGGER.exception("unhandled error handling request_id=%s", message.get("request_id"))
            request_id = message.get("request_id")
            job_id = message.get("job_id")
            emit(
                answer(
                    request_id if isinstance(request_id, str) else "unknown",
                    error=ProtocolError(
                        "INTERNAL_ERROR", "The worker encountered an unexpected internal error."
                    ),
                    job_id=job_id if isinstance(job_id, str) else None,
                ),
                output,
            )
            continue
        emit(response, output)
        if stop:
            return


def main() -> None:
    logging.basicConfig(stream=sys.stderr, level=logging.INFO)
    serve()


if __name__ == "__main__":
    main()
