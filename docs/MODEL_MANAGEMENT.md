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

### Instalador explícito de Demucs

No Windows, o atalho local é o script versionado abaixo. Ele é uma ação
explícita do usuário: usa rede somente durante sua execução e o worker volta a
operar offline depois que o bundle foi publicado.

```powershell
.\scripts\install-demucs.ps1 -Model demucs-4
```

Se a policy do PowerShell bloquear o script, execute somente para esta
invocacao:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\install-demucs.ps1 -Model demucs-4
```

O script baixa o conjunto de arquivos declarado pelo registry do
`audio-separator` para um diretório temporário, confere arquivos não vazios,
registra SHA-256 e tamanho em `.lma-bundle.json`, escreve
`.lma-model.json` e só então promove o diretório completo para
`workspace/models/<model-id>/`. Nunca sobrescreve bundle existente. Use:

```powershell
.\scripts\install-demucs.ps1 -Model demucs-4 -Plan
.\scripts\install-demucs.ps1 -Model demucs-4 -Verify
.\scripts\install-demucs.ps1 -Model demucs-6-experimental
```

O workspace respeita `LOCAL_MUSIC_ANALYZER_WORKSPACE`; sem essa variável, no
Windows usa `%LOCALAPPDATA%\LocalMusicAnalyzer\workspace`.

O worker de análise declara `onnxruntime` CPU explicitamente porque o
`audio-separator` importa esse runtime mesmo no caminho Demucs. O primeiro uso
de `uv` pode, portanto, sincronizar esse wheel antes de iniciar o download do
modelo.

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
