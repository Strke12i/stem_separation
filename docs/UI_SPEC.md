# UI desktop

## Stack

Svelte + TypeScript dentro do Tauri.

## Princípio

UI apresenta estado.

Não:

- acessa worker diretamente;
- executa shell;
- implementa mixer real;
- calcula BPM;
- carrega arquivos arbitrariamente sem comando Rust.

## Tela principal

### Library/Import

```text
[ Open audio file ]  [ Browse library ]

Library
[ search name, key or tag ]  [ Recent | Name ]
(funk 2) (practice 1)
- Track.mp3  3:12 · 120.0 BPM · C major   Stems: demucs-4   opened 3x
  (funk ×) [ add tag ]                                     [ Open ]
```

A aba Library é a primeira aba e a única acessível sem track carregada. Reabrir uma
track recarrega o áudio e as análises em cache pelos mesmos comandos de sempre; a UI
só conhece `trackId`, nunca paths.

Ao selecionar:

- Rust abre dialog;
- Rust valida;
- UI recebe metadata.

## Analysis setup

```text
Separation:
[ 4 stems — recommended ]
[ 6 stems — experimental ]

Analysis:
[x] BPM / beats
[x] Key
[x] Chords
[x] Monophonic pitch
[ ] Polyphonic transcription
```

## Progress

UI recebe events do Rust.

Mostrar:

- queued;
- stage;
- determinate/indeterminate progress;
- model loading;
- warning;
- cancel.

## Workspace

Página de faixa:

```text
┌─────────────────────────────────────────┐
│ Track                         119.8 BPM │
│                               A minor   │
├─────────────────────────────────────────┤
│ waveform                                │
│ beats                                   │
│ chords                                  │
│ notes                                   │
├─────────────────────────────────────────┤
│ vocals [M] [S] volume                   │
│ drums  [M] [S] volume                   │
│ bass   [M] [S] volume                   │
│ other  [M] [S] volume                   │
└─────────────────────────────────────────┘
```

## Transport

Commands:

- play;
- pause;
- stop;
- seek;
- master volume.

State vem do Rust.

## Timeline

Frontend recebe:

- waveform peaks;
- beat timestamps;
- chord segments;
- note events.

Renderizar eficientemente.

Não receber PCM completo.

## Mixer

Cada alteração chama Rust.

UI pode usar optimistic visuals apenas se reconcile com state do host.

## Worker health

Settings/diagnostics:

```text
Desktop core: OK
Analysis worker: OK
AMT worker: not installed / stopped / OK
FFmpeg: OK
GPU: ...
```

## Errors

Mensagem principal amigável.

Details expansíveis:

```text
error code
stage
worker exit code
technical detail
```

## Offline

Indicador claro.

Quando offline:

- botões de download de modelo indisponíveis;
- análise existente continua.

## Model manager

Mostrar:

- model ID;
- purpose;
- size;
- installed;
- engine;
- license note;
- device compatibility.

## Performance

Não renderizar UI update para cada evento interno.

Rust pode throttle/coalesce position updates.

## Accessibility

- keyboard controls;
- labels;
- focus;
- contrast;
- não depender apenas de cor.

## Shortcuts futuros

```text
Space = play/pause
M = mute selected
S = solo selected
Left/Right = seek
```
