# Roadmap híbrido Rust + Python

## Fase 0 — Workspace e boundary

Objetivo:

Provar a arquitetura antes de áudio/ML.

Entregas:

- Cargo workspace;
- Tauri 2 app mínimo;
- Svelte/TS;
- crates domain/protocol;
- Python analysis-worker mínimo;
- NDJSON handshake;
- supervisor Rust;
- `doctor`;
- logs correlacionados;
- testes de protocol/crash.

DoD:

O app abre, inicia worker, valida versão, faz `ping`, mata/reinicia fake worker e continua vivo.

## Fase 1 — Ingestão Rust

- file dialog;
- hashing;
- ffprobe;
- workspace;
- manifest;
- atomic writes;
- normalize via FFmpeg.

DoD:

Importar MP3 cria workspace reproduzível.

## Fase 2 — Audio engine Rust

- rodio/cpal;
- Symphonia;
- transport;
- waveform peaks;
- play original;
- device error recovery.

DoD:

Player funciona sem Python.

## Fase 3 — Stem separation Python

- audio-separator;
- model registry inicial;
- 4 stems;
- 6 stems experimental;
- progress;
- cache;
- cancellation;
- artifact promotion.

DoD:

Stems são gerados por sidecar e validados pelo Rust.

## Fase 4 — Stem mixer Rust

- synchronized playback;
- mute;
- solo;
- volume;
- seek;
- UI mixer.

DoD:

4 stems permanecem sincronizados durante playback e seek.

## Fase 5 — Rhythm Python

- BPM;
- beats;
- synthetic tests;
- timeline UI.

## Fase 6 — Harmony Python

- chroma;
- key;
- chord templates;
- smoothing;
- beat alignment;
- UI timeline.

## Fase 7 — Pitch monofônico

- pYIN;
- note segmentation;
- per-stem analysis.

## Fase 8 — AMT isolated worker

- separate environment;
- Basic Pitch;
- MIDI;
- worker lifecycle;
- piano roll.

## Fase 9 — Resilience hardening

- crash injection;
- OOM scenarios;
- interrupted jobs;
- repair;
- stale temp cleanup;
- worker restart limits.

## Fase 10 — Performance

- benchmark suite;
- GPU scheduler;
- model warm cache;
- memory tuning;
- waveform optimization;
- frontend timeline performance.

## Fase 11 — Packaging

- per-platform sidecars;
- installer;
- models manager;
- FFmpeg strategy;
- smoke tests.

## Fase 12 — Library

Opcional:

- SQLite index;
- history;
- search;
- tags.

## Fase 13 — Advanced Rust

Exploração educacional:

- PyO3 extension benchmark;
- Rust DSP primitives;
- custom resampling;
- optimized peak extraction;
- SIMD experiments;
- custom audio effects.

Só promover experimentos para produção quando medidos.

## Backlog

- loop A/B;
- metronome;
- time stretch;
- pitch shift;
- sections;
- downbeats;
- time signature;
- MusicXML;
- tabs;
- manual chord correction;
- plugin importers.
