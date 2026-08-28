# Mudanças em relação ao plano Python-first

## Antes

```text
NiceGUI
  |
Python application core
  |
Python analysis engines
```

## Agora

```text
Svelte UI
  |
Tauri
  |
Rust application core
  |
managed Python workers
```

## Responsabilidades migradas para Rust

### Desktop

Antes:

NiceGUI/Python.

Agora:

Tauri/Rust + Svelte.

### Playback

Antes:

não definido em profundidade.

Agora:

Rust/rodio/cpal/Symphonia.

### Orchestration

Antes:

Python orchestrator.

Agora:

Rust.

### Workspace

Antes:

Python.

Agora:

Rust.

### Cache

Antes:

Python.

Agora:

Rust.

### Job lifecycle

Antes:

Python.

Agora:

Rust.

### Process recovery

Antes:

não era boundary central.

Agora:

parte explícita do Rust supervisor.

### Waveform preprocessing

Antes:

UI/Python possível.

Agora:

Rust.

## Responsabilidades que permanecem Python

- audio-separator;
- librosa;
- NumPy/SciPy;
- BPM/beats;
- key;
- chords;
- pYIN;
- Basic Pitch.

## Mudança importante de confiabilidade

Antes:

Uma exceção pesada de ML estava no mesmo runtime do aplicativo.

Agora:

```text
ML crash
   ↓
worker dies
   ↓
Rust detects
   ↓
job fails/retries
   ↓
app remains open
```

## Mudança importante de dependências

Antes:

Um grande ambiente Python.

Agora:

```text
analysis-worker env
amt-worker env
```

Isso evita resolver à força dependências incompatíveis.

## Mudança de UI

NiceGUI foi removido.

Motivo:

O foco passou de protótipo Python local para aplicativo desktop Rust.

Tauri fornece melhor superfície para:

- packaging;
- lifecycle;
- native integrations;
- sidecars;
- learning Rust desktop development.

## PyO3

Adicionado como ferramenta futura, não obrigatória.

O projeto pode usá-lo para explorar:

- Python extension em Rust;
- otimização de DSP;
- bindings.

Isso vira uma trilha educacional da Fase 13.

## Resultado

A nova arquitetura exige mais engenharia inicialmente, mas oferece:

- melhor isolamento;
- melhor desktop lifecycle;
- player de áudio mais controlado;
- oportunidade real de aprender Rust;
- manutenção clara das vantagens do ecossistema Python.
