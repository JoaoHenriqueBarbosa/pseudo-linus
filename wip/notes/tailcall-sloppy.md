# Cauda "sloppy" no bun: não existe, era rótulo errado no golden

## Medição (bun 1.4.2)

- `function f(n){return n?f(n-1):0}` com 1e6 níveis, arquivo `.js` sem diretiva e sem marcador de CJS: resultado 0.
  Nesse arquivo `(function(){return this===undefined})()` dá `true`: o bun o roda como ESM, portanto ESTRITO.
- O mesmo programa em `.cjs` sem diretiva (sloppy de verdade, `this` solto é o global): `RangeError`.
  Sondagem de profundidade em sloppy: 3e4 passa, 1e5 estoura. Idêntico com `BUN_JSC_useJIT=0` e com
  `BUN_JSC_useDFGJIT=0`: o limite é de pilha do interpretador, sem JIT envolvido.
- O mesmo programa com `"use strict"` no topo (`.cjs` estrito ou `.js`): 0.

## Mecanismo no JSC

Não há tail call de JIT em sloppy. `BytecodeGenerator.cpp:451` (`m_allowTailCallOptimization` exige
`functionNode->isStrictMode()`) é a única fonte de `op_tail_call`; o LLInt, o Baseline e o DFG só executam o
que o bytecode já marcou. Em sloppy o bytecode emite `op_call` e a recursão de 1e6 estoura a pilha. A hipótese
antiga (DFG `handleRecursiveTailCall` em sloppy, em `wip-notes/tailcall-audit.md`) estava errada: a medição
sloppy daquela passada tinha rodado ESM.

## Origem do erro

`scripts/gen-tailcall-golden.js` chama `prepareProgram(original)` sem `sloppy = true`. Sem diretiva, o fonte cai
em ESM (`golden-prelude.js`, regra "ESM é o padrão"), o bun executa estrito, e a linha gravada leva
`"use strict"` embutido pelo `canonicalSource`. As 11 linhas `sloppy_*` eram então programas estritos (modo 0 na
meta) e o porte, que lê o `"use strict"`, emite `op_tail_call` para elas igual aos `strict_*` (modo 2, CJS
estrito). O teste as pulava por um motivo que não existe.

## O que foi feito

- Runner `tests/tailcall_bun_golden.rs`: removido o `continue`; os 22 casos contam.
- Linhas renomeadas `sloppy_*` para `esm_*` no tsv e no gerador (laço `["strict", "esm"]`).
- Não houve mudança no interpretador: nenhuma heurística de JIT a reproduzir.
- Não rodei cargo: o resultado dos 11 casos `esm_*` no porte não foi observado. Eles devem se comportar como os
  `strict_*` equivalentes (mesma fonte, outro wrapper); `esm_try_finally` e `esm_construct` esperam RangeError e
  caem na pendência já anotada do `finally` em `wip-notes/tailcall-audit.md`.

## Pendência: sloppy de verdade

Gerar linhas `sloppy_*` autênticas com `prepareProgram(original, true)` (`.cjs`, modo 1). O bun responde
`RangeError` em todas, então no porte exigem que o limite de profundidade seja alcançado sem tail call
(`MAX_NATIVE_DEPTH = 10_000` já dá RangeError a 1e6). A diferença de limite (bun entre 3e4 e 1e5, porte 1e4)
só aparece com N intermediário, tema da divergência 3 do audit.
