# Status

## Estado atual

As fases 0 a 11 estão implementadas. O aplicativo possui a fronteira
Rust/Python supervisionada, ingestão local, playback do original em Rust e
separação local de stems por sidecar Python, com mixer sincronizado em Rust.
Também há análise local de BPM, beats, tonalidade, acordes e notas
monofônicas em stems de baixo ou voz, além de transcrição polifônica local por
Basic Pitch/ONNX em worker isolado.

## Fases

- [x] Fase 0 — Workspace e boundary
- [x] Fase 1 — Ingestão Rust
- [x] Fase 2 — Audio engine Rust
- [x] Fase 3 — Stem separation Python
- [x] Fase 4 — Stem mixer Rust
- [x] Fase 5 — Rhythm Python
- [x] Fase 6 — Harmony Python
- [x] Fase 7 — Pitch monofônico
- [x] Fase 8 — AMT worker
- [x] Fase 9 — Resilience hardening
- [x] Fase 10 — Performance
- [x] Fase 11 — Packaging
- [x] Fase 12 — Library
- [x] Fase 13 — Advanced Rust

## Registro

### Fase 0 — Workspace e boundary

Date: 2026-08-27
Commit: uncommitted
Rust changes: Cargo workspace; domain/protocol crates; Tauri 2 shell; NDJSON sidecar supervisor; doctor service; tracing with worker/request correlation.
Python changes: Python 3.11 `uv` project with the minimal analysis worker. Stdout is NDJSON-only and logs go to stderr.
Frontend changes: Svelte/TypeScript diagnostics screen with desktop/worker/protocol status and worker restart.
Tests: Rust protocol tests; six fake-worker supervisor tests; one real Rust-to-Python boundary harness; Python protocol tests.
Benchmarks: Not applicable.
Known limitations: Development requires the `uv` environment to be synced. Sidecar packaging is deferred to Phase 11.
Next: Fase 1 — Ingestão Rust.

### Fase 1 — Ingestão Rust

Date: 2026-08-27
Commit: uncommitted
Rust changes: Validated local import; SHA-256 hashing; structured `ffprobe` metadata collection; staging workspace; atomic copies, WAV normalization and manifest publication; manifest/domain types; native Rust file dialog.
Python changes: None.
Frontend changes: Native-file import action plus imported-track metadata and friendly error rendering.
Tests: Four deterministic ingest tests using fake FFmpeg tools and an opt-in real FFmpeg smoke test that generates, imports and normalizes a synthetic MP3.
Benchmarks: Not applicable.
Known limitations: FFmpeg is a development system dependency. Its release packaging strategy remains deferred to Phase 11.
Next: Fase 2 — Audio engine Rust.

### Fase 2 — Audio engine Rust

Date: 2026-08-28
Commit: uncommitted
Rust changes: New `analyzer-audio` crate with Rodio/CPAL output, Symphonia decoding, Rust-owned transport, playback controls, seek, master volume, output-device reopen action and streaming waveform peaks.
Python changes: None.
Frontend changes: Original-track transport, waveform canvas, seek and master-volume controls, and output-device recovery action.
Tests: Unit tests for transport state and waveform reduction; opt-in real playback test using `files/track_test.mp3` passed for load, waveform, seek, play, pause and stop.
Benchmarks: Not applicable; no performance claim was made.
Known limitations: Waveform peaks live in the engine snapshot in this phase; persistent waveform artifact caching is deferred. Output-device hotplug detection remains a recovery action rather than automatic monitoring.
Next: Fase 3 — Stem separation Python.

### Fase 3 — Stem separation Python

Date: 2026-08-28
Commit: uncommitted
Rust changes: Registry local para `demucs-4` e `demucs-6-experimental`; job
serializado; chave de cache determinística; diretório temporário por job;
validação de paths relativos e cabeçalho WAV; hashes e registro de artefatos
no manifest; promoção atômica somente após validação; proteção contra promoção
após cancelamento solicitado.
Python changes: Dependência `audio-separator` 0.39.x; adaptador offline para
`htdemucs.yaml` (vocals, drums, bass, other) e `htdemucs_6s.yaml`
(acrescenta guitar e piano); eventos NDJSON de progresso; validação estrita de
workspace e marcador de instalação local. A rede é bloqueada durante o load e
inferência do motor.
Frontend changes: Lista de perfis, estado de modelo local, ação de separação e
cancelamento seguro após o passo atual do modelo.
Tests: Teste Rust de ponta a ponta com sidecar fake (gera WAVs, valida,
promove e acerta cache); testes Python de protocolo/progresso; workspace Rust,
ruff, mypy, pytest, svelte-check e build passaram.
Benchmarks: Não aplicável; não há promessa de desempenho.
Known limitations: Nenhum bundle de pesos foi instalado automaticamente. Para
executar uma separação real, o usuário deve instalar deliberadamente o bundle
do modelo em `workspace/models/<model-id>/`, incluindo o arquivo YAML, todos
os pesos referidos e `.lma-model.json` com `model_id` e `model_filename`. O
cancelamento impede promoção de artefatos e limpa o temporário; a interrupção
física da inferência depende do retorno seguro do `audio-separator`.
Next: Fase 4 — Stem mixer Rust.

### Fase 4 — Stem mixer Rust

Date: 2026-08-28
Commit: uncommitted
Rust changes: A engine de áudio agora aceita fontes de stems, abre todas no
mesmo dispositivo, usa um único transporte lógico, reconstrói todas as fontes
na mesma posição em seek e expõe gain, mute, solo e master gain. O ganho efetivo
divide o headroom entre stems audíveis para reduzir clipping no MVP.
Frontend changes: Ação para carregar stems gerados no mixer e controles por
stem de volume, mute e solo; transporte existente passa a controlar o mix.
Tests: Teste unitário para as semânticas de headroom/mute/solo; workspace Rust,
clippy, testes, svelte-check e build passaram.
Known limitations: A sincronização é coordenada pelo transporte Rust e fontes
reabertas no mesmo offset; uma medição de drift com stems reais e device de
saída será adicionada na fase de performance. O teste de reprodução real segue
opt-in por depender de dispositivo de áudio.
Next: Fase 5 — Rhythm Python.

### Fase 5 — Rhythm Python

Date: 2026-08-28
Commit: uncommitted
Rust changes: Novo serviço de ritmo com assinatura de cache baseada no hash da
fonte, validação de BPM, estabilidade, ordem e limites temporais dos beats;
persistência atômica em `analysis/rhythm/<cache>/rhythm.json`; artefato e
resumo registrados no manifest.
Python changes: `librosa` passou a ser dependência explícita. O sidecar
implementa `analyze_rhythm`, calcula onset envelope, tempo e beats a partir do
WAV normalizado, gera eventos de progresso e retorna apenas o resultado JSON
pequeno pelo IPC.
Frontend changes: Ação de análise, exibição de BPM/estabilidade/cache e
timeline leve com marcadores de batida.
Tests: Click track sintético a 120 BPM; teste de estabilidade; integração
Rust-sidecar fake validando persistência e cache; workspace Rust, clippy,
ruff, mypy, pytest, svelte-check e build passaram.
Known limitations: Estabilidade é uma medida de regularidade dos intervalos,
não probabilidade ou confiança musical. Não há downbeats nem métrica nesta
fase.
Next: Fase 6 — Harmony Python.

### Fase 6 — Harmony Python

Date: 2026-08-31
Commit: uncommitted
Rust changes: Serviço de harmonia com cache dependente da fonte e dos beats,
validação de key/chords e persistência atômica em `analysis/harmony/<cache>`.
Python changes: Chroma CQT, perfis maior/menor para tonalidade, templates de
tríades maiores/menores e `N`, suavização de frames e alinhamento opcional do
início dos segmentos a beats próximos.
Frontend changes: Ação para análise, key detectada e timeline de acordes.
Tests: Tríades sintéticas C maior e A menor; ruff, mypy e pytest passaram;
Rust clippy, checks e build da interface passaram.
Known limitations: Vocabulário inicial limitado a tríades maior/menor e `N`;
não há inversões, sétimas, modulações ou correção manual nesta fase.
Next: Fase 7 — Pitch monofônico.

### Fase 7 — Pitch monofônico

Date: 2026-08-31
Commit: uncommitted
Rust changes: Novo serviço de pitch por stem, com seleção segura dos stems
`bass` e `vocals`, cache determinístico baseado nos hashes da fonte e do stem,
validação de notas MIDI e persistência atômica em
`analysis/pitch/<stem>/<cache>/notes.json`. Os resultados e artefatos ficam no
manifest por stem; o IPC agora expõe `analyze_pitch`.
Python changes: O sidecar usa `librosa.pyin`, limites de frequência próprios
para baixo e voz e segmenta frames vozeados contíguos por MIDI arredondado,
emitindo início, fim, nota, MIDI e confiança.
Frontend changes: Ações de análise para baixo e voz, com feedback para stems
ausentes e timelines leves de notas detectadas.
Tests: Frequências sintéticas de 110, 220 e 440 Hz; descarte de trechos curtos
ou não vozeados; integração Rust-sidecar fake cobrindo validação, persistência
e cache. Workspace Rust, clippy, ruff, mypy, pytest, svelte-check e build
passaram.
Benchmarks: Não aplicável; não há alegação de desempenho nesta fase.
Known limitations: A análise exige stems já gerados localmente e é adequada a
linhas predominantemente monofônicas; vibrato, notas sobrepostas, harmonias
vocais e instrumentos polifônicos não são transcritos de modo confiável.
Next: Fase 8 — AMT worker.

### Fase 8 — AMT worker

Date: 2026-08-31
Commit: uncommitted
Rust changes: Novo `AmtService` isolado do analysis-worker, iniciado apenas
sob demanda com `uv run --offline`; request `transcribe`, validação de eventos
MIDI e cabeçalho, cache determinístico, promoção atômica de `.mid` e JSON de
notas em `analysis/amt/<cache>/`.
Python changes: Novo projeto `python/amt-worker`, separado por lockfile e
ambiente CPython 3.10, com Basic Pitch 0.4 + ONNX Runtime no Windows. O adapter
preserva stdout como NDJSON e transforma eventos em notas MIDI; o worker nunca
baixa runtime/modelo durante o app.
Frontend changes: Ação de transcrição do original e piano roll leve para notas
MIDI retornadas.
Tests: Rust workspace e clippy; ruff, mypy e pytest do novo worker; svelte-check
e build passaram.
Validation: O modelo distribuído foi carregado localmente pelo backend ONNX no
Windows. A dependência `setuptools<81` permanece fixada enquanto `resampy`
precisar de `pkg_resources`.
Known limitations: AMT é mais confiável para um instrumento por vez; notas e
artefatos MIDI devem ser tratados como sugestão musical, não partitura final.
Next: Fase 9 — Resilience hardening.

### Fase 9 — Resilience hardening

Date: 2026-08-31
Rust changes: O supervisor do analysis-worker agora descarta o processo ao
receber timeout, erro de IPC, resposta ausente ou evento com `job_id`
incompatível. Como o processo é iniciado com `kill_on_drop`, o próximo comando
cria um sidecar limpo em vez de reutilizar stdout potencialmente dessicronizado.
Na inicialização, o host também limpa apenas diretórios de staging/job órfãos
com mais de 24 horas, tanto em `.tmp/` de ingestão quanto em `track-*/tmp/`.
Workspaces publicados, manifests e fontes originais não entram nessa limpeza.
Há no máximo três reinícios automáticos consecutivos; uma solicitação bem
sucedida zera o contador e a ação explícita de restart também o restabelece.
Erros `MODEL_OUT_OF_MEMORY` recebem classificação amigável. No startup, jobs e
stages persistidos como em andamento são marcados como falhos/interrompidos
com aviso recuperável, preservando todos os artefatos existentes.
Diagnosis: Os manifests locais inspecionados não possuem artefatos de stem;
portanto o erro de pitch em `bass` é o pré-requisito esperado de separação,
não uma falha do worker. O desvio de `job_id` em Rhythm/Harmony podia contaminar
as solicitações seguintes porque o supervisor antigo era preservado após falha.
Tests: A suíte já cobre handshake inválido, crash e timeout do worker. Foi
adicionado teste de limpeza seletiva de staging/job que prova a preservação de
dados publicados. `cargo fmt`, clippy, testes Rust, svelte-check e build foram
executados após a alteração.
Known limitations: A classificação de OOM depende do código estruturado do
worker; telemetria de memória e retry por tipo de backend são trabalho futuro.
Next: Fase 10 — Performance.

### Fase 10 — Performance

Date: 2026-09-06
Commit: uncommitted
Rust changes: `ResourceScheduler` centraliza um único permit para inferências
potencialmente GPU-heavy, compartilhado entre separação e AMT. Workers de
análise já permanecem serializados pelo manager existente. A inspeção dos stems
secundários no mixer agora lê somente metadados de duração, sem gerar waveform
nem decodificar todos os samples. Foi adicionado benchmark reprodutível em
`cargo run -p analyzer-audio --example waveform_benchmark`.
Frontend changes: O piano roll limita a 1.200 os elementos DOM visíveis e faz
amostragem determinística quando a transcrição contém mais notas, mantendo a
contagem total acessível.
Benchmarks: B4 waveform, Windows desenvolvimento, áudio sintético estéreo de
300 s / 44.1 kHz, 1.200 janelas: 591 ms em build debug. O valor é baseline de
regressão, não comparação de melhoria ou promessa de desempenho.
Tests: Workspace Rust, clippy, Svelte check e build foram executados; testes
existentes cobrem a redução de waveform e as semânticas de mixer.
Known limitations: O gate de GPU é conservador e não identifica provider por
runtime; telemetria de memória e benchmarks de separação/AMT com modelos reais
dependem dos bundles locais instalados.
Next: Fase 11 — Packaging.

### Fase 11 — Packaging

Date: 2026-09-06
Commit: uncommitted
Rust/Tauri changes: A resolução dos workers prioriza os sidecars empacotados
ao lado do executável, mantendo os overrides explícitos por variável de
ambiente para desenvolvimento. O timeout de handshake é maior para binários
PyInstaller. A configuração normal do Tauri permanece própria para dev/test;
`tauri.release.conf.json` habilita o bundle e declara os dois `externalBin`
somente no fluxo de release.
Python changes: Cada worker possui grupo `package` bloqueado com PyInstaller.
`build-sidecars.ps1` cria um ambiente isolado `.package-venv`, evitando alterar
o virtualenv em uso pelo desenvolvimento, e gera executáveis `onefile` com o
sufixo do target. O script opcionalmente copia `ffmpeg.exe` e `ffprobe.exe` de
um diretório informado.
Release tooling: `package-desktop.ps1` encadeia build dos dois sidecars, cópia
explícita de FFmpeg, smoke de handshake/ffprobe e `tauri build` com a
configuração de release. `smoke-package.ps1` falha antes do bundle se qualquer
sidecar ou ferramenta de mídia estiver ausente. Modelos continuam fora do
instalador e não são baixados no runtime.
Tests: `cargo fmt`, `cargo check -p local-music-analyzer-desktop` e todos os
testes desse pacote passaram. `svelte-check` terminou com zero diagnósticos; o
Vite ainda registra a ACL conhecida do diretório-pai do OneDrive ao carregar a
configuração. O smoke final requer artefatos gerados e um diretório de FFmpeg,
portanto não tenta baixar nem inventar binários de release.
Known limitations: O primeiro executável onefile do worker de análise agrega
bibliotecas ML grandes e pode levar vários minutos para ser produzido em uma
máquina Windows. A release pública ainda precisa de revisão de licença da build
de FFmpeg, medição do tamanho final e assinatura de código.
Next: Fase 12 — Library.

### Fase 12 — Library

Date: 2026-09-19
Rust changes: Novo `LibraryService` (`src/library.rs`) sobre `rusqlite` com SQLite
embutido (`bundled`). O índice em `<workspace_root>/library/index.sqlite3` é um
cache derivado descartável: `reconcile()` varre `track-*/manifest.json`, pula
manifests inalterados (gate por mtime + tamanho), remove tracks cujo diretório
sumiu e `refresh(track_id)` atualiza uma única linha após cada operação. Um
arquivo corrompido ou com `user_version` diferente é apagado e recriado vazio.
Tags e histórico de abertura vivem em um sidecar `track-*/library.json`
(escrito com temp → fsync → rename), não no banco; o SQLite apenas os espelha,
então apagar o banco nunca perde dados. `search()` combina texto (nome, key,
tags; `%`, `_` e `\` escapados) e interseção de tags, com ordenação por recente
ou nome. `IngestService` ganhou `workspace_root()`. Sete comandos Tauri
(`library_list`, `library_search`, `library_tags`, `library_add_tag`,
`library_remove_tag`, `library_open_track`, `library_rebuild`); os comandos de
import, separação e análises chamam `library.refresh` ao terminar com sucesso.
Python changes: None.
Frontend changes: Nova aba Library (primeira aba, acessível sem track
carregada) com busca, ordenação, chips de tags, adicionar/remover tags,
contagem de aberturas, reindexação manual e reabertura de tracks via
`adoptTrack()`, extraído de `importTrack()`.
Tests: `tests/library.rs` (indexação, reindexação por mudança de manifest,
esquecimento de track apagada, refresh unitário, recuperação de banco
corrompido com tags preservadas, tags/histórico após `rebuild()`, ordenação
por recente, contagem de tags, busca por nome/key/tag, interseção de tags,
escape de curingas) e testes unitários de schema, normalização de tags e
`summarize`. Workspace Rust fmt/clippy/test, `svelte-check` e `vite build`
passaram.
Known limitations: A aba Library não foi exercitada no app Tauri em execução
(janela nativa); a verificação foi por tipos, build e leitura da superfície de
comandos. O índice não observa o filesystem em tempo real: mudanças externas
são vistas no próximo `list()` (abrir a aba) ou no startup. Não há timeline de
histórico, apenas `last_opened_at` e `open_count`.
Next: Fase 13 — Advanced Rust.

### Fase 13 — Advanced Rust

Date: 2026-09-19
Commit: 892a890, 1f26df6, a76024b
Rust changes: Nenhuma mudança de produção. `waveform_benchmark` agora roda em
release sobre um sinal musical sintético e, com um caminho de áudio, mede
decode isolado e decode + peaks (o que `AudioEngine` faz ao carregar). Duas
variantes de vetorização (8 lanes de min/max com teste `is_finite` por amostra,
e lanes com verificação por soma de NaN/inf) foram comparadas ao loop escalar
atual e conferidas como idênticas.
Python changes: `chord_frames` passou a pontuar os 24 templates com um produto
matricial (em blocos de 8192 quadros) e `segment()` ordena os beats uma vez e usa
`bisect` em `nearest_beat()`. Empates exatos (ex.: quadro com uma só nota) mantêm
a regra antiga de maior rótulo, agora com tolerância de 1e-9 para não depender do
arredondamento do BLAS. Um novo laboratório `labs/analyzer-native` (crate
maturin/PyO3 fora do workspace Cargo) implementa `smooth_labels`.
Frontend changes: None.
Benchmarks (Windows de desenvolvimento; faixa real de 257 s, 22 161 quadros):
`chord_frames` 1228 ms → 27 ms (~45×) e `segment` 242 ms → 28 ms (~8×), com
rótulos e os 1943 segmentos idênticos à implementação anterior (maior delta de
score 8,5e-8 por promoção de float32 para float64). Peaks de waveform: decode
≈535 ms e a redução ≈9% do caminho de carga; as duas variantes de lanes ficaram
0,85× (mais lentas) que o loop atual. PyO3 `smooth_labels`: 88 ms → 8,8 ms
(10×), mas ≈80 ms por faixa contra 5–8 s de `chroma_cqt`.
Tests: `test_harmony_equivalence.py` congela as implementações antigas e compara
com as novas em entrada aleatória, propensa a empates e silenciosa. Python
ruff/mypy/pytest, Rust fmt/clippy/test passaram.
Decisions: D-022 — só o que foi medido entra em produção. Entrou: vetorização
NumPy. Não entrou: extração de peaks vetorizada e a extensão PyO3.
Known limitations: Resampling próprio, primitivas de DSP em Rust e efeitos de
áudio não foram implementados: não há consumidor no produto (o rodio já faz o
resampling na reprodução) e D-019 exige uma medição que justifique o código.
`smooth` continua em Python (≈60–90 ms por faixa). Os números vêm de uma única
máquina Windows e de uma única faixa.
Next: Hardening dos itens adiados da auditoria; depois o Backlog.

### Hardening — itens adiados da auditoria

Date: 2026-09-19
Commit: ffd9962, bda99c3, 6c8530b, 3b856de, f134ca8, 8bc24c2, 6d6a488
Rust changes: `WorkerManager::cancel_job` cancela de verdade: a requisição em
andamento disputa um sinal de cancelamento e o processo do worker é descartado
(`kill_on_drop`); o próximo pedido sobe um worker novo e um cancelamento
deliberado não conta para o limite de reinícios. `doctor()` usa `try_lock` e
responde "busy running a job" em vez de travar durante uma separação, e
`restart()` cancela o job em andamento. `SeparationService` registra jobs
pendentes ("Queued behind another separation", depois "Preparing local
workspace") que aparecem em `separation_status` e podem ser cancelados sem
esperar o job em execução. O engine de áudio separa `prepare_track` /
`prepare_stem_mix` (decodificação, fora do lock) de `AudioEngine::install`, e o
waveform do mix agora soma os picos de todos os stems (`mix_waveforms`) em vez
de vir só do `stems[0]`. Comandos de disco e SQLite (`cached_*`, `library_*`,
`reopen_audio_device`) rodam no pool bloqueante em vez da thread principal do
Tauri; os controles de reprodução continuam síncronos para preservar a ordem
das chamadas. `save_amt_midi` (diálogo nativo `rfd`) substitui
`export_amt_midi`. A validação de harmonia rejeita mais de 50.000 segmentos.
Python changes: Removido o escape hatch `LOCAL_MUSIC_ANALYZER_TEST_SEPARATOR`;
o modelo é chamado por `run_model()`, que os testes substituem. Áudio silencioso
na harmonia devolve `SILENT_AUDIO` em vez de uma tonalidade arbitrária.
Frontend changes: `modelsError` próprio (o erro da lista de modelos não
sobrescreve mais o erro de uma separação); salvar MIDI passa por `save_amt_midi`.
Benchmarks: `prepare_stem_mix` com 4 WAVs de 300 s estéreo: ≈500 ms contra
≈370 ms de um único stem (release; os stems são decodificados em paralelo).
Tests: cancelamento que retorna rápido contra um worker de 60 s (e falha sem a
correção), doctor busy + restart cancelando, fila visível e cancelável, envelope
do mix sobre WAVs reais, erros de stem, descarte de saída inválida e de worker
morto no AMT (ambos verificados por mutação), erro `SILENT_AUDIO`, limite de
segmentos.
Decisions: D-023.
Not done, on purpose:
- Journal de jobs em `manifest.jobs`: nenhum serviço persiste estado "em
  andamento"; artefatos só são promovidos ao terminar e `tmp/<job-id>` órfãos já
  são limpos. Implementar exigiria tocar todos os serviços sem consumidor na UI.
  `repair_interrupted_workspaces` ficou documentado como rede de segurança.
- Modelo recarregado a cada job: o cancelamento agora mata o worker (D-023), o
  que já descarta o cache, e o ganho (segundos) é pequeno diante da inferência
  (minutos); não há como validar um cache do `Separator` sem rodar o Demucs.
- Cancelamento cooperativo em Python: substituído pelo descarte do processo.
Known limitations: Cancelar uma separação recarrega o modelo na próxima. Um
`restart` pode deixar um job já enfileirado atrás do cancelado iniciar antes.
O diálogo de salvar MIDI, o estado "queued" e a aba de modelos não foram
exercitados no app Tauri em execução (janela nativa); a verificação foi por
testes Rust/Python, `svelte-check`, `vite build` e clippy.
Next: Backlog (loop A/B, metrônomo, time stretch...).

### Arranjo — timeline por stem no Mixer

Date: 2026-09-21
Commit: 86e2235, f8f8a14, 64b0a90, ed0bd6e
Rust changes: O engine guarda o waveform de cada stem (resolução ≈20 picos/s, até
40.000) e o expõe por `stem_waveforms`; o snapshot de polling deixou de carregar o
waveform (só as respostas de load o trazem). Novo módulo `stems` (`stem_audio`,
`is_known_stem`) extraído do pitch. `HarmonyService` e `AmtService` aceitam `stem`
opcional, com cache, persistência e MIDI próprios por stem (ver DATA_CONTRACTS); as
chaves de cache da mix não mudaram. Comandos `analyze_harmony`, `cached_harmony`,
`transcribe_track`, `cached_amt` e `save_amt_midi` ganharam o parâmetro opcional.
Python changes: None.
Frontend changes: Novo `Arrangement.svelte` (canvas virtualizado com overlay do que
soa, régua de compassos, faixas com waveform/notas/acordes, playhead suave,
transporte, zoom, follow, clique e arraste para posicionar, atalhos) e
`arrangement.ts` (grade, tempo↔pixel, notas ativas, colunas de waveform).
`App.svelte` carrega os waveforms e os caches ao abrir o mixer e orquestra "Analyze
all lanes".
Benchmarks: None.
Tests: 26 testes vitest da lógica de arranjo (`npm test`); Rust: resolução e
snapshot enxuto do engine, testes de integração de harmonia/AMT por stem (mix e stem
coexistem, nome de stem inválido, stem ausente), estabilidade das chaves de cache.
A UI foi dirigida em Chrome headless contra um backend Tauri simulado: layout em DPR
1 e 2, clique e arraste, teclado, zoom, Fit, mute/solo e follow.
Decisions: D-024.
Known limitations: Não foi exercitado no app Tauri real nem com áudio e análises
reais (só com dados sintéticos). Downbeats e fórmula de compasso não são detectados,
então o agrupamento em compassos é manual (padrão 4/4). Acordes por stem vêm da
análise de chroma/templates, limitada a tríades maiores e menores, e Basic Pitch por
stem pode ser lento em CPU. Bateria não tem notas (só waveform). Sem exportar MIDI
por faixa na UI (o comando `save_amt_midi` já aceita o stem).
Next: Backlog (loop A/B, metrônomo, downbeats/fórmula de compasso, correção manual
de acordes) ou validar o arranjo no app real.

### Ferramenta local de modelos — Demucs

Date: 2026-09-06
Rust changes: None. A instalação continua fora do runtime para preservar o
modo offline do worker.
Tooling changes: `scripts/install-demucs.ps1` instala explicitamente os perfis
`demucs-4` ou `demucs-6-experimental`. Ele descobre o conjunto de arquivos que
o `audio-separator` requer, faz download somente para staging, valida que todos
existem e não estão vazios, registra hashes/tamanhos locais e promove o bundle
apenas após validação. `-Plan` não acessa rede; `-Verify` detecta alterações ou
arquivos ausentes depois da instalação.
Dependency correction: O worker de análise passa a declarar `onnxruntime` CPU
explicitamente. Embora o perfil seja Demucs, `audio-separator` importa esse
runtime ao inicializar o registry de modelos; sem ele, o instalador falhava
antes de iniciar o download.
Known limitations: O hash registrado protege contra corrupção posterior, mas
esta versão do `audio-separator` não fornece checksums oficiais fixos dos pesos
remotos. Uma futura UI de Model Manager deve usar manifest versionado com
origem e hashes oficiais antes de oferecer distribuição pública.

### Correção — Verificação de integridade do bundle de modelo

Date: 2026-09-18
Cause: O inventário `.lma-bundle.json` gravado pelo instalador já registrava
SHA-256 por arquivo, mas nada no app em execução o lia. `SeparationService`
só checava a existência de `<filename>` e `.lma-model.json`, e o worker
Python carregava os pesos via `torch.load(weights_only=False)` sem qualquer
verificação — um download corrompido ou uma instalação adulterada era
confiada silenciosamente antes da desserialização do pickle.
Fix: Rust agora lê `.lma-bundle.json` e recalcula o SHA-256 de cada arquivo
listado antes de iniciar uma separação, com o resultado — sucesso ou motivo
da falha — mantido em cache por modelo pelo tempo de vida do processo, para
não hashear centenas de MB de pesos a cada job. O worker Python repete a
mesma verificação (também com cache por processo) imediatamente antes de
`load_model`, como última barreira antes da desserialização.
Tests: Novo teste Rust `rejects_a_model_bundle_whose_checksum_no_longer_matches`
e teste Python equivalente cobrindo o worker isoladamente; ambos verificados
como capazes de falhar sem a correção antes de serem restaurados. `cargo fmt`,
clippy, testes Rust, `ruff`, `mypy` e `pytest` do analysis-worker passaram.
Known limitations: A verificação por processo/sessão não detecta uma
substituição do arquivo depois de já verificado nessa mesma sessão; um
Model Manager futuro com manifest de origem oficial continua sendo o
caminho para distribuição pública.

### Correção de ambiente — FFprobe no Windows

Date: 2026-08-28
Cause: FFmpeg Essentials estava instalado pelo Winget, mas o diretório `bin`
não fazia parte do `PATH` da sessão que iniciou o Tauri.
Fix: `MediaTools::development` continua priorizando `LOCAL_MUSIC_ANALYZER_*`,
mas no Windows também procura a instalação local do pacote FFmpeg Essentials
do Winget. O fallback para `ffprobe`/`ffmpeg` no `PATH` permanece para outros
ambientes.
Validation: Smoke test real importou e normalizou um MP3 sem configurar as
variáveis `LOCAL_MUSIC_ANALYZER_FFPROBE` e `LOCAL_MUSIC_ANALYZER_FFMPEG`.

### Correção de ambiente — Publicação do workspace no Windows

Date: 2026-08-28
Cause: O workspace de desenvolvimento relativo à árvore do repositório herdou
uma ACL que nega remoção de subdiretórios. O staging era criado normalmente,
mas o rename atômico final era rejeitado com `os error 5`.
Fix: Sem `LOCAL_MUSIC_ANALYZER_WORKSPACE`, o host usa
`%LOCALAPPDATA%/LocalMusicAnalyzer/workspace` no Windows. O diretório de
workspace continua configurável pela variável de ambiente.
Validation: Teste unitário garante que o padrão não aponta para a árvore do
repositório; testes Rust, clippy, svelte-check e build passaram.

### Correção — Stems já separados são reaproveitados, e o mixer abre nos casos que falhavam

Date: 2026-09-21
Cause: Reproduzida no app Tauri real (WebView2 dirigido por depuração remota, com o
workspace do usuário). (1) Cada importação criava um track novo, então a mesma música
(13 importações do mesmo arquivo no workspace, 4 com cópias completas de stems, ~880 MB
duplicados) era separada de novo a cada vez. (2) Dois desses tracks tinham stems no disco
mas o manifest não os listava (provavelmente a corrida de escrita corrigida em 3d0a201: esses tracks são de 07/09):
"Separate" respondia cache hit e "Open mixer" falhava com "A generated stem failed local
validation: vocals", erro que a aba Stems não mostrava. (3) A checagem do cache só
acontecia depois de exigir o modelo instalado e de gastar ~3 s conferindo o checksum do
bundle. (4) "Open mixer" levava 5 s no build dev sem nenhum indicador.
Fix: `IngestService::import` devolve o track existente do mesmo checksum
(`reused: true`); `SeparationService` reaproveita, antes da fila e do modelo, stems do
próprio track, de outra importação da mesma música (hard link, cópia como plano B) ou
os que estavam órfãos no disco (registro no manifest); novo comando `cached_separation`
faz a UI mostrar "Using cached stems" ao abrir o track. "Open mixer" mostra progresso e
seus erros aparecem na aba Stems. Dependências passam a compilar com `opt-level = 3` no
perfil dev (5,0 s → 0,76 s para preparar o mix, medido).
Decisions: D-025, D-026.
Tests: Rust: importar o mesmo áudio reaproveita o track e o trabalho salvo, áudio
diferente cria outro, escolha da duplicata com stems, diretório sem áudio normalizado não
é reaproveitado; separação registra de novo stems órfãos, reaproveita stems de outro
track da mesma música sem modelo nem worker, e não reaproveita os de outra música. Os
testes foram verificados por mutação (reparo, adoção, ranking, checksum e checagem do
áudio desligados, um por vez).
Validation: No app real, um track com stems órfãos e um nunca separado abriram o mixer a
partir do cache e tocaram, com a posição avançando; clique, arraste, teclado e botões do
transporte posicionaram com exatidão (5,21 s esperado, 5,21 s obtido). O hard link foi
confirmado no disco (2 links, sem duplicar os 180 MB).
Known limitations: O diálogo nativo de importação não foi acionado no app real (só os
testes com ffprobe/ffmpeg de mentira cobrem o caminho de `import`). Nenhum áudio foi
ouvido: só o estado do motor (tocando, posição avançando, sem erro de dispositivo).
Duplicatas antigas seguem na biblioteca; nenhuma é removida automaticamente.

### Correção — Notas por faixa do arranjo (pitch de bass e vocals nunca chegava à tela)

Date: 2026-09-21
Cause: Reproduzida no app real. "Detect notes" em bass e vocals falhava sempre: primeiro
com "analysis worker exceeded the request timeout" (o pYIN de um stem leva 100 s ou mais
contra 8 s do launch de desenvolvimento) e, dado tempo, com "The pitch result returned by
the analysis worker is invalid": notas coladas se sobrepunham por ~7e-15 s (7 casos no
baixo). Sem esses resultados, o que sobrava funcionando era a transcrição da mix inteira
(aba MIDI). O `other` (Basic Pitch) já funcionava por faixa.
Fix: `ANALYSIS_REQUEST_TIMEOUT` (10 min) para rhythm, harmony e pitch. No worker, cada
nota termina exatamente no início da seguinte, e o pYIN roda a 22,05 kHz com resolução
de 0,15 semitom (112 s → 28 s no baixo). Removido `request_job`, sem uso.
Decisions: D-027.
Tests: Rust: rhythm, harmony e pitch completam uma análise mais lenta que o timeout do
launch (falhavam antes). Python: notas coladas nunca se sobrepõem (falha com o código
antigo). Ruff, mypy e pytest passam.
Validation: No app real, com o áudio do usuário: baixo 536 notas (E1–E3, 30 s), vocais
605 notas (C2–A5, 31 s), `other` 1025 notas e 2231 acordes, cada um distinto da mix
inteira (1209 notas, C#1–G5), visíveis nas faixas do mixer.
Known limitations: O Demucs de 4 stems mistura guitarra, teclas e sintetizadores em
`other`: separar por instrumento exige o modelo de 6 stems (guitar e piano), hoje sem
bundle instalado. A segmentação de acordes é muito picotada (mediana de 0,09 s, tanto na
mix quanto no `other`, com o ritmo já analisado), o que sugere que não está agrupada por
batida; já era assim antes e a causa não foi investigada.

