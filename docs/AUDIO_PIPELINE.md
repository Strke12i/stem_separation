# Pipeline de análise

## Princípio

Pipeline batch e playback são sistemas separados.

```text
Batch analysis -> Python workers
Interactive audio -> Rust
```

## Stage 0 — ingest

Rust:

- valida path;
- cria TrackId;
- SHA-256;
- coleta metadata;
- cria workspace;
- registra manifest.

## Stage 1 — normalize

Rust chama FFmpeg.

Resultado canônico:

```text
normalized/source.wav
```

Não alterar original.

## Stage 2 — separate

Rust:

1. verifica cache;
2. cria temp output root;
3. agenda GPU/CPU;
4. envia request ao worker;
5. acompanha progress;
6. valida outputs;
7. promove arquivos atomically.

Python:

- carrega engine/model;
- separa;
- escreve temporários;
- retorna manifest dos arquivos.

## Stage 3 — rhythm

Python:

```text
audio
-> onset envelope
-> tempo
-> beats
```

Rust valida JSON e persiste.

## Stage 4 — key

Python:

```text
harmonic signal
-> chroma
-> key profiles
-> key/mode
```

## Stage 5 — chords

Python:

```text
CQT/chroma
-> templates
-> frame scores
-> smoothing
-> segments
-> optional beat alignment
```

Vocabulário inicial:

```text
12 major
12 minor
N
```

## Stage 6 — monophonic pitch

Python/pYIN.

Prioridade:

- bass;
- vocals;
- lead-like stems.

## Stage 7 — AMT

Worker independente.

Executado somente quando solicitado/configurado.

## Stage 8 — waveform cache

Rust.

Gerar peaks para:

- original;
- stems relevantes.

Não depender do Python.

## Stage 9 — consolidate

Rust cria manifest final.

## Cache boundary

Cada stage possui cache próprio.

Exemplo:

Mudar chord smoothing:

- não refaz stems;
- não refaz rhythm;
- refaz harmony/chords quando assinatura muda.

## Job graph

Evitar pipeline rigidamente linear quando etapas forem independentes.

Após separação:

```text
             +--> rhythm
separation --+--> harmony
             +--> pitch
```

Rodar em paralelo somente quando recursos permitirem.

## Resource policy

Default conservador:

```text
GPU-heavy jobs: 1
CPU-heavy jobs: bounded
disk IO: bounded
```

Não paralelizar por aparência de performance.

Medir.

## Cancel

Cada stage deve possuir cleanup.

Artefatos finalizados de stages anteriores podem permanecer.

## Retry

Somente erros transitórios.

Não repetir automaticamente:

- invalid input;
- unsupported model;
- deterministic decode failure.

Pode repetir uma vez:

- worker crash transitório;
- device reset;
- temporary filesystem issue;

se classificado explicitamente.
