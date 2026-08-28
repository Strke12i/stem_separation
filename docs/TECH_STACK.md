# Stack tecnológica

## Desktop

### Rust stable

Linguagem principal do host.

Ferramentas:

- Cargo;
- rustfmt;
- Clippy;
- cargo-nextest opcional;
- cargo-deny opcional.

## Tauri 2

Shell desktop.

Responsabilidades:

- janela;
- lifecycle;
- commands;
- events;
- file dialogs;
- sidecar bundling;
- app directories;
- permissions/capabilities.

Não colocar domínio diretamente nos command handlers.

## UI

### Svelte + TypeScript + Vite

Escolha inicial.

Motivos:

- UI declarativa;
- baixo overhead;
- bom encaixe em Tauri;
- timeline e controles interativos;
- menor complexidade que uma SPA pesada.

Frontend é intercambiável.

## Rust crates

### Tokio

Async orchestration:

- subprocesses;
- channels;
- timers;
- cancellation coordination.

Não executar DSP pesado diretamente no runtime async.

### serde / serde_json

Contratos e manifests.

### thiserror

Errors de domínio.

### tracing

Logging/diagnostics estruturado.

### sha2

Hash de conteúdo.

### tempfile

Temporários seguros.

### rodio

Playback e mixing de áudio.

Usar API atual validada durante implementação.

### cpal

Acesso de baixo nível ao dispositivo de áudio quando necessário.

Preferir abstração de rodio enquanto suficiente.

### Symphonia

Decoding local e metadata em caminhos onde fizer sentido.

FFmpeg continua disponível como fallback/normalizador universal.

### rubato ou alternativa

Somente adicionar para resampling em Rust se benchmark/feature exigir.

Não duplicar FFmpeg prematuramente.

## FFmpeg

Dependência de sistema no desenvolvimento.

Uso:

- probe;
- normalização;
- formatos não cobertos adequadamente;
- conversão canônica.

Chamado pelo Rust.

## Python

### Python 3.11

Base inicial do worker.

### uv

Ambientes e lockfiles independentes.

```text
python/analysis-worker
python/amt-worker
```

Cada worker possui dependências próprias.

### python-audio-separator

Separação de stems.

### librosa

- tempo;
- beats;
- chroma;
- CQT;
- pitch;
- MIR.

### NumPy + SciPy

Base científica.

### Basic Pitch

AMT opcional em worker separado.

## PyO3 + maturin

Ferramentas aprovadas, mas opcionais.

Uso futuro:

- extensão nativa Rust consumida pelo Python;
- benchmark de hot paths.

Não são a fronteira principal Rust/Python.

## Modelos

Perfis iniciais:

### 4 stems

```text
vocals
drums
bass
other
```

Default.

### 6 stems

```text
vocals
drums
bass
guitar
piano
other
```

Experimental.

## Harmonia

Engine inicial Python:

```text
librosa CQT/chroma
+ custom chord templates
+ smoothing
```

## Persistência

MVP:

- filesystem;
- JSON manifests.

Futuro:

- SQLite via `rusqlite` ou `sqlx` para index da biblioteca.

## Testes

Rust:

- built-in unit tests;
- cargo test;
- proptest quando útil;
- insta somente para snapshots pequenos.

Python:

- pytest;
- ruff;
- mypy.

Frontend:

- Vitest para lógica relevante;
- Playwright somente quando fluxo UI justificar.

## Qualidade

Rust:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Python:

```text
uv run ruff check .
uv run mypy ...
uv run pytest
```

## Packaging

Desenvolvimento:

```text
Tauri + uv workers
```

Release:

```text
Tauri bundle
+ Python sidecars por target
+ FFmpeg strategy definida por plataforma
+ modelos separados
```

Consultar `PACKAGING.md`.
