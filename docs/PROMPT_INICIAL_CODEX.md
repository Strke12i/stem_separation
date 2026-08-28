# Prompt inicial para o Codex

Leia `AGENTS.md` e toda a documentação `.md` relevante.

Implemente **somente a Fase 0 — Workspace e boundary** de `ROADMAP.md`.

## Objetivo

Provar que a arquitetura Rust + Python sidecar funciona antes de implementar áudio ou ML.

## Entregas

### Rust

Crie:

- Cargo workspace;
- crate `analyzer-domain`;
- crate `analyzer-protocol`;
- Tauri 2 desktop app mínimo;
- supervisor de sidecar;
- tipos de request/response/event;
- versionamento de protocolo;
- logging com `tracing`;
- IDs de request/job;
- detecção de processo encerrado;
- graceful shutdown;
- timeout de startup;
- comando/serviço `doctor`.

### Python

Crie `analysis-worker` mínimo com Python 3.11 + `uv`.

Ele deve:

- iniciar;
- escrever logs em stderr;
- usar stdout somente para NDJSON;
- emitir handshake;
- responder `ping`;
- responder `inspect_capabilities`;
- aceitar `shutdown`;
- validar protocolo;
- não possuir ainda librosa/PyTorch/audio-separator.

### Frontend

Criar somente UI suficiente para mostrar:

```text
Desktop core: OK
Analysis worker: OK / ERROR
Protocol: v1
[Restart worker]
```

Não criar ainda player, waveform ou análise.

## Testes obrigatórios

### Rust protocol

- serialize request;
- parse response;
- parse event;
- incompatible protocol.

### Python protocol

- valid ping;
- malformed request;
- unsupported method;
- shutdown.

### Supervisor

Teste com fake workers:

1. handshake válido;
2. processo fecha antes do handshake;
3. handshake com versão inválida;
4. stdout retorna JSON inválido;
5. worker trava no startup;
6. worker morre após ficar healthy;
7. graceful shutdown.

## Restrições

Não implementar:

- stem separation;
- librosa;
- Basic Pitch;
- FFmpeg processing;
- audio player;
- model downloads;
- SQLite;
- PyO3;
- Docker.

## Decisões

Use stdin/stdout NDJSON.

Não abra servidor HTTP.

Não use embedded CPython.

O Rust é dono do lifecycle do worker.

## Validação final

Execute e relate os resultados de:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

e no worker:

```text
ruff
mypy
pytest
```

Execute também o app ou um integration harness para provar:

```text
Rust spawn
-> Python hello
-> Rust ping
-> Python pong
-> Rust shutdown
-> clean exit
```

Não diga que está funcionando se essa sequência não foi executada.

Ao finalizar, atualize `IMPLEMENTATION_STATUS.md`.
