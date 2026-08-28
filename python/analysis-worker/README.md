# Analysis worker

Minimal Python 3.11+ sidecar used to prove the Rust/Python boundary in Phase 0.

It communicates exclusively through UTF-8 NDJSON: protocol messages on stdout and
logs on stderr. It intentionally contains no audio, MIR, or ML dependency.
