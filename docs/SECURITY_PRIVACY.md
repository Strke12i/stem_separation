# Segurança e privacidade

## Regra

O áudio permanece local.

## Tauri capabilities

Conceder somente permissões necessárias.

Frontend não deve possuir shell genérico.

Spawning de sidecar acontece pelo Rust.

## Sidecars

Somente executáveis conhecidos/configurados.

Não permitir:

```text
spawn arbitrary executable
```

via frontend.

## Python worker

Não aceitar:

- `eval`;
- código Python;
- shell command;
- module path arbitrário;
- pickle arbitrário do usuário;
- checkpoint não registrado.

## Model registry

Modelos são identificados por IDs conhecidos.

Quando possível armazenar:

- source;
- checksum;
- license;
- local path;
- engine compatibility.

## Paths

Rust resolve e valida paths.

Python recebe workspace interno.

## Original

Read-only logic.

Nenhuma etapa sobrescreve arquivo de entrada.

## Network

Development pode acessar rede para instalar deps/modelos.

Runtime offline mode:

- bloqueia downloads;
- bloqueia update checks;
- não inicializa HTTP client de features opcionais.

## Local UI

Tauri não expõe servidor web público.

## FFmpeg

Usar structured arguments.

Não concatenar comando.

## Logs

Não registrar áudio.

Evitar path absoluto quando não necessário.

## Temporary data

Armazenar em diretório da aplicação/workspace.

Cleanup definido.

## Model files

Modelos podem usar formatos com risco de deserialização.

Preferir:

- ONNX;
- safetensors;

quando engine/modelo equivalentes existirem.

Nunca carregar checkpoint arbitrário fornecido como "modelo" pelo usuário no MVP.

## Crash reports

Sem envio automático.

Logs locais podem ser exportados manualmente.

## Updates

Não implementar auto-updater na primeira versão.

## Importadores futuros

Download de mídia não faz parte do core.

Qualquer importador deve respeitar:

- autorização do usuário;
- termos da fonte;
- direitos sobre o conteúdo.
