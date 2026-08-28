# Workflow do Codex

## Antes de editar

1. Leia `AGENTS.md`.
2. Leia os documentos da área.
3. Inspecione Rust workspace, Python workers e frontend.
4. Identifique ownership da mudança.
5. Evite deslocar responsabilidade para linguagem errada.

## Pergunta obrigatória mental

Antes de criar uma função:

```text
Isso pertence a Rust, Python ou UI?
```

### Rust se envolve

- lifecycle;
- OS;
- filesystem;
- audio playback;
- concurrency;
- orchestration;
- process;
- cache;
- safety;
- durable state.

### Python se envolve

- model inference;
- scientific arrays;
- MIR;
- experimentação de algoritmo.

### UI se envolve

- render;
- interaction;
- local display state.

## Boundary first

Para uma feature que cruza Rust/Python:

1. definir contrato;
2. escrever contract tests;
3. implementar worker;
4. implementar Rust client;
5. integrar orchestration;
6. integrar UI.

Não começar pela UI.

## Dependências Rust

Antes de adicionar crate:

- conferir manutenção;
- licença;
- MSRV se relevante;
- features;
- dependências nativas;
- cross-platform.

## Dependências Python

Verificar:

- licença;
- Python 3.11;
- NumPy compatibility;
- GPU backend;
- wheel availability;
- conflito com worker atual.

Se conflitar:

criar worker separado antes de quebrar ambiente estável.

## Concurrency

Não usar `spawn_blocking` como solução universal.

Identificar:

- async IO;
- CPU bound;
- audio realtime;
- external process.

## Unsafe Rust

Evitar.

Se necessário:

- justificar;
- encapsular;
- testar;
- documentar invariantes.

## Errors

Não usar `unwrap`/`expect` em caminhos recuperáveis de produção.

## Logs

Rust:

`tracing`.

Python:

logging para stderr.

IDs comuns.

## Testes por mudança

Rust-only:

```text
fmt
clippy
test
```

Python-only:

```text
ruff
mypy
pytest
```

Boundary:

ambos + protocol integration test.

## Benchmark

Alteração "performance" exige números.

## Documentação

Quando responsabilidade muda entre linguagens:

atualizar:

- `ARCHITECTURE.md`;
- `DECISIONS.md`;
- documento específico.

## Encerramento

Relatar:

- Rust alterado;
- Python alterado;
- frontend alterado;
- testes;
- benchmark se aplicável;
- limitações.
