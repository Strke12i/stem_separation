from __future__ import annotations

import json
import logging
import sys
from dataclasses import asdict, dataclass
from typing import Any, TextIO

from music_analyzer_amt import PROTOCOL_VERSION, WORKER_VERSION
from music_analyzer_amt.transcribe import TranscriptionError, transcribe

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
    if not isinstance(request_id, str) or message.get("protocol_version") != PROTOCOL_VERSION:
        return answer(
            "unknown", error=ProtocolError("MALFORMED_REQUEST", "Invalid AMT request.")
        ), False
    if method == "ping":
        return answer(request_id, result={"pong": True}), False
    if method == "inspect_capabilities":
        return answer(request_id, result={"capabilities": CAPABILITIES}), False
    if method == "shutdown":
        return answer(request_id, result={"shutting_down": True}), True
    if method != "transcribe" or not isinstance(job_id, str):
        return answer(
            request_id, error=ProtocolError("UNSUPPORTED_METHOD", "Unsupported AMT method.")
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
            request_id, result=transcribe(message.get("params", {}), progress), job_id=job_id
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
        response, stop = handle(message, output)
        emit(response, output)
        if stop:
            return


def main() -> None:
    logging.basicConfig(stream=sys.stderr, level=logging.INFO)
    serve()


if __name__ == "__main__":
    main()
