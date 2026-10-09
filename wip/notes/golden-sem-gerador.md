# Goldens sem gerador versionado (auditoria de 2026-10-09)

Escopo: os 433 arquivos distintos referenciados por `include_str!("golden/...")` em `tests/*_golden.rs`.
Critério: o nome do arquivo aparece em algum `scripts/*.js` ou `scripts/*.py`, ou o gerador o produz por
`<nome>_bun.tsv` + `<nome>.preludes.json` (escritos por `writePreludes` de `scripts/golden-prelude.js`).

## Nenhum tsv escrito à mão

Todo `*_bun.tsv` / `*.tsv` referenciado tem gerador. Os 122 `*.preludes.json` saem do mesmo gerador do tsv
irmão (via `golden-prelude.js`); o grep literal pelo nome do json não os acha, mas não são manuais.

## Sem gerador JS/Python

| arquivo | situação |
|---|---|
| `wasm_stack_bun.js` | fonte do caso, escrito à mão por natureza (entrada, não saída). |
| `wasm_stack_bun.expected` | tinha gerador só descrito em comentário. Agora tem `scripts/gen-wasm-stack-golden.js`; duas execuções com `GOLDEN_OUT_DIR=/tmp/xw` saem idênticas ao arquivo existente. |
| `string_hash.txt` | gerador é `scripts/oracle/string_hash.cpp` (C++ compilado contra o WebKit do Bun), não JS. Versionado, mas não rodado nesta auditoria (exige build do WebKit). |

## Referenciados mas inexistentes em `tests/golden/`

- `likely_subtags_bun.tsv`: o gerador existe (`scripts/gen-likely-subtags.js`), o arquivo não está gerado.
- `promise_grid.preludes.json`: o gerador existe (`scripts/gen-promise-grid-golden.js`), o arquivo não está gerado.

Nos dois casos o teste não compila até o gerador rodar.

## Observação

`tests/golden/*.tsv` e `*.preludes.json` aparecem como não rastreados (`??`) no git no momento da auditoria.
