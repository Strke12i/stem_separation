# Decisões arquiteturais

## D-001 — Rust como host desktop

Status: accepted

Rust possui:

- lifecycle;
- audio;
- files;
- jobs;
- supervision;
- orchestration.

## D-002 — Python como compute sidecar

Status: accepted

Motivo:

Ecossistema científico maduro sem sacrificar estabilidade do host.

## D-003 — Process isolation em vez de embedded CPython

Status: accepted

Motivo:

- crash isolation;
- OOM isolation;
- dependency isolation;
- restart;
- packaging modular.

PyO3 permanece opcional.

## D-004 — Tauri 2

Status: accepted

Motivo:

Rust host + desktop packaging + sidecar support + webview UI.

## D-005 — Svelte/TypeScript para apresentação

Status: accepted

Motivo:

UI complexa é um domínio em que web tooling é produtivo.

Isso não reduz o papel de Rust porque:

- áudio;
- estado;
- commands;
- files;
- jobs;

permanecem Rust-side.

## D-006 — Player/mixer em Rust

Status: accepted

Não usar Web Audio como engine principal.

## D-007 — NDJSON stdio IPC

Status: accepted

Motivo:

- sem porta;
- simples;
- streamable;
- supervisionável.

## D-008 — Filesystem para artefatos grandes

Status: accepted

Não passar áudio via IPC.

## D-009 — python-audio-separator

Status: accepted

Engine inicial de stem separation.

## D-010 — librosa para MIR inicial

Status: accepted

BPM/beats/chroma/key/chords/pYIN.

## D-011 — Basic Pitch em worker separado

Status: accepted

Motivo:

isolar dependências e memória.

## D-012 — 4 stems default

Status: accepted

6 stems experimental.

## D-013 — Chords próprios no MVP

Status: accepted

`chroma + templates + smoothing`.

## D-014 — FFmpeg chamado por Rust

Status: accepted

Normalização e compatibilidade de mídia.

## D-015 — Symphonia + rodio para playback

Status: accepted

Rust mantém domínio do áudio interativo.

## D-016 — JSON manifests primeiro

Status: accepted

SQLite apenas quando biblioteca justificar.

## D-017 — Model downloads explícitos

Status: accepted

Offline mode nunca baixa.

## D-018 — No downloader in core

Status: accepted

Importadores são plugins futuros.

## D-019 — Performance measured

Status: accepted

Benchmark é requisito para otimização.

## D-020 — PyO3 como laboratório, não arquitetura principal

Status: accepted

O projeto pode explorar Rust/Python bindings depois, especialmente Python → Rust extensions.

## D-021 — rusqlite bundled como índice derivado da biblioteca

Status: accepted

A biblioteca (Fase 12) usa `rusqlite` com SQLite embutido, síncrono, alinhado ao
resto do código (I/O de arquivo inline, sem padrão async de banco). O banco é um
cache derivado: manifests continuam sendo a fonte de verdade (D-016) e tags/histórico
ficam em `track-*/library.json`, de modo que apagar `index.sqlite3` nunca perde
dados. Não há framework de migrações: `PRAGMA user_version` diferente da constante
do código apaga e reconstrói o banco. Busca usa `LIKE` com escape; FTS5 fica fora
até haver escala que o justifique.

## D-022 — Otimizações só entram com medição, e o gargalo real vem primeiro

Status: accepted

Resultado da Fase 13 aplicando D-019 e D-020. Antes de escrever código nativo, o
custo de cada etapa foi medido numa faixa real. O maior ganho veio de vetorizar
`chord_frames` e `segment` em NumPy (~45× e ~8×, sem toolchain nova). Peaks de
waveform (~9% do carregamento, dominado pelo decode) e uma extensão PyO3 para
`smooth` (10×, mas ≈80 ms por faixa) não justificaram promoção: a primeira ficou
mais lenta com auto-vetorização e a segunda exigiria Rust no empacotamento do
sidecar e um wheel por versão de Python e plataforma. Ambos ficam como
experimentos reproduzíveis (`waveform_benchmark`, `labs/analyzer-native`).
Reavaliar a extensão PyO3 apenas se uma etapa de Python passar a custar uma
fração relevante do tempo de análise.

## D-023 — Cancelar um job descarta o processo do worker

Status: accepted

Cancelar uma separação só marcava uma flag: o Demucs seguia por minutos e o
resultado era jogado fora. Interromper a inferência dentro do Python não é
confiável (não há ponto de cancelamento no modelo), então cancelar passa a
abandonar a requisição e matar o processo do worker; o próximo pedido inicia um
processo novo. Isso mantém a invariante de que o Rust é dono do ciclo de vida do
job e reaproveita o descarte que já existia para workers dessincronizados. O
custo é recarregar o modelo na próxima separação, e o cancelamento deliberado
não conta para o limite de reinícios automáticos. `restart_worker` cancela o job
em andamento em vez de esperá-lo, e o "Check engine" responde "busy" enquanto um
job usa o worker.



## D-024 — Análise por stem reaproveita os workers; a UI de arranjo só apresenta

Status: accepted

Para mostrar notas e acordes por faixa, harmonia e Basic Pitch passaram a aceitar
um stem opcional. Os workers Python não mudaram: eles já analisam qualquer arquivo
do workspace que Rust indicar, e Rust continua dono de localizar o stem, da chave
de cache e da persistência (D-001, invariantes de `ARCHITECTURE.md`). Resultados de
stem vivem ao lado dos da mix, nunca no lugar deles. Que análise cabe a cada stem é
regra da apresentação (`laneCapabilities`): pYIN só faz sentido monofônico, então
bass e vocals; other, guitar e piano recebem Basic Pitch e acordes; drums não tem
pitch. O agrupamento em compassos é escolha do usuário porque downbeats e fórmula
de compasso ainda não são detectados (Backlog). A lógica de tempo, grade e
"o que soa agora" fica em `arrangement.ts`, pura e testada, separada do componente.

## D-025 — Uma música é um track; stems são reaproveitados por conteúdo

Status: accepted

Cada importação criava um `track-<uuid>` novo, então o cache de stems, que vive no
workspace do track, nunca acertava entre importações e a mesma música era
separada de novo (cerca de 9 minutos de CPU cada vez, com 180 MB de stems por cópia).
A identidade passa a ser o checksum do áudio de origem: importar de novo devolve o
track existente, e stems já separados para o mesmo áudio são adotados por hard link
(com cópia como plano B) pelo caminho normal de promoção, com validação do WAV e
registro no manifest sob o lock do track. Um conjunto no disco que o manifest não
lista é registrado de novo em vez de tratado como cache válido e depois impossível de
abrir. A checagem do cache acontece antes da fila, do modelo instalado e do checksum
do bundle: stems que já existem não dependem de nada disso. Consequência aceita:
duplicatas antigas continuam na biblioteca até o usuário removê-las; o sistema só
deixa de criar novas e de reprocessar por causa delas.

## D-026 — Dependências otimizadas também no perfil dev

Status: accepted

Medido (D-019/D-022): preparar o mix de 4 stems de 4:17 no build dev levava 5,0 s por
causa da decodificação em `opt-level 0`, sem nenhum indicador na UI. Com
`[profile.dev.package."*"] opt-level = 3` leva 0,76 s. Só as dependências mudam; o
código do projeto continua depurável. O custo é uma primeira compilação mais longa.

## D-027 — Análises de música inteira têm timeout próprio; pYIN a 22,05 kHz

Status: accepted

O timeout do launch do worker (8 s em desenvolvimento) é para handshake e checagem de
saúde, mas rhythm, harmony e pitch também o usavam. Analisar um stem de 4 minutos leva
mais do que isso, então o pitch por faixa expirava e as faixas do arranjo ficavam sem
notas. Essas três análises passam a ter `ANALYSIS_REQUEST_TIMEOUT` (10 min, como o AMT),
no mesmo padrão do timeout da separação.

Com o timeout resolvido apareceu o defeito por trás: o worker fechava cada nota em
`tempo do último quadro + hop`, e em notas coladas isso passa do início da seguinte por
~1e-15 s. O Rust rejeitava o resultado inteiro como inválido. A nota agora termina
exatamente onde a seguinte começa. Medido (D-019/D-022) num baixo de 4:17: reamostrar
para 22,05 kHz e usar resolução de 0,15 semitom leva 28 s em vez de 112 s, sem o aviso
do librosa sobre o quadro de 2048 amostras ser curto para o E1, e mantém 94 % do tempo
sonoro na mesma nota da saída anterior (resolução 0,25: 6 s, 86 %, perde notas). Essa
concordância é contra a saída anterior, não contra uma verdade de referência.

