# UI desktop

## Stack

Svelte + TypeScript dentro do Tauri.

## Princípio

UI apresenta estado.

Não:

- acessa worker diretamente;
- executa shell;
- implementa mixer real;
- calcula BPM;
- carrega arquivos arbitrariamente sem comando Rust.

## Tela principal

### Library/Import

```text
[ Open audio file ]  [ Browse library ]

Library
[ search name, key or tag ]  [ Recent | Name ]
(funk 2) (practice 1)
- Track.mp3  3:12 · 120.0 BPM · C major   Stems: demucs-4   opened 3x
  (funk ×) [ add tag ]                                     [ Open ]
```

A aba Library é a primeira aba e a única acessível sem track carregada. Reabrir uma
track recarrega o áudio e as análises em cache pelos mesmos comandos de sempre; a UI
só conhece `trackId`, nunca paths.

Importar um áudio que já está na biblioteca (mesmo checksum) não cria um track novo:
Rust devolve o existente com `reused: true` e a UI avisa que os stems e as análises
salvos foram reaproveitados. Ao abrir ou importar um track, a UI consulta
`cached_separation`; se já houver stems (do próprio track ou de outra importação da
mesma música), a aba Stems mostra "Using cached stems" com o botão "Open mixer" sem
pedir nova separação. "Open mixer" fica desabilitado enquanto carrega e qualquer erro
aparece na própria aba Stems.

Ao selecionar:

- Rust abre dialog;
- Rust valida;
- UI recebe metadata.

## Analysis setup

```text
Separation:
[ 4 stems — recommended ]
[ 6 stems — experimental ]

Analysis:
[x] BPM / beats
[x] Key
[x] Chords
[x] Monophonic pitch
[ ] Polyphonic transcription
```

## Progress

UI recebe events do Rust.

Mostrar:

- queued (uma separação atrás de outra aparece como "Queued behind another separation" e pode ser cancelada);
- stage;
- determinate/indeterminate progress;
- model loading;
- warning;
- cancel.

## Workspace

Página de faixa:

```text
┌─────────────────────────────────────────┐
│ Track                         119.8 BPM │
│                               A minor   │
├─────────────────────────────────────────┤
│ waveform                                │
│ beats                                   │
│ chords                                  │
│ notes                                   │
├─────────────────────────────────────────┤
│ vocals [M] [S] volume                   │
│ drums  [M] [S] volume                   │
│ bass   [M] [S] volume                   │
│ other  [M] [S] volume                   │
└─────────────────────────────────────────┘
```

## Transport

Commands:

- play;
- pause;
- stop;
- seek;
- master volume.

State vem do Rust.

## Timeline

Frontend recebe:

- waveform peaks;
- beat timestamps;
- chord segments;
- note events.

Renderizar eficientemente.

Não receber PCM completo.

## Mixer

Cada alteração chama Rust.

UI pode usar optimistic visuals apenas se reconcile com state do host.

### Arranjo (timeline por stem)

A aba Mixer é uma timeline estilo DAW, uma linha por stem num eixo de tempo
compartilhado e com zoom:

```text
⏮ ⏪ ◀ ⏸ ⏹ ▶ ⏩   0:07.9 / 1:36.0  Bar 4 · 3  120.0 BPM     Beats/bar [4] Bar 1 on beat [1]  − ──○── + Fit  ☑ Follow  [Analyze all lanes]
            ┌ 1 ┬ 2 ┬ 3 ┬ 4 ┬ 5 ┬ 6 ┬ …            régua: compassos e batidas
Mix chords  │Am │F  │C  │G  │Am │…                 acordes da mix
● Vocals MS │ waveform + notas (piano roll da faixa)
● Drums  MS │ waveform  (percussão: sem pitch)
● Bass   MS │ waveform + notas (pYIN)
● Other  MS │ faixa de acordes + waveform + notas (Basic Pitch)
```

- A grade vem das batidas detectadas. Downbeats não são detectados, então o
  agrupamento (batidas por compasso e em qual batida começa o compasso 1) é
  escolha do usuário; padrão 4/4. Sem análise de ritmo a régua mostra segundos.
- Cada faixa mostra o waveform do próprio stem, as notas detectadas nele (janela
  de pitch própria da faixa) e, para stems harmônicos, uma faixa de acordes.
  Notas e acordes sob o playhead são contornados e nomeados no cabeçalho da faixa.
- O que cada stem sabe detectar: vocals e bass → notas monofônicas (pYIN);
  other, guitar e piano → notas polifônicas (Basic Pitch) e acordes; drums →
  nada de pitch. Cada detecção pode ser pedida por faixa ou toda de uma vez.
- Mouse: clicar ou arrastar em qualquer ponto move o playhead; Ctrl+roda dá zoom
  no cursor; Fit mostra a música inteira; Follow mantém o playhead à vista.
- Cabeçalhos mantêm mute, solo e volume. Selecionar uma faixa (clique nela) habilita
  os atalhos M e S.

Atalhos (com o foco na timeline): Espaço = play/pause; ←/→ = batida anterior/próxima
(Shift = compasso); Home/End = início/fim; +/− = zoom; F = seguir playhead;
M/S = mute/solo da faixa selecionada.

## Worker health

Settings/diagnostics:

```text
Desktop core: OK
Analysis worker: OK
AMT worker: not installed / stopped / OK
FFmpeg: OK
GPU: ...
```

## Errors

Mensagem principal amigável.

Details expansíveis:

```text
error code
stage
worker exit code
technical detail
```

## Offline

Indicador claro.

Quando offline:

- botões de download de modelo indisponíveis;
- análise existente continua.

## Model manager

Mostrar:

- model ID;
- purpose;
- size;
- installed;
- engine;
- license note;
- device compatibility.

## Performance

Não renderizar UI update para cada evento interno.

Rust pode throttle/coalesce position updates.

## Accessibility

- keyboard controls;
- labels;
- focus;
- contrast;
- não depender apenas de cor.

## Shortcuts

Implementados na timeline do Mixer (ver "Arranjo"): Space, M, S, Left/Right.
