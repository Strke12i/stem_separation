# Arquitetura híbrida

## Visão

O sistema é dividido em quatro domínios:

```text
Presentation
Desktop/System
Scientific Compute
Artifacts
```

## Diagrama

```text
┌──────────────────────────────────────────────────────────────┐
│ Presentation                                                 │
│ Svelte + TypeScript                                          │
│                                                              │
│ - views                                                      │
│ - timeline                                                   │
│ - waveform                                                   │
│ - controls                                                   │
└───────────────────────────┬──────────────────────────────────┘
                            │ Tauri IPC
                            ▼
┌──────────────────────────────────────────────────────────────┐
│ Desktop/System — Rust                                        │
│                                                              │
│ App Core                                                     │
│ ├── AnalysisOrchestrator                                     │
│ ├── JobScheduler                                             │
│ ├── WorkerSupervisor                                         │
│ ├── WorkspaceRepository                                      │
│ ├── ArtifactRegistry                                         │
│ ├── Cache                                                    │
│ ├── ModelRegistry                                            │
│ └── Settings                                                 │
│                                                              │
│ Audio Engine                                                 │
│ ├── playback                                                 │
│ ├── synchronized stems                                       │
│ ├── mute/solo                                                │
│ ├── gain                                                     │
│ ├── seek                                                     │
│ └── waveform peaks                                           │
└─────────────┬───────────────────────┬────────────────────────┘
              │                       │
         NDJSON IPC              filesystem
              │                       │
              ▼                       ▼
┌─────────────────────────┐   ┌───────────────────────────────┐
│ Scientific — Python     │   │ Artifacts                     │
│                         │   │                               │
│ analysis-worker         │   │ source                        │
│ ├── separation          │   │ normalized                    │
│ ├── rhythm              │   │ stems                         │
│ ├── harmony             │   │ analysis JSON                 │
│ └── pitch               │   │ MIDI                          │
│                         │   │ logs                          │
│ amt-worker              │   │ cache                         │
│ └── Basic Pitch         │   └───────────────────────────────┘
└─────────────────────────┘
```

## Rust workspace

### `analyzer-domain`

Pure Rust.

Contém:

- TrackId;
- JobId;
- Stage;
- JobStatus;
- StemKind;
- Artifact;
- AnalysisManifest;
- errors de domínio.

Não depende de:

- Tauri;
- rodio;
- Python;
- filesystem concreto.

### `analyzer-protocol`

Contratos IPC.

Contém:

- requests;
- responses;
- events;
- protocol version;
- serialization tests.

Pode ser compartilhado com geração de schemas.

### `analyzer-audio`

Áudio interativo.

Contém:

- decoder abstraction;
- player;
- transport;
- stem mixer;
- synchronized source;
- waveform peak extractor.

### `desktop/src-tauri`

Application layer.

Contém:

- Tauri commands;
- state;
- supervisor;
- jobs;
- workspace implementation;
- orchestration.

## Python

### `analysis-worker`

Ambiente principal científico:

- audio-separator;
- librosa;
- NumPy;
- SciPy;
- PyTorch/ONNX dependencies;
- stem separation;
- rhythm;
- key;
- chords;
- monophonic pitch.

### `amt-worker`

Ambiente isolado:

- Basic Pitch;
- dependências próprias.

Não fazer import do AMT no analysis-worker.

## Frontend

A UI é um projection do estado Rust.

Fluxo correto:

```text
Button clicked
-> invoke Tauri command
-> Rust validates
-> Rust changes state / schedules work
-> Rust emits state/progress
-> UI renders
```

Fluxo proibido:

```text
UI
-> spawn Python
```

## Process model

### Main process

Tauri/Rust.

Nunca deve morrer por erro de análise.

### Analysis worker

Processo filho supervisionado.

Pode ser persistente durante a sessão.

Executa um job pesado por vez inicialmente.

Cancelar um job (ou reiniciar o worker) descarta o processo em vez de esperar o
job terminar; o próximo pedido inicia um worker novo (D-023). Enquanto um job
roda, o health check responde "busy" em vez de bloquear.

### AMT worker

Iniciado sob demanda.

Pode ser encerrado quando ocioso para liberar memória.

## Boundary de arquivos

Python recebe paths pertencentes ao workspace.

Não fornecer acesso arbitrário ao filesystem sempre que possível.

Rust cria o workspace antes do job.

Python escreve apenas em:

```text
<track-workspace>/tmp/<job-id>/
```

Rust valida e promove artefatos concluídos para seus destinos finais.

Isso reduz risco de resultados parcialmente escritos.

## Job lifecycle

```text
created
-> queued
-> preparing
-> running
-> finalizing
-> completed
```

Saídas:

```text
cancelled
failed
completed_with_warnings
```

## Pipeline lifecycle

```text
ingest
-> normalize
-> separation
-> rhythm
-> harmony
-> pitch
-> optional AMT
-> consolidate
```

Nem todas as etapas precisam estar dentro de Python.

### Rust

- ingest metadata orchestration;
- hashing;
- workspace;
- FFmpeg invocation;
- artifact promotion;
- cache decisions.

### Python

- scientific calculations;
- ML inference.

## Por que filesystem em vez de passar buffers?

Uma faixa de 5 minutos em float32 stereo 44.1 kHz pode ocupar dezenas ou centenas de MB ao longo de várias representações.

Serializar isso em IPC:

- duplica memória;
- aumenta cópias;
- dificulta cancelamento;
- dificulta recovery.

A fronteira é baseada em paths.

## Estado persistente

MVP:

- manifests JSON;
- filesystem.

SQLite indexa a biblioteca (Fase 12), sem substituir os manifests:

- `<workspace_root>/library/index.sqlite3` é um cache derivado e descartável,
  reconstruível varrendo `track-*/manifest.json`;
- fatos exclusivos da biblioteca (tags, histórico de abertura) ficam em
  `track-*/library.json`, escrito atomicamente; o banco apenas os espelha;
- um banco corrompido ou de outra versão de schema é apagado e recriado.

## Invariantes

1. Rust mantém ownership do job.
2. Worker nunca decide sozinho estado global.
3. Worker não renomeia artefato temporário para final.
4. UI nunca lê arquivos arbitrários por path recebido do Python.
5. Protocol version é validada no handshake.
6. Resultados parciais permanecem identificáveis.
