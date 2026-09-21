# Contratos de dados

## Ownership

Rust define os tipos de domínio canônicos.

Os schemas compartilhados devem derivar dessas definições ou de schema versionado.

## Track manifest

```json
{
  "schema_version": 1,
  "track_id": "track-...",
  "source": {
    "sha256": "...",
    "original_name": "song.mp3",
    "duration_seconds": 213.4,
    "sample_rate": 44100,
    "channels": 2
  },
  "analysis": {},
  "artifacts": [],
  "jobs": []
}
```

## Artifact

```json
{
  "artifact_id": "artifact-...",
  "kind": "stem",
  "stem": "bass",
  "relative_path": "stems/four_stem/bass.wav",
  "sha256": "...",
  "created_by": {
    "stage": "separation",
    "engine": "audio-separator",
    "engine_version": "...",
    "model": "..."
  }
}
```

## Stage

```json
{
  "name": "separation",
  "status": "completed",
  "cache_hit": false,
  "started_at": "...",
  "finished_at": "...",
  "warnings": []
}
```

Status válidos:

```text
pending
queued
running
completed
completed_with_warnings
failed
cancelled
skipped
```

## Rhythm

```json
{
  "bpm": 119.8,
  "beat_times": [0.51, 1.01, 1.51],
  "algorithm": "librosa.beat",
  "stability_score": 0.92
}
```

Não tratar `stability_score` como probabilidade.

## Key

```json
{
  "tonic": "A",
  "mode": "minor",
  "label": "A minor",
  "score": 0.81,
  "second_best": "C major",
  "margin": 0.08
}
```

## Chord

```json
{
  "start": 12.0,
  "end": 14.0,
  "label": "Am",
  "root": "A",
  "quality": "minor",
  "score": 0.76,
  "beat_aligned": false
}
```

Sem acorde:

```text
N
```

## Note

```json
{
  "start": 23.48,
  "end": 23.92,
  "midi": 45,
  "note": "A2",
  "confidence": 0.87,
  "stem": "bass",
  "engine": "pyin"
}
```

## Library sidecar

`track-*/library.json` guarda fatos exclusivos da biblioteca que não existem em
nenhum manifest. Pertence apenas ao `LibraryService`; é a fonte de verdade de tags
e histórico, e o SQLite o espelha.

```json
{
  "schema_version": 1,
  "tags": ["funk", "practice"],
  "last_opened_at": "2026-09-19T12:04:11Z",
  "open_count": 7
}
```

Tags são normalizadas (trim, minúsculas, espaços colapsados, 1 a 32 caracteres, sem
caracteres de controle). Arquivo ausente ou ilegível equivale a "sem tags, nunca
aberto". `LibraryEntry` devolvido à UI nunca carrega path.

## Análises por stem

Harmonia e transcrição Basic Pitch também rodam sobre um stem separado (o pitch
monofônico já era por stem). O resultado nunca substitui o da mix:

| Análise | Mix | Stem |
| ------- | --- | ---- |
| harmony | `analysis.harmony`, `analysis/harmony/<sig>/harmony.json`, stage `harmony` | `analysis.stem_harmony.<stem>`, `analysis/harmony/<stem>/<sig>/harmony.json`, stage `harmony:<stem>` |
| amt | `analysis.amt`, `analysis/amt/<key>/`, stage `amt` | `analysis.stem_amt.<stem>`, `analysis/amt/<stem>/<key>/` (com `transcription.mid` próprio), stage `amt:<stem>` |
| pitch | — | `analysis.pitch.<stem>`, stage `pitch:<stem>` |

Os relatórios de stem carregam `stem`; o da mix não. A chave de cache do stem inclui
o nome e o checksum do artefato do stem, então uma nova separação invalida o cache.
A chave da mix é a mesma de antes do suporte a stems. Nomes de stem são validados
contra a lista fixa (`vocals`, `drums`, `bass`, `other`, `guitar`, `piano`) antes de
virarem parte de um path.

## Paths

Persistência usa path relativo ao track workspace.

Não persistir path absoluto em manifest portable.

## Numeric rules

- tempo musical: seconds as float;
- frequency: Hz;
- pitch: MIDI number;
- tempo: BPM;
- sample rate: Hz integer.

JSON não pode possuir:

- NaN;
- Infinity.

## Timestamps

ISO-8601 com timezone.

## Atomicity

Manifest raiz:

```text
manifest.json.tmp
-> rename
-> manifest.json
```

## Schema validation

Rust valida toda resposta Python antes de promover artefatos.

Python deve validar requests antes de executar.
