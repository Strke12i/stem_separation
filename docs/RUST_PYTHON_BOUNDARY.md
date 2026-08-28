# Fronteira Rust ↔ Python

## Estratégia

A integração primária é **process isolation**.

```text
Rust
  |
  +--- spawn ---> Python worker
  |
  +<-- stdout --- structured protocol
  +--> stdin  --- structured protocol
  |
  +--- files ---> workspace
```

## Por que sidecar?

### Isolamento de crash

Um crash dentro de:

- PyTorch;
- ONNX Runtime;
- TensorFlow;
- biblioteca nativa;

não encerra o host desktop.

### Isolamento de memória

O SO pode recuperar memória do worker inteiro após encerramento.

### Dependências

`analysis-worker` e `amt-worker` podem possuir versões de NumPy/TensorFlow diferentes.

### Restart

Worker travado pode ser substituído.

### Packaging

Tauri permite empacotar binários externos por target.

## Por que não HTTP localhost?

Não usar servidor HTTP interno no MVP.

NDJSON sobre stdin/stdout:

- não abre porta;
- não exige autenticação local;
- reduz superfície de ataque;
- simplifica lifecycle;
- acompanha automaticamente o child process.

## Por que não PyO3 como ponte principal?

PyO3 pode embutir CPython, mas isso junta lifecycles.

Problemas:

- crash native pode derrubar app;
- GIL entra no processo principal;
- unloading de runtimes científicos é complexo;
- ambientes Python independentes ficam mais difíceis;
- distribuição se torna mais acoplada.

IPC tem custo desprezível para tarefas de inferência que duram segundos/minutos.

## Quando PyO3 faz sentido?

### Rust → Python embedding

Somente se benchmark mostrar uma chamada de alta frequência onde IPC é gargalo.

### Python → Rust extension

Mais provável.

Se algum DSP usado pelo Python precisar de aceleração:

```text
Python worker
   |
   └── import analyzer_native
              |
              └── Rust/PyO3
```

Construir com `maturin`.

Exemplos futuros:

- segment merging;
- peak extraction;
- custom filters;
- feature post-processing.

Ainda assim, primeiro medir.

## Ownership

### Rust possui

- job ID;
- track ID;
- paths finais;
- config global;
- model registry;
- cache;
- cancellation;
- status final.

### Python possui temporariamente

- arrays;
- tensors;
- loaded models;
- intermediate features;
- temporary artifacts.

## Startup

Fluxo:

```text
Rust spawns worker
-> worker emits hello
-> Rust validates protocol
-> Rust validates capabilities
-> Rust marks worker healthy
```

Worker hello:

```json
{
  "type": "hello",
  "protocol_version": 1,
  "worker": "analysis",
  "worker_version": "0.1.0",
  "capabilities": [
    "separation",
    "tempo",
    "beats",
    "key",
    "chords",
    "pyin"
  ]
}
```

## Shutdown

Rust envia:

```text
shutdown
```

Worker:

1. rejeita novos jobs;
2. finaliza cleanup;
3. responde;
4. encerra.

Se timeout:

Rust termina o processo.

## Crash recovery

Ao receber EOF inesperado:

1. capturar exit status;
2. marcar worker unhealthy;
3. identificar job ativo;
4. invalidar temporários;
5. marcar etapa failed/retryable;
6. reiniciar worker se política permitir.

Nunca reprocessar automaticamente infinitamente.

Default:

```text
automatic restart: yes
automatic job retry: max 1 somente para falha classificada como transitória
```

## Modelo carregado

Worker pode manter modelo em memória entre jobs para performance.

Rust pode enviar:

```text
unload_model
```

quando:

- usuário troca modelo;
- memória está sob pressão;
- worker ficará ocioso;
- job precisa de outro backend.

## GPU

Workers devem reportar:

- backend;
- device;
- VRAM info quando disponível;
- model currently loaded.

Rust usa isso para scheduling.

## Paths

Paths devem ser enviados como strings UTF-8 quando possível.

No Rust, manter suporte correto a `PathBuf`.

Se houver path não UTF-8 em plataforma suportada:

- copiar arquivo para um workspace com path UTF-8 seguro;
- enviar o path interno ao Python.

Não inventar conversão lossy silenciosa.

## Compatibilidade

Protocol deve permitir worker mais novo ou mais antigo somente quando versões forem compatíveis.

Caso contrário:

```text
worker_protocol_mismatch
```

com instrução clara para reparar instalação.
