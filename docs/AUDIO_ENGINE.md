# Engine de áudio em Rust

## Objetivo

Fornecer playback responsivo e sincronizado sem delegar o clock do áudio ao WebView.

## Responsabilidades

```text
AudioEngine
├── output device
├── transport
├── sources
├── stem mixer
├── seek
├── mute
├── solo
├── gain
├── master gain
└── playback state
```

## Clock

A engine Rust é a fonte de verdade de:

```text
current_position
playing
paused
duration
```

A UI recebe snapshots/eventos.

Não usar timer JavaScript como clock de áudio.

## Playback de stems

Ao tocar stems separados:

- todos devem iniciar da mesma posição;
- compartilhar operação de seek;
- sofrer drift mínimo;
- solo/mute não deve alterar posição.

Arquitetura conceitual:

```text
Transport Clock
     |
     +--> StemSource(vocals) -- gain --\
     +--> StemSource(drums)  -- gain ---+--> mixer --> device
     +--> StemSource(bass)   -- gain ---+
     +--> StemSource(other)  -- gain --/
```

## Sincronização

Não criar quatro players independentes com clocks distintos.

Preferir:

- mixer comum;
- fontes alinhadas;
- seek coordenado.

## UI updates

Não emitir posição a cada sample.

Faixa inicial:

```text
20–60 UI updates / second
```

Configurar conforme medição.

Audio thread não espera a UI.

## Decoding

### Symphonia

Preferir para:

- WAV;
- FLAC;
- MP3/AAC quando configuração/licença suportar;
- leitura de stems já normalizados.

### FFmpeg

Usar para ingestão/conversão universal.

## Formato interno de stems

Preferir WAV PCM/float adequado ao pipeline.

Para playback, evitar re-decode desnecessário.

Não carregar todos os stems inteiros em RAM para músicas longas.

## Buffering

Usar buffers limitados.

Objetivos:

- evitar underrun;
- evitar RAM excessiva;
- manter seek razoável.

## Waveform

Gerar peaks em Rust como artefato pequeno.

Exemplo:

```json
{
  "sample_windows": 4000,
  "min": [],
  "max": []
}
```

A UI recebe peaks, não samples brutos.

Pode gerar níveis:

```text
overview
medium
detailed
```

somente se necessário.

## Audio device change

Tratar:

- device desconectado;
- default device mudou;
- sample rate não suportado.

Erro do dispositivo não deve apagar a análise.

## Mixer

Cada stem:

```text
gain: 0.0..n
mute: bool
solo: bool
```

Semântica de solo:

Se qualquer stem estiver solo, tocar apenas stems solo e não mutados.

## Clipping

Mixer deve evitar clipping desnecessário.

Não aplicar limiter complexo no MVP.

Inicialmente:

- somar com headroom;
- permitir master gain;
- medir peak.

## Threading

Audio callback:

- sem IO;
- sem alocações grandes;
- sem locks longos;
- sem chamadas Python;
- sem JSON;
- sem logging pesado.

## Futuro

- loop A/B;
- metronome;
- time stretch;
- pitch shift;
- output device selection;
- audio latency settings.

Esses recursos permanecem Rust-side.
