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

## D-021 — rusqlite bundled como índice derivado da biblioteca

Status: accepted

A biblioteca (Fase 12) usa `rusqlite` com SQLite embutido, síncrono, alinhado ao
resto do código (I/O de arquivo inline, sem padrão async de banco). O banco é um
cache derivado: manifests continuam sendo a fonte de verdade (D-016) e tags/histórico
ficam em `track-*/library.json`, de modo que apagar `index.sqlite3` nunca perde
dados. Não há framework de migrações: `PRAGMA user_version` diferente da constante
do código apaga e reconstrói o banco. Busca usa `LIKE` com escape; FTS5 fica fora
até haver escala que o justifique.

## D-022 — Otimizações só entram com medição, e o gargalo real vem primeiro

Status: accepted

Resultado da Fase 13 aplicando D-019 e D-020. Antes de escrever código nativo, o
custo de cada etapa foi medido numa faixa real. O maior ganho veio de vetorizar
`chord_frames` e `segment` em NumPy (~45× e ~8×, sem toolchain nova). Peaks de
waveform (~9% do carregamento, dominado pelo decode) e uma extensão PyO3 para
`smooth` (10×, mas ≈80 ms por faixa) não justificaram promoção: a primeira ficou
mais lenta com auto-vetorização e a segunda exigiria Rust no empacotamento do
sidecar e um wheel por versão de Python e plataforma. Ambos ficam como
experimentos reproduzíveis (`waveform_benchmark`, `labs/analyzer-native`).
Reavaliar a extensão PyO3 apenas se uma etapa de Python passar a custar uma
fração relevante do tempo de análise.

