# Protocolo IPC

## Transporte

NDJSON:

```text
1 JSON object por linha
UTF-8
stdout = protocolo
stdin = protocolo
stderr = logs humanos/técnicos
```

Nunca escrever logs arbitrários no stdout do worker.

## Envelope

### Request

```json
{
  "protocol_version": 1,
  "type": "request",
  "request_id": "req-...",
  "job_id": "job-...",
  "method": "analyze_rhythm",
  "params": {}
}
```

### Response

```json
{
  "protocol_version": 1,
  "type": "response",
  "request_id": "req-...",
  "job_id": "job-...",
  "ok": true,
  "result": {}
}
```

### Error response

```json
{
  "protocol_version": 1,
  "type": "response",
  "request_id": "req-...",
  "job_id": "job-...",
  "ok": false,
  "error": {
    "code": "MODEL_OUT_OF_MEMORY",
    "message": "The model could not fit in the selected device.",
    "stage": "separation",
    "recoverable": true,
    "technical_detail": "..."
  }
}
```

### Event

```json
{
  "protocol_version": 1,
  "type": "event",
  "job_id": "job-...",
  "event": "progress",
  "data": {
    "stage": "separation",
    "current": 3,
    "total": 10,
    "message": "Processing segment 3/10"
  }
}
```

## Request IDs

- únicos por processo;
- Rust gera;
- response deve repetir.

## Job IDs

- Rust gera;
- persistentes durante todo pipeline;
- usados em logs e temporários.

## Métodos iniciais

```text
ping
inspect_capabilities
separate
analyze_rhythm
analyze_harmony
analyze_pitch
unload_models
cancel
shutdown
```

AMT worker:

```text
transcribe
cancel
shutdown
```

## Eventos

```text
ready
progress
warning
stage_started
stage_completed
model_loading
model_loaded
memory
```

## Progress

Progress não deve fingir precisão quando engine não fornece progresso real.

Permitido:

```json
{
  "kind": "indeterminate"
}
```

ou:

```json
{
  "kind": "fraction",
  "current": 4,
  "total": 12
}
```

## Cancellation

Rust envia:

```json
{
  "type": "request",
  "method": "cancel",
  "job_id": "job-123"
}
```

Worker deve responder rapidamente mesmo que a tarefa em andamento só possa parar em boundaries.

Se engine nativa estiver bloqueada:

Rust pode usar hard termination após timeout.

## Heartbeat

MVP:

Rust pode usar `ping` apenas quando necessário.

Não gerar heartbeat de alta frequência.

Uma inferência longa pode ser considerada saudável enquanto:

- processo existe;
- stderr/stdout pipes estão abertos;
- timeout máximo não foi atingido.

## Timeout

Timeout é por operação e configurável.

Não usar timeout curto em modelos.

Separar:

```text
startup_timeout
graceful_cancel_timeout
operation_timeout
shutdown_timeout
```

## Stdout discipline

No Python:

```text
stdout = protocol only
stderr = logging
```

Bibliotecas que imprimem em stdout devem ter saída capturada/redirecionada para stderr quando possível.

## Schema

Manter schemas em `/schemas`.

Idealmente gerar:

- Rust serde types;
- Python Pydantic models;
- TypeScript view contracts quando necessário.

Evitar três definições manuais divergentes.

## Versionamento

```text
protocol_version: integer
```

Bump major para quebra.

Campo novo opcional não exige bump quando consumidores antigos ignorarem corretamente.

## Segurança

Worker não aceita:

- comandos shell;
- paths fora das roots autorizadas;
- código Python;
- arbitrary module names;
- arbitrary checkpoint path do usuário.

Model IDs devem vir de registry confiável.
