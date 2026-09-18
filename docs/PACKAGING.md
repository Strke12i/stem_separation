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

### PyInstaller onefile

Vantagens:

- amplamente usado;
- artefato compatível com `externalBin` do Tauri;
- não exige Python do usuário.

Desvantagens:

- bundle grande;
- PyTorch/TensorFlow complicam size;
- builds por plataforma/arquitetura.

O primeiro pipeline usa `onefile`: o `externalBin` do Tauri inclui um binário
por sidecar e, assim, não deixa dependências Python ao lado de fora do
instalador. O custo é uma inicialização um pouco maior, pois o PyInstaller
extrai o runtime em diretório temporário. Se esse custo se tornar relevante,
uma futura revisão poderá usar `onedir` como recurso completo do bundle, e não
somente copiar seu `.exe`.

## Tauri externalBin

Sidecar final precisa existir por target triple.

Exemplo conceitual:

```text
analysis-worker-x86_64-pc-windows-msvc.exe
analysis-worker-aarch64-apple-darwin
analysis-worker-x86_64-unknown-linux-gnu
```

Build pipeline gera artefatos por plataforma.

`tauri.conf.json` mantém o bundle desativado para que `cargo check`, testes e
`tauri dev` não exijam binários de release. O arquivo
`tauri.release.conf.json`, aplicado apenas por `package-desktop.ps1`, ativa o
bundle e declara os dois `externalBin`.

### ImplementaÃ§Ã£o da Fase 11

`scripts/build-sidecars.ps1` produz executáveis Windows com sufixo do target em
`apps/desktop/src-tauri/binaries/`, usando o ambiente bloqueado de cada worker
e PyInstaller `onefile`. Passe `-Amt` para incluir o worker AMT opcional e
`-FfmpegDirectory <diretório-bin>` para copiar `ffmpeg.exe` e `ffprobe.exe`
explicitamente para o bundle.
`scripts/smoke-package.ps1` validates both sidecar handshakes and requires
`ffmpeg.exe` plus `ffprobe.exe` in the same directory before a desktop bundle.
Models remain outside the installer and are installed explicitly.

Para gerar o instalador Windows completo, execute:

```powershell
.\scripts\package-desktop.ps1 -FfmpegDirectory "C:\caminho\para\ffmpeg\bin"
```

O fluxo de release inclui ambos os workers que `externalBin` declara. Nenhuma
etapa baixa modelos: eles são instalados e verificados pelo Model Manager depois
da instalação do app.

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
