# Auditoria de funções globais e escopo (golden contra o bun)

## O que existe

- `scripts/gen-globals-golden.js` gera `tests/golden/globals_bun.tsv` (1809 programas, medidos no bun 1.4.2).
  Cada programa roda como script (`vm.runInThisContext`, arquivo `globals_case.js`) em processo próprio,
  sloppy por padrão (programas que começam com `'use strict'` ganham `var R;` depois da diretiva). O
  resultado é a variável global `R`; exceção sem captura vira `Uncaught Nome: mensagem`. Fontes só em ASCII
  (o porte lê Latin-1), sem caminho da máquina, 3 programas descartados (laço infinito e `delete globalThis`).
- `tests/globals_bun_golden.rs` compara com `evaluate_named_script_result` (padrão de
  `function_error_bun_golden.rs`; exceção do programa vira `Uncaught` + `describe_exception`).
- Regerar: `bun scripts/gen-globals-golden.js > tests/golden/globals_bun.tsv`.

## Cobertura

parseInt (100 pares texto/radix), parseFloat/isNaN/isFinite (100 entradas cada), encodeURI, decodeURI,
encodeURIComponent, decodeURIComponent, escape, unescape (surrogates soltos, URIError e mensagens), eval direto e
indireto (var/let/const/function, strict e sloppy, `this`, `arguments`, `new.target`, `super`, delete de var de
eval, valor de conclusão), `new Function` (parâmetros, corpo, SyntaxError, toString, construtores de
generator/async), globalThis (NaN/Infinity/undefined, var vs let global, delete, descritores, getter/setter,
preventExtensions), TDZ, Annex B, `with` e `Symbol.unscopables` (inclusive Proxy), `arguments` mapeado vs
strict, rótulo+function, closures em laço, modo strict (this, atribuição a não declarada, delete não deletável).

## Revisão do código (sem rodar nada)

- `js_global_object_functions.rs` e `js_global_object_functions_natives.rs`: `encode`, `decode`,
  `escape`, `unescape`, `parseFloat`, atalhos de `parseInt`/`parseFloat` e mensagens de `URIError` conferidos
  contra `JSGlobalObjectFunctions.cpp`; nenhuma divergência.
- `execute_eval.rs`: corrigido o `variableObject` do `eval` que não cria `StrictEvalActivation`. O
  C++ só para no global ou num `VarScope`; o porte também parava num `StrictEvalActivation` da cadeia. Agora o
  ramo estrito usa a ativação recém-criada e o outro ramo procura sem contar ativação (o achatamento de dicionário
  uncacheable ficou só nele, como no C++).

## Pendências conhecidas

- `eval` não tem o pré-parser JSON (`LiteralParser::tryEval`), documentado em `execute_eval.rs`.
- Programa que deixa `R` sem existir (global não extensível) lança `ReferenceError` na leitura final de `R` no
  porte, enquanto o bun grava `<undefined>`; hoje nenhum programa do golden depende disso.
- Primeira rodada do golden contra o porte ainda não feita (a tarefa proibiu rodar cargo).
