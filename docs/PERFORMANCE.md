# Performance

## Princípio

Performance é requisito, mas deve ser medida.

## Domínios

### Latency-sensitive

- audio callback;
- seek;
- mixer controls;
- UI commands;
- cancellation.

Implementar em Rust.

### Throughput-sensitive

- separation;
- feature extraction;
- transcription.

Executar em workers Python.

## IPC

Enviar metadata, não áudio.

Assim o custo de IPC permanece pequeno em relação à inferência.

## CPU

Não ocupar todos os cores sem controle.

Config:

```text
max_cpu_jobs
python_thread_limit
```

Algumas libs científicas já usam threads internas.

Evitar:

```text
N Rust jobs × N BLAS threads
```

que causa oversubscription.

## GPU

Resource scheduler em Rust.

Default:

```text
1 heavy GPU task
```

O worker reporta backend.

Backends possíveis variam por plataforma.

## Model warmup

Manter modelo carregado pode reduzir latência entre jobs.

Tradeoff:

```text
speed vs memory
```

Configurar idle unload.

## Memory

Evitar duplicar áudio:

- Python lê do path;
- Rust não envia buffers;
- waveform usa peaks reduzidos;
- stems podem ser streamados no player.

## Audio playback

Real-time path:

- bounded allocations;
- no blocking IO;
- no Python;
- no network;
- no heavy locks.

## Waveforms

Não transferir milhões de samples para frontend.

Pré-computar envelope.

## Files

Para grandes artefatos:

- stream;
- buffered IO;
- hashes incrementais.

## Benchmark fields

Todo benchmark registra:

```text
platform
CPU
GPU
RAM
audio duration
sample rate
stage
engine
model
wall time
peak memory quando disponível
real-time factor
cache hit
```

## Real-time factor

```text
RTF = processing_seconds / audio_duration_seconds
```

Exemplo:

```text
RTF 0.5 = processa 2x mais rápido que tempo real
RTF 2.0 = demora 2x a duração da faixa
```

## Startup

Tauri deve abrir sem iniciar modelos de ML.

Workers podem iniciar lazy.

AMT worker deve iniciar somente quando necessário.

## Frontend

Timeline virtualization se houver muitos note events.

Não renderizar milhares de DOM nodes individualmente quando canvas/WebGL for mais adequado.

## Benchmark milestones

### B0

App start + worker start.

### B1

Hash + ingest.

### B2

4-stem separation.

### B3

BPM/key/chords.

### B4

4-stem synchronized playback.

### B5

AMT.

O Codex não deve alegar melhoria de performance sem comparar benchmark antes/depois.
