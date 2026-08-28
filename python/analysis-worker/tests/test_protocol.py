from __future__ import annotations

import json
from io import StringIO
from typing import Any

from music_analyzer_worker import PROTOCOL_VERSION
from music_analyzer_worker.__main__ import serve


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
    messages = messages_for(request("req-unknown", "separate") + "\n")

    assert messages[1]["error"]["code"] == "UNSUPPORTED_METHOD"
    assert messages[1]["ok"] is False


def test_shutdown_stops_the_worker() -> None:
    input_text = request("req-stop", "shutdown") + "\n" + request("req-late", "ping") + "\n"
    messages = messages_for(input_text)

    assert len(messages) == 2
    assert messages[1]["result"] == {"shutting_down": True}
