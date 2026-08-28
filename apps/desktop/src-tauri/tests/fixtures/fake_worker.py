"""Small deterministic fake used by Rust supervisor integration tests."""

from __future__ import annotations

import json
import sys
import time

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
        respond(request)


if __name__ == "__main__":
    main()
