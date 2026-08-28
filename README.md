# Local Music Analyzer — Rust + Python

Aplicativo desktop local e open-source para análise musical.

A arquitetura combina:

- **Rust** para o aplicativo desktop, player/mixer, filesystem, jobs, IPC, cache e supervisão;
- **Python** para separação de stems, MIR e modelos de ML já maduros;
- **Tauri 2** como shell desktop;
- **Svelte + TypeScript** como camada visual;
- **sidecars Python isolados** para proteger o processo principal contra falhas de ML.

## Objetivo

Receber uma faixa local e produzir:

- stems por instrumento;
- BPM e grade de beats;
- tonalidade;
- timeline de acordes;
- pitch/notas por stem;
- MIDI opcional;
- visualização sincronizada;
- player/mixer com solo, mute e volume;
- artefatos JSON reproduzíveis.

Tudo deve funcionar localmente. O áudio não deve ser enviado a serviços externos.

## Princípio arquitetural

```text
┌─────────────────────────────────────────────────────────────┐
│                    Desktop Application                      │
│                                                             │
│  Svelte/TS UI                                               │
│      │                                                      │
│      ▼                                                      │
│  Tauri Commands / Events                                    │
│      │                                                      │
│      ▼                                                      │
│  Rust Application Core                                      │
│  ├── audio player/mixer                                     │
│  ├── workspace/files                                        │
│  ├── job scheduler                                          │
│  ├── cache                                                  │
│  ├── model registry                                         │
│  ├── sidecar supervisor                                     │
│  └── IPC contracts                                          │
│      │                                                      │
│      ├──────────── NDJSON/stdin/stdout ──────────────┐       │
│      ▼                                              ▼       │
│ Python Analysis Worker                     Python AMT Worker │
│ ├── audio-separator                        └── Basic Pitch   │
│ ├── librosa                                                │
│ ├── NumPy/SciPy                                            │
│ ├── rhythm                                                │
│ ├── harmony                                               │
│ └── monophonic pitch                                      │
└─────────────────────────────────────────────────────────────┘
```

## Por que não embutir Python no processo Rust?

O projeto privilegia isolamento.

Operações de ML podem:

- consumir muita RAM/VRAM;
- falhar ao carregar um modelo;
- provocar OOM;
- carregar runtimes pesados;
- possuir árvores de dependências incompatíveis.

O aplicativo Rust deve sobreviver a esses problemas.

Por isso, o padrão inicial é:

```text
Rust host
    |
    └── managed Python sidecar
```

e não:

```text
Rust host
    |
    └── embedded CPython
```

PyO3 continua sendo uma ferramenta permitida para integrações futuras específicas, mas não é a fronteira principal do MVP.

## Linguagem por responsabilidade

### Rust

Usar para:

- desktop lifecycle;
- Tauri commands/events;
- path safety;
- hash;
- filesystem;
- manifests;
- atomic writes;
- jobs;
- cancellation;
- timeout;
- process supervision;
- audio playback;
- stem synchronization;
- waveform peak generation;
- cache;
- resource limits;
- hardware discovery;
- orchestration;
- IPC.

### Python

Usar para:

- stem separation;
- PyTorch/ONNX-based models;
- librosa;
- BPM/beat analysis;
- chroma;
- key detection;
- chord recognition;
- pYIN;
- Basic Pitch;
- experimentation with MIR/ML.

### TypeScript/Svelte

Usar apenas para apresentação e interação:

- mixer controls;
- timeline;
- waveform;
- settings;
- model manager UI;
- progress;
- errors.

A UI não implementa regras de negócio nem DSP.

## Estrutura alvo

```text
local-music-analyzer/
├── apps/
│   └── desktop/
│       ├── src/                     # Svelte/TypeScript UI
│       └── src-tauri/
│           ├── src/
│           │   ├── commands/
│           │   ├── audio/
│           │   ├── jobs/
│           │   ├── ipc/
│           │   ├── models/
│           │   ├── supervisor/
│           │   ├── workspace/
│           │   └── state/
│           └── binaries/            # release sidecars
├── crates/
│   ├── analyzer-domain/
│   ├── analyzer-protocol/
│   ├── analyzer-audio/
│   └── analyzer-test-support/
├── python/
│   ├── analysis-worker/
│   │   └── src/music_analyzer_worker/
│   └── amt-worker/
│       └── src/music_analyzer_amt/
├── schemas/
├── tests/
│   ├── fixtures/
│   └── synthetic/
├── scripts/
├── models/
└── workspace/
```

## Regra de dependência

```text
UI
 ↓
Rust application
 ↓
Rust domain/protocol
 ↓
Python workers

Python nunca chama a UI.
UI nunca chama Python diretamente.
```

## Ordem de leitura para o Codex

1. `AGENTS.md`
2. `ARCHITECTURE.md`
3. `RUST_PYTHON_BOUNDARY.md`
4. `TECH_STACK.md`
5. `IPC_PROTOCOL.md`
6. `AUDIO_ENGINE.md`
7. `AUDIO_PIPELINE.md`
8. `DATA_CONTRACTS.md`
9. `RESILIENCE.md`
10. `PERFORMANCE.md`
11. `SECURITY_PRIVACY.md`
12. `TESTING.md`
13. `PACKAGING.md`
14. `UI_SPEC.md`
15. `ROADMAP.md`
16. `CODEX_WORKFLOW.md`
17. `DECISIONS.md`
18. `REFERENCES.md`

A primeira tarefa está em `PROMPT_INICIAL_CODEX.md`.
