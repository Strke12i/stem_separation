from __future__ import annotations

import json
import logging
import sys
from dataclasses import asdict, dataclass
from typing import Any, TextIO

from music_analyzer_worker import PROTOCOL_VERSION, WORKER_VERSION

LOGGER = logging.getLogger(__name__)
CAPABILITIES = ["ping", "inspect_capabilities", "shutdown"]


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


def error_response(request_id: str, error: ProtocolError) -> dict[str, Any]:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "type": "response",
        "request_id": request_id,
        "ok": False,
        "error": asdict(error),
    }


def response(request_id: str, result: dict[str, Any]) -> dict[str, Any]:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "type": "response",
        "request_id": request_id,
        "ok": True,
        "result": result,
    }


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
    if message.get("type") != "request":
        return None, ProtocolError("MALFORMED_REQUEST", "Message type must be 'request'.")
    if not isinstance(message.get("request_id"), str):
        return None, ProtocolError("MALFORMED_REQUEST", "Request must include a string request_id.")
    if message.get("protocol_version") != PROTOCOL_VERSION:
        return None, ProtocolError(
            "PROTOCOL_MISMATCH",
            f"Unsupported protocol version {message.get('protocol_version')!r}.",
        )
    if not isinstance(message.get("method"), str):
        return None, ProtocolError("MALFORMED_REQUEST", "Request must include a string method.")
    return message, None


def handle_request(message: dict[str, Any]) -> tuple[dict[str, Any], bool]:
    request_id = str(message["request_id"])
    method = message["method"]
    LOGGER.info("request_id=%s method=%s", request_id, method)
    if method == "ping":
        return response(request_id, {"pong": True}), False
    if method == "inspect_capabilities":
        return response(request_id, {"capabilities": CAPABILITIES}), False
    if method == "shutdown":
        return response(request_id, {"shutting_down": True}), True
    return (
        error_response(
            request_id,
            ProtocolError(
                "UNSUPPORTED_METHOD",
                f"Unsupported method: {method}.",
                recoverable=False,
            ),
        ),
        False,
    )


def serve(input_stream: TextIO = sys.stdin, output: TextIO = sys.stdout) -> None:
    """Run the worker until EOF or a successful shutdown request."""
    emit(hello(), output)
    for raw_line in input_stream:
        request, parse_error = parse_request(raw_line)
        if parse_error is not None:
            emit(error_response("unknown", parse_error), output)
            continue
        if request is None:
            continue

        payload, should_stop = handle_request(request)
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
