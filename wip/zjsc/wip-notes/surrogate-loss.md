# Perda de surrogates solitários na saída do filho (U+FFFD no resultado)

Levantamento por grep, sem editar gerador nem golden. Critério: goldens com U+FFFD em `tests/golden/*.tsv` (14 arquivos), cruzados com o gerador que os escreve e com a forma como o filho emite `globalThis.R`.

Dos 273 `scripts/gen-*-golden.js`, os que escrevem `process.stdout.write(String(globalThis.R))` ou `text = String(globalThis.R)` sem `JSON.stringify` são vários (lista completa: `grep -ln 'write(String(globalThis.R))' scripts/gen-*-golden.js`). Só os da tabela abaixo têm U+FFFD no golden.

## Geradores com `String(globalThis.R)` cru e U+FFFD no golden

| Gerador | Golden | Linhas com U+FFFD | Onde está | Veredito |
|---|---|---|---|---|
| gen-json-reviver-golden.js | json_reviver_bun.tsv | 193 | resultado | 177 `Unrecognized token '�'` + 5 `Invalid escape character �` + 11 `"\u00�4" is not a valid unicode escape`: PERDA (o bun imprime a unidade UTF-16 solta, o UTF-8 do filho a troca por U+FFFD) |
| gen-json-grid-golden.js | json_grid_bun.tsv | 10 | resultado | 10 `Unrecognized token '�'` (programa com emoji, token é meia-unidade): PERDA |
| gen-json-deep-golden.js | json_deep_bun.tsv | 10 | resultado | 6 PERDA (1 `Unrecognized token '�'`, 5 `Invalid escape character �`); 4 legítimos (programas com `�` real) |
| gen-bigint-symbol-golden.js | bigint_symbol_bun.tsv | 2 | resultado | `Symbol('\ud800')` vira `Symbol(�)`: PERDA (esse gerador também faz `String(globalThis.R)` em `text`) |
| gen-string-receiver-args-golden.js | string_receiver_args_bun.tsv | 6 | resultado | todos `toWellFormed` com `\ud800x`: LEGÍTIMO (U+FFFD real) |
| gen-string-coerce-golden.js | string_coerce_bun.tsv | 22 | resultado | todos `toWellFormed`: LEGÍTIMO |
| gen-string-unicode-extra-golden.js | string_unicode_extra_bun.tsv | 1 | resultado | linha 2530 `JSON.parse('\u{e0020}1')` com `Unrecognized token '�'`: PROVÁVEL PERDA (token é unidade solta). A linha 4567 (`escape("�")` no programa, resultado `%uFFFD`) é legítima, mas está contada no mesmo arquivo: total real de 2 linhas, ver abaixo |

Nota sobre string_unicode_extra: o grep por linha acusou 2 ocorrências no arquivo (linhas 2530 e 4567), a contagem por última coluna dá 1 (a 4567 tem U+FFFD no programa). Perda: 1; legítimo no programa: 1.

## Geradores que já usam JSON.stringify ou o prelúdio (U+FFFD não vem de perda do filho)

| Gerador | Golden | Linhas | Onde está | Veredito |
|---|---|---|---|---|
| gen-annexb-methods-golden.js | annexb_methods_bun.tsv | 1 | programa | `escape("�")` literal: LEGÍTIMO (filho emite JSON.stringify) |
| gen-builtins-golden.js | builtins_bun.tsv | 2 | resultado | `'\ud800'.toWellFormed()` e `'a\udc00b'.toWellFormed()`: LEGÍTIMO |
| gen-regexp-more-golden.js | regexp_more_bun.tsv | 1 | resultado | `RegExp.escape("�")`: LEGÍTIMO |
| gen-string-unicode-more-golden.js | string_unicode_more_bun.tsv | 1 | prelúdio/programa (linha 244) | LEGÍTIMO (texto-fonte; filho com JSON.stringify) |
| gen-string-unicode-golden.js, gen-string-golden.js | string_unicode_bun.tsv, string_bun.tsv | 6 e 7 | programa (coluna do meio, nenhuma na última) | LEGÍTIMO; filho usa JSON.stringify |
| gen-module-more-golden.js | module_more_bun.tsv | 2 | resultado (campo `error` do JSON) | PROVÁVEL PERDA: `export 'a\uD800' not found` vira `'a�'`; o gerador serializa com JSON.stringify de `{log, error}` (linha 46), mas passa por `clean(...)` e `spawnSync(... encoding "utf8")`, então confirmar se a perda é do bun ou do pipe antes de mexer |

## Resumo

- PERDA confirmada (resultado tem U+FFFD no lugar de surrogate solto): json_reviver (193), json_grid (10), json_deep (6 de 10), bigint_symbol (2). Total 211 linhas, 4 geradores.
- PROVÁVEL PERDA: string_unicode_extra (1), module_more (2, gerador já com JSON mas passa por `clean`).
- LEGÍTIMO: string_receiver_args (6), string_coerce (22), builtins (2), regexp_more (1), annexb_methods (1), string_unicode_more (1), string_unicode (6), string (7), json_deep (4 de 10), string_unicode_extra (1 no programa).
- Os 4 geradores com perda confirmada usam `process.stdout.write(String(globalThis.R))` (json_reviver linha 14, json_grid 21, json_deep 15) ou `process.stdout.write(text...)` (bigint_symbol 19): candidatos a trocar pelo `RESULT_PRELOAD`/`decodeResult` de `scripts/golden-prelude.js`.
- Depois da troca, regerar e conferir que as mensagens de erro do bun passam a conter `\ud83d` solto em vez de U+FFFD; os goldens legítimos não devem mudar.
