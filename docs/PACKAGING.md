# Packaging e distribuição

## Objetivo

Chegar a um instalador desktop sem exigir que o usuário configure manualmente Python.

## Fases

### Desenvolvimento

Rust/Tauri:

```text
cargo / tauri dev
```

Python:

```text
uv sync
uv run ...
```

Workers são executados diretamente do virtualenv.

### Alpha interna

App pode exigir instalação documentada de:

- FFmpeg;
- Python environment;

se isso acelerar validação.

Não tratar isso como distribuição final.

### Release desktop

Objetivo:

```text
Installer
├── Rust/Tauri app
├── analysis-worker sidecar
├── amt-worker sidecar opcional
├── runtime libraries
└── configuration
```

Modelos podem ser instalados separadamente.

## Python sidecar

Estratégia inicial a avaliar:

### PyInstaller onedir

Vantagens:

- amplamente usado;
- Tauri documenta sidecars Python;
- não exige Python do usuário.

Desvantagens:

- bundle grande;
- PyTorch/TensorFlow complicam size;
- builds por plataforma/arquitetura.

Preferir `onedir` inicialmente para debuggability.

Não prometer single executable.

## Tauri externalBin

Sidecar final precisa existir por target triple.

Exemplo conceitual:

```text
analysis-worker-x86_64-pc-windows-msvc.exe
analysis-worker-aarch64-apple-darwin
analysis-worker-x86_64-unknown-linux-gnu
```

Build pipeline gera artefatos por plataforma.

## FFmpeg

Opções:

1. exigir FFmpeg instalado;
2. bundle de uma build compatível;
3. usar decoding Rust para mais casos e FFmpeg apenas onde necessário.

Para release pública, revisar licença da distribuição/binário escolhido.

## Models

Não empacotar todos por padrão.

Model Manager:

```text
model manifest
-> explicit install
-> checksum
-> local cache
```

Offline mode funciona se modelos necessários já estiverem presentes.

## App directories

Usar diretórios apropriados da plataforma.

Separar:

```text
config
cache
models
workspace/library
logs
```

Workspace escolhido pelo usuário pode ficar fora do app data.

## Updates

Não implementar auto-update no MVP.

## Reproducibility

Lock:

- Cargo.lock;
- frontend lockfile;
- uv.lock de cada worker.

Build metadata registra versões.

## Code signing

Futuro release:

- Windows signing;
- macOS signing/notarization.

Não necessário para primeiro protótipo local.

## Size

Bundle de ML será grande.

Não otimizar cedo removendo runtimes essenciais.

Medir por componente:

```text
desktop
analysis worker
AMT worker
models
FFmpeg
```

## Repair

Futuro comando/tela `Doctor` deve detectar:

- sidecar ausente;
- incompatible protocol;
- model checksum mismatch;
- FFmpeg ausente;
- GPU backend inválido.

## PyO3 packaging

Se extensões nativas forem adicionadas:

- usar maturin;
- build wheels por target;
- incluir no worker correspondente.
