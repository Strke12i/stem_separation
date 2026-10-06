# Avisos de licença de terceiros

Este projeto (código deste repositório) é distribuído sob a [licença MIT](LICENSE).
Este arquivo documenta a licença dos componentes de terceiros que o aplicativo usa
em tempo de execução — bibliotecas empacotadas com ele e modelos de machine
learning baixados separadamente — para que qualquer pessoa possa avaliar os termos
antes de usar, redistribuir ou adaptar o projeto.

Isto não é aconselhamento jurídico. As licenças abaixo foram conferidas nos
arquivos `LICENSE` e nos metadados públicos (crates.io/PyPI/npm) de cada projeto
upstream em 2026-10-06; reconfirme a origem antes de qualquer uso comercial ou
redistribuição, especialmente dos pesos dos modelos.

## Modelos de machine learning (baixados sob ação explícita do usuário)

Nenhum peso de modelo é distribuído neste repositório. `scripts/install-demucs.ps1`
baixa o modelo Demucs apenas quando o usuário executa o comando; o worker de AMT
baixa o modelo Basic Pitch na primeira transcrição. Ver `docs/MODEL_MANAGEMENT.md`.

| Modelo | Autor | Licença | Fonte |
| --- | --- | --- | --- |
| Demucs (`htdemucs`, `htdemucs_6s`) | Meta Platforms (facebookresearch/demucs) | MIT | [LICENSE](https://github.com/facebookresearch/demucs/blob/main/LICENSE) |
| Basic Pitch (modelo ICASSP 2022) | Spotify (spotify/basic-pitch) | Apache License 2.0 | [LICENSE](https://github.com/spotify/basic-pitch/blob/main/LICENSE) |

Nenhum dos dois projetos upstream publica um arquivo de licença separado para os
pesos pré-treinados; o único `LICENSE` de cada repositório cobre tanto o código
quanto os modelos que ele distribui. Isso não é o mesmo que uma garantia sobre o
conteúdo usado para treinar os modelos, que este projeto não controla.

## Principais bibliotecas (Rust)

| Crate | Licença |
| --- | --- |
| tauri | Apache-2.0 OR MIT |
| rodio | MIT OR Apache-2.0 |
| tokio | MIT |
| serde / serde_json | MIT OR Apache-2.0 |
| rusqlite | MIT |
| sha2 | MIT OR Apache-2.0 |
| thiserror | MIT OR Apache-2.0 |
| tracing / tracing-subscriber | MIT |
| uuid | MIT OR Apache-2.0 |

A árvore completa de dependências transitivas é maior que esta lista; use
`cargo metadata` ou `cargo license` (não incluído neste repositório) para gerar
um inventário completo antes de uma distribuição formal.

## Principais bibliotecas (Python)

| Pacote | Licença |
| --- | --- |
| audio-separator (nomadkaraoke) | MIT |
| librosa | ISC |
| onnxruntime | MIT |
| basic-pitch | Apache License 2.0 |

## Principais bibliotecas (frontend)

| Pacote | Licença |
| --- | --- |
| svelte | MIT |
| @tauri-apps/api, @tauri-apps/cli | Apache-2.0 OR MIT |
| vite | MIT |

## Sobre o áudio que você processa

Este software não envia áudio a nenhum serviço externo (ver
`docs/SECURITY_PRIVACY.md`) e não concede nenhum direito sobre a música que você
processa com ele. Separar stems, transcrever MIDI ou analisar harmonia/ritmo de
uma gravação protegida por direitos autorais não transfere, remove nem modifica
esses direitos. Você é responsável por ter autorização para processar o áudio que
importa e por como usa os artefatos gerados (stems, MIDI, análises).
