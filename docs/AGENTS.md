# AGENTS.md

## Papel do agente

Você é o principal agente de engenharia do **Local Music Analyzer**.

O projeto também é uma experiência prática de desenvolvimento desktop em Rust.

Portanto, não use Python por conveniência quando a responsabilidade pertence naturalmente ao host desktop.

## Objetivo técnico

Construir um sistema:

- local-first;
- desktop-first;
- open-source;
- resiliente;
- observável;
- performático;
- modular;
- multiplataforma.

## Regra central

Use cada linguagem onde ela possui vantagem real.

```text
Rust = sistema, desktop, áudio interativo, concorrência, robustez
Python = MIR, ML, experimentação científica
TypeScript = apresentação
```

## Regras não negociáveis

### 1. Rust é o processo principal

O app desktop, estado, jobs e lifecycle pertencem ao Rust.

Python não inicia o aplicativo.

Python não é fonte de verdade do estado global.

### 2. Python é sidecar

O padrão de integração é processo separado.

Não incorporar CPython via PyO3 sem uma decisão arquitetural explícita.

Motivos:

- crash isolation;
- OOM isolation;
- dependency isolation;
- possibilidade de restart;
- packaging separado;
- observabilidade.

### 3. Nenhum áudio via IPC

Nunca enviar buffers de música em JSON, Base64 ou stdin/stdout.

IPC transmite:

- paths;
- IDs;
- parâmetros;
- progresso;
- resultados pequenos;
- erros.

Artefatos grandes são trocados via filesystem.

### 4. IPC versionado

Todo request e event deve carregar versão de protocolo.

Mudança incompatível exige bump.

Consultar `IPC_PROTOCOL.md`.

### 5. Sidecar supervisionado

Rust deve:

- iniciar;
- validar handshake;
- monitorar;
- detectar EOF inesperado;
- registrar exit code;
- matar processo travado;
- reiniciar quando seguro;
- reportar erro amigável.

Não deixar subprocessos órfãos.

### 6. Cancellation real

Jobs devem aceitar cancelamento.

Ordem:

1. enviar cancel request;
2. aguardar grace period;
3. terminar worker se necessário;
4. limpar artefatos temporários;
5. marcar job corretamente.

### 7. Audio playback em Rust

Player e mixer pertencem ao Rust.

Não usar Web Audio como engine principal.

A UI apenas envia comandos e recebe estado.

### 8. ML não bloqueia áudio

Inferência e processamento nunca podem bloquear:

- UI event loop;
- audio output thread;
- Tauri command handler.

### 9. GPU é recurso compartilhado

Por padrão:

- máximo de 1 job GPU pesado simultâneo;
- concorrência CPU é limitada;
- playback não disputa executor de ML.

### 10. Local-first

Áudio não sai da máquina.

Não adicionar:

- APIs remotas de IA;
- telemetria;
- tracking;
- upload;
- analytics externos.

### 11. Offline mode

Quando ativo:

- nenhum download;
- nenhum update check;
- nenhum HTTP;
- nenhum fallback remoto.

### 12. Open-source

Não adicionar dependência obrigatória proprietária.

Revisar licença de:

- crates;
- wheels;
- modelos;
- plugins;
- binários.

### 13. Original imutável

Nunca modificar arquivo do usuário.

### 14. Escritas atômicas

Resultados e manifests devem ser escritos:

```text
temporary
-> fsync quando necessário
-> atomic rename
```

Evitar JSON truncado após crash.

### 15. Cache determinístico

Chave deve incluir:

- source hash;
- stage;
- engine;
- version;
- model;
- parameters.

### 16. Sem shell injection

Processos externos:

- argumentos estruturados;
- nunca interpolar shell command;
- evitar `shell=true`;
- validar paths.

### 17. Tipagem

Rust:

- tipos explícitos de domínio;
- `thiserror` para erros;
- `serde` nos contratos;
- evitar `unwrap()` em caminhos de produção.

Python:

- type hints;
- Pydantic/dataclasses;
- mypy onde viável.

TypeScript:

- strict mode;
- tipos gerados ou derivados do schema quando possível.

### 18. Erros cruzando IPC

Python nunca deve enviar traceback cru como única resposta.

Formato:

```text
error_code
message
recoverable
technical_detail
stage
```

Rust decide o que mostrar na UI.

### 19. Failures parciais

Uma etapa opcional falhar não invalida os artefatos já concluídos.

### 20. PyO3

Permitido para:

- hot paths comprovadamente críticos;
- extensão Rust consumida por Python;
- experimentos controlados.

Não usar para substituir o sidecar sem benchmark e ADR.

### 21. Benchmark antes de otimizar

Toda otimização relevante deve estar ligada a uma medição.

### 22. Cross-platform

Prioridade inicial:

1. Windows x86_64;
2. Linux x86_64;
3. macOS arm64/x86_64.

Não escrever lógica Windows-only no domínio.

## Proibições no MVP

Não adicionar sem solicitação:

- cloud backend;
- login;
- Kubernetes;
- microservices;
- Electron;
- embedded Python;
- database server;
- remote queue;
- web app hospedada;
- updater automático;
- downloader de plataformas.

## Definição de concluído

Uma tarefa só está pronta quando:

- build Rust passa;
- testes Rust relevantes passam;
- testes Python relevantes passam;
- lint passa;
- type checks aplicáveis passam;
- caminho de erro foi considerado;
- documentação afetada foi atualizada;
- nenhuma chamada remota obrigatória foi introduzida.
