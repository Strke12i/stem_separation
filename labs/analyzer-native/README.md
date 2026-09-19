# analyzer-native (laboratório da Fase 13)

Extensão Rust/PyO3 para o `analysis-worker`. É um **experimento**, não faz parte do
build do produto: fica fora do workspace Cargo (por isso `cargo test --workspace` não
precisa de toolchain Python) e nenhum código do worker a importa.

Ver D-020 e D-022 em `docs/DECISIONS.md`.

## O que mede

`smooth_labels` reproduz exatamente `music_analyzer_worker.harmony.smooth` (rótulo
majoritário numa janela de `2 * width + 1` quadros, empate pela posição mais antiga
na janela). É o único trecho de pós-processamento que continua em Python puro depois
da vetorização NumPy de `chord_frames` e `segment`.

## Como reproduzir

```text
cd labs/analyzer-native
uvx maturin build --release -i ../../python/analysis-worker/.venv/Scripts/python.exe
cd ../..
uv run --project python/analysis-worker \
  --with labs/analyzer-native/target/wheels/analyzer_native-0.0.0-cp311-cp311-win_amd64.whl \
  python labs/analyzer-native/bench.py
```

O nome do wheel muda com a versão do Python e a plataforma. O script primeiro
confere igualdade com `smooth` em 15 combinações de semente e largura.

## Resultado (Windows, CPython 3.11, 2026-09-19)

| Quadros | Python | Rust (PyO3) | Ganho |
| ------: | -----: | ----------: | ----: |
| 100 | 0,317 ms | 0,022 ms | 14× |
| 22 161 (faixa de 257 s) | 88 ms | 8,8 ms | 10× |

Custos de empacotamento observados: build a frio de 55 s, wheel de 105 KB, e um
wheel por versão de Python e por plataforma, além de exigir Rust no build do sidecar.

## Veredito

Não promover. O ganho absoluto é ≈80 ms por faixa, numa etapa de harmonia cujo
`chroma_cqt` sozinho gasta de 5 a 8 s. O custo permanente (toolchain Rust no
empacotamento Python, wheels por plataforma) é desproporcional.
