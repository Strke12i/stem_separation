# Decisões arquiteturais

## D-001 — Rust como host desktop

Status: accepted

Rust possui:

- lifecycle;
- audio;
- files;
- jobs;
- supervision;
- orchestration.

## D-002 — Python como compute sidecar

Status: accepted

Motivo:

Ecossistema científico maduro sem sacrificar estabilidade do host.

## D-003 — Process isolation em vez de embedded CPython

Status: accepted

Motivo:

- crash isolation;
- OOM isolation;
- dependency isolation;
- restart;
- packaging modular.

PyO3 permanece opcional.

## D-004 — Tauri 2

Status: accepted

Motivo:

Rust host + desktop packaging + sidecar support + webview UI.

## D-005 — Svelte/TypeScript para apresentação

Status: accepted

Motivo:

UI complexa é um domínio em que web tooling é produtivo.

Isso não reduz o papel de Rust porque:

- áudio;
- estado;
- commands;
- files;
- jobs;

permanecem Rust-side.

## D-006 — Player/mixer em Rust

Status: accepted

Não usar Web Audio como engine principal.

## D-007 — NDJSON stdio IPC

Status: accepted

Motivo:

- sem porta;
- simples;
- streamable;
- supervisionável.

## D-008 — Filesystem para artefatos grandes

Status: accepted

Não passar áudio via IPC.

## D-009 — python-audio-separator

Status: accepted

Engine inicial de stem separation.

## D-010 — librosa para MIR inicial

Status: accepted

BPM/beats/chroma/key/chords/pYIN.

## D-011 — Basic Pitch em worker separado

Status: accepted

Motivo:

isolar dependências e memória.

## D-012 — 4 stems default

Status: accepted

6 stems experimental.

## D-013 — Chords próprios no MVP

Status: accepted

`chroma + templates + smoothing`.

## D-014 — FFmpeg chamado por Rust

Status: accepted

Normalização e compatibilidade de mídia.

## D-015 — Symphonia + rodio para playback

Status: accepted

Rust mantém domínio do áudio interativo.

## D-016 — JSON manifests primeiro

Status: accepted

SQLite apenas quando biblioteca justificar.

## D-017 — Model downloads explícitos

Status: accepted

Offline mode nunca baixa.

## D-018 — No downloader in core

Status: accepted

Importadores são plugins futuros.

## D-019 — Performance measured

Status: accepted

Benchmark é requisito para otimização.

## D-020 — PyO3 como laboratório, não arquitetura principal

Status: accepted

O projeto pode explorar Rust/Python bindings depois, especialmente Python → Rust extensions.
