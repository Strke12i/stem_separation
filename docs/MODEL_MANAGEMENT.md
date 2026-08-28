# Gerenciamento de modelos

## Ownership

Rust mantém o registry lógico.

Python workers conhecem adapters de engine.

## Model descriptor

```json
{
  "id": "separator.example",
  "engine": "audio-separator",
  "task": "four_stem",
  "version": "model-version",
  "source": "...",
  "license_note": "...",
  "sha256": "...",
  "size_bytes": 123,
  "installed": true,
  "relative_path": "models/..."
}
```

## Download

Não implementar download silencioso.

Fluxo:

```text
UI request
-> Rust validates model
-> explicit user action
-> download/install
-> checksum
-> mark installed
```

Worker não escolhe modelo remoto arbitrário.

## Offline

Se ausente:

```text
MODEL_NOT_INSTALLED
```

Não tentar rede.

## Compatibility

Registry pode declarar:

- CPU;
- CUDA;
- DirectML experimental;
- platform limitations;
- worker requirements.

## Loaded model

Worker reporta modelo ativo.

Rust pode solicitar unload.

## Cleanup

Remoção exige:

- modelo não em uso;
- confirmação;
- atualização de registry/cache.

## Licenças

Model weights têm licença própria.

Não assumir que licença da biblioteca = licença dos pesos.
