# Status

## Estado atual

As fases 0 e 1 estão implementadas. O aplicativo possui a fronteira Rust/Python
supervisionada e a ingestão local de áudio em Rust. Ainda não há playback, ML ou
análise musical.

## Fases

- [x] Fase 0 — Workspace e boundary
- [x] Fase 1 — Ingestão Rust
- [ ] Fase 2 — Audio engine Rust
- [ ] Fase 3 — Stem separation Python
- [ ] Fase 4 — Stem mixer Rust
- [ ] Fase 5 — Rhythm Python
- [ ] Fase 6 — Harmony Python
- [ ] Fase 7 — Pitch monofônico
- [ ] Fase 8 — AMT worker
- [ ] Fase 9 — Resilience hardening
- [ ] Fase 10 — Performance
- [ ] Fase 11 — Packaging
- [ ] Fase 12 — Library
- [ ] Fase 13 — Advanced Rust

## Registro

### Fase 0 — Workspace e boundary

Date: 2026-08-27
Commit: uncommitted
Rust changes: Cargo workspace; domain/protocol crates; Tauri 2 shell; NDJSON sidecar supervisor; doctor service; tracing with worker/request correlation.
Python changes: Python 3.11 `uv` project with the minimal analysis worker. Stdout is NDJSON-only and logs go to stderr.
Frontend changes: Svelte/TypeScript diagnostics screen with desktop/worker/protocol status and worker restart.
Tests: Rust protocol tests; six fake-worker supervisor tests; one real Rust-to-Python boundary harness; Python protocol tests.
Benchmarks: Not applicable.
Known limitations: Development requires the `uv` environment to be synced. Sidecar packaging is deferred to Phase 11.
Next: Fase 1 — Ingestão Rust.

### Fase 1 — Ingestão Rust

Date: 2026-08-27
Commit: uncommitted
Rust changes: Validated local import; SHA-256 hashing; structured `ffprobe` metadata collection; staging workspace; atomic copies, WAV normalization and manifest publication; manifest/domain types; native Rust file dialog.
Python changes: None.
Frontend changes: Native-file import action plus imported-track metadata and friendly error rendering.
Tests: Four deterministic ingest tests using fake FFmpeg tools and an opt-in real FFmpeg smoke test that generates, imports and normalizes a synthetic MP3.
Benchmarks: Not applicable.
Known limitations: FFmpeg is a development system dependency. Its release packaging strategy remains deferred to Phase 11.
Next: Fase 2 — Audio engine Rust.
