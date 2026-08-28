# Testes

## Pirâmide

```text
Rust unit tests
Python unit tests
Protocol contract tests
Synthetic audio tests
Integration tests
Worker process tests
Desktop E2E
```

## Rust

### Domain

- IDs;
- state transitions;
- cache signatures;
- paths;
- artifact promotion;
- errors.

### Protocol

Golden pequeno:

```text
Rust serialize
Python parse
Python serialize
Rust parse
```

Testar version mismatch.

### Supervisor

Usar fake worker executável/script que:

- handshakes;
- responde;
- trava;
- fecha;
- escreve malformed JSON;
- ignora cancel.

Testar recovery.

### Audio

Gerar sinais sintéticos.

Testar:

- duration;
- seek;
- gain;
- mute;
- solo;
- synchronized source math;
- waveform peaks.

## Python

### Rhythm synthetic

Click tracks:

```text
60
90
120
180 BPM
```

### Harmony synthetic

Tríades maiores/menores e progressões.

### Pitch synthetic

```text
110 Hz
220 Hz
440 Hz
```

## Integration

### Ingest

MP3/WAV curto autorizado.

### Worker

Rust spawns real Python worker no ambiente de dev.

### Separation

Marker lento:

```text
model
gpu
integration
```

Não baixar modelo em testes rápidos.

## Failure tests

Obrigatórios:

- worker not found;
- bad protocol version;
- worker crash;
- worker OOM-like exit;
- FFmpeg missing;
- model missing offline;
- invalid audio;
- cancel;
- app restart with interrupted job;
- malformed worker result.

## Copyright

Fixtures preferencialmente:

- sintetizadas;
- domínio público;
- CC0;
- criadas especificamente para testes.

## Performance

Benchmarks separados de testes de correção.

Rust:

Criterion quando fizer sentido.

Python:

pytest-benchmark ou scripts simples somente quando necessário.

## Frontend

Testar lógica de:

- store;
- state mapping;
- error rendering;
- mixer semantics.

Não duplicar testes do domínio Rust no frontend.

## CI futura

Matriz:

```text
Windows x86_64
Linux x86_64
macOS arm64
```

Workers científicos pesados podem ter pipeline separado.

## Definition of Done

Feature Rust/Python não está pronta sem teste da boundary quando cruza IPC.
