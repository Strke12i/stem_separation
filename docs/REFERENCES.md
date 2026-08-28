# Referências

Revisão inicial: 2026-08-26.

## Tauri 2

https://v2.tauri.app/

Sidecars:

https://v2.tauri.app/develop/sidecar/

Notas:

- Tauri suporta binários externos;
- documentação cita Python CLI/API server como caso de sidecar;
- bundles exigem binário apropriado por target.

## PyO3

https://pyo3.rs/

Pode:

- criar módulos Python em Rust;
- embutir Python em Rust.

Neste projeto não é a fronteira principal do MVP.

## maturin

https://www.maturin.rs/

Build/publishing de módulos PyO3/cffi/uniffi.

## rodio

https://github.com/RustAudio/rodio

https://docs.rs/rodio/

Playback/mixing em Rust.

## CPAL

https://github.com/RustAudio/cpal

Low-level cross-platform audio I/O.

## Symphonia

https://github.com/pdeljanov/Symphonia

https://docs.rs/symphonia/

Audio decoding/demuxing 100% Rust.

## python-audio-separator

https://github.com/nomadkaraoke/python-audio-separator

Uso:

- stem separation;
- múltiplas famílias de modelos.

Licença do projeto: MIT.

Revisar também licença/termos de cada modelo/peso.

## librosa

https://github.com/librosa/librosa

https://librosa.org/

MIR/DSP.

## Basic Pitch

https://github.com/spotify/basic-pitch

AMT opcional.

## FFmpeg

https://ffmpeg.org/

Revisar distribuição/licenciamento de build caso seja embutido no installer.

## uv

https://github.com/astral-sh/uv

Ambientes Python.

## Svelte

https://svelte.dev/

Frontend.

## Regra

Antes de pin de produção:

- verificar release atual;
- changelog;
- licença;
- plataforma;
- breaking changes.
