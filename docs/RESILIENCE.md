# Resiliência

## Objetivo

Um modelo falhar não deve equivaler a "o aplicativo fechou".

## Failure domains

Separar:

```text
UI failure
Rust command failure
audio device failure
worker failure
model failure
filesystem failure
artifact validation failure
```

## Worker state machine

```text
stopped
-> starting
-> healthy
-> busy
-> healthy
```

Falhas:

```text
starting -> failed
busy -> crashed
healthy -> unresponsive
```

## Health

Worker saudável quando:

- process alive;
- handshake válido;
- protocol compatível.

Não interpretar "job lento" automaticamente como worker travado.

## Restart

Worker pode ser reiniciado:

- após crash;
- após OOM;
- após explicit reset;
- após troca de ambiente.

Limite:

```text
max restart attempts in window
```

Evitar restart loop.

## OOM

Se worker morrer possivelmente por OOM:

- preservar diagnóstico possível;
- recomendar CPU/modelo menor;
- liberar worker;
- não reiniciar o mesmo job infinitamente.

## Artifact promotion

Worker escreve:

```text
tmp/<job_id>/
```

Rust valida:

- arquivos esperados;
- tamanho;
- decode;
- duration tolerance;
- JSON schema.

Só depois:

```text
tmp -> final
```

## Crash no meio da escrita

Ao startup, Rust pode procurar temporários antigos.

Política:

- identificar owner job;
- verificar se job estava concluído;
- limpar temporário órfão após confirmação/regras.

Nunca assumir temporário = resultado válido.

## Job journal

Persistir mudanças importantes de status.

MVP pode usar manifest atomically written.

Futuro:

SQLite WAL se biblioteca crescer.

## Cancellation

Soft:

```text
request cancel
```

Hard:

```text
terminate child
```

Após hard cancel:

- worker state invalid;
- restart worker;
- cleanup temp.

## FFmpeg

Cada processo:

- stdout/stderr capturados;
- exit code validado;
- timeout configurável;
- cancellation integrada.

## Audio engine

Erro de output device:

- pausar transport;
- preservar position;
- mostrar erro;
- permitir selecionar/reabrir device.

Não invalidar análise.

## UI

UI nunca assume sucesso a partir de progress=100%.

Somente estado Rust `completed` encerra job.

## Recovery on app restart

Ao iniciar:

1. ler manifests;
2. identificar jobs `running`;
3. convertê-los para `interrupted`;
4. verificar temporários;
5. preservar artefatos finalizados;
6. permitir retry.

## Warnings

Warnings são dados.

Exemplo:

```text
PIANO_STEM_LOW_CONFIDENCE
MODEL_FELL_BACK_TO_CPU
TEMPO_DOUBLE_TIME_AMBIGUITY
```

Não esconder warnings em logs apenas.

## Logging

Correlation fields:

```text
track_id
job_id
request_id
worker
stage
```

Rust e Python devem registrar esses IDs.
