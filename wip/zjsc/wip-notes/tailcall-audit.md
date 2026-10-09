# Auditoria de proper tail calls (bun 1.4.2 contra o porte)

Escopo: confirmar que o porte emite e executa `op_tail_call` e `op_tail_call_varargs` sem crescer a pilha nativa
do Rust nem a de registradores de JS. Nenhum código foi alterado nesta passada (nada faltava no caminho estrito).

## O que o porte tem (conferido no código)

- Bytecompiler: `allow_tail_call_optimization` em `src/bytecompiler/bytecode_generator_cpp1.rs:508`, igual ao
  upstream (`BytecodeGenerator.cpp:451`): `useTailCalls && !isConstructor && constructorKind == None &&
  isStrictMode`. Program, eval e módulo ficam `false` (cpp1:430, cpp2:31), como no upstream.
- `emit_call_in_tail_position` (`bytecode_generator_cpp4.rs:1451`) e `emit_call_varargs_in_tail_position`
  (cpp4:1880) escolhem `op_tail_call` / `op_tail_call_varargs`; `set_has_tail_calls()` é marcado.
- Envenenamento da posição de cauda: `emit_node` de statement e de expressão zeram a flag (como o `SetForScope`
  do `.h`); `ReturnNode` com `has_finally_scopes()` usa `emit_node_expression` (cpp5b:531), então `try/finally`
  não é cauda; o `try` sem finally emite o bloco por `emit_node` (cpp5c:307), também sem cauda.
- `FunctionCallDotNode` (`f.call`, `f.apply`) emite `emit_call_in_tail_position` e
  `emit_call_varargs_in_tail_position` (nodes_codegen_cpp3b.rs:136 a 311). `Reflect.apply` é builtin JS
  (`ReflectObject.js`: `return target.@apply(...)`), então vira `op_tail_call_varargs` dentro do builtin.
- `op_tail_call_forward_arguments` NÃO existe nesta versão do JSC (`BytecodeList.rb` só tem `tail_call` e
  `tail_call_varargs`; o grep do upstream confirma). Nada a portar.
- Interpretador: `dispatch.rs` `call_prepared_frame` (linha 509): com `tail` e callee de script, chama
  `replace_frame_for_tail_call` (a `prepareForTailCall`: copia cabeçalho e argumentos para o lugar do frame
  corrente, preserva `callerFrame` e `returnPC`, copia no sentido que não pisa a origem) e devolve
  `CallOutcome::TailCall`; `dispatch_loop_from` devolve `LoopExit::TailCall` e `dispatch_loop`
  (`dispatch_ext.rs:158`) recomeça em pc 0 do callee com `enter_frame(.., check_depth = false)`. Logo o frame
  não consome `native_depth` nem Rust stack: é laço. `varargs.rs:290` passa o mesmo `tail` pelo mesmo caminho.
- Testes existentes: `tests/proper_tail_calls.rs` (8 testes: auto-recursão e recursão mútua de 1e6, mudança de
  contagem de argumentos, `this`/`arguments`, varargs com spread e `apply`, callee nativo, exceção atravessando
  o frame trocado, frame do chamador ausente no stack) e `tests/call_spread_varargs.rs::tail_position_varargs`.
  Eles passam por leitura do código (o cargo não foi rodado, por ordem da tarefa).

## Medição no bun 1.4.2 (profundidade 1e6, programas em /tmp/tcaudit/tc.js, tc2.js)

Atenção: o "sem `"use strict"`" desta medição era ESM (estrito de fato). Em sloppy de verdade (`.cjs`) todos os
casos de cauda abaixo dão `RangeError`; ver a divergência 1.

| Programa | bun | Porte (por leitura) |
|---|---|---|
| `f(n-1)` auto-recursão | 0 (estrito/ESM); RangeError em `.cjs` sloppy | estrito: laço; sloppy: RangeError |
| `f.call(this, n-1)` | 0 | estrito: tail para o alvo; sloppy: RangeError |
| `f.apply(this,[n-1])` | 0 | idem, via `op_tail_call_varargs` |
| `Reflect.apply(f,this,[..])` | 0 | estrito: tail no builtin JS; chamador `return Reflect.apply` é tail nativo-builtin |
| arrow, método, método de classe | 0 | iguais (estrito para classe e módulo; arrow e método sloppy não) |
| `f(...[n-1])` (spread) | 0 | estrito: `op_tail_call_varargs` |
| recursão mútua | 0 | idem |
| `return n && f(n-1)`, `return (0, f(n-1))` | 0 | `&&` e vírgula propagam posição de cauda (cpp4:179 a 367) |
| mais ou menos argumentos que o chamador | 0 | coberto por `strict_tail_call_changes_the_argument_count` |
| `try { return f() } finally {}` | RangeError | não é cauda (`has_finally_scopes`): RangeError |
| `try { return f() } catch {}` | RangeError capturado (undefined) | não é cauda: igual |
| generator `yield* g(n-1)` | RangeError | não é cauda: igual |
| `new F(n-1)` | RangeError | construct nunca é cauda: igual |
| `1 + f(n-1)` e `f(n-1)` como statement | RangeError | igual |
| async `return f(n-1)` | retorna e rejeita com RangeError | corpo async não é cauda; deve rejeitar com RangeError |
| bound function `g = f.bind(null)`, `return g(n-1)` | 0 | DIVERGE (ver abaixo) |

## Divergências conhecidas

1. Sloppy. CORRIGIDO (ver `wip/notes/tailcall-sloppy.md`): a hipótese anterior de que o bun faz cauda em sloppy
   por JIT (DFG `handleRecursiveTailCall`) estava errada. As medições "sloppy" daquela passada rodaram como ESM
   (`.js` sem diretiva e sem marcador de CJS), portanto estrito. Em sloppy de verdade (`.cjs` sem diretiva) o bun
   dá `RangeError` em 1e6 (limite de pilha do interpretador entre 3e4 e 1e5, igual com `useJIT=0`). O bytecode
   só emite `op_tail_call` com `isStrictMode()` (`BytecodeGenerator.cpp:451`) e o porte faz o mesmo; não há
   heurística de JIT a reproduzir. O teste `sloppy_recursion_is_not_a_tail_call` e as 11 linhas `sloppy_*` do
   golden (todas `RangeError`) fixam isso.
2. Bound function em posição de cauda (estrito ou não): o bun dá 0 em 1e6 (DFG inline
   `BoundFunctionTailCall`). No porte, `JSBoundFunction` é host function, vai por `handle_host_call` e o alvo
   roda por `executeCall` aninhado, então esbarra em `MAX_NATIVE_DEPTH = 10_000` e vira RangeError antes de 1e6.
   O LLInt do upstream faz o mesmo (aninha), por isso não foi mexido; se algum golden exigir, a saída é
   desembrulhar o bound function em `call_prepared_frame` quando `tail` (substituir callee, `this` e
   prefixar argumentos antes de `replace_frame_for_tail_call`).
3. Limite de profundidade não-cauda: o porte para em `MAX_NATIVE_DEPTH = 10_000` frames JS, o bun tem um limite
   bem maior; programas "recursão profunda sem tail" com N entre 1e4 e o limite do bun devem divergir.

## Golden tests/golden/function_proto_bun.tsv

O TSV tem 3011 linhas; só duas famílias dependem de pilha: `apply` com array-like de 1e5 e 1e6 argumentos
(`new Array(1e5).fill(0)`, `{length: 1e6}`, `Math.max(...new Array(1e5).fill(1))`) e `Reflect.construct` de 1e5.
Elas dependem do limite de argumentos varargs (coberto por `too_many_arguments_is_a_range_error`) e não de
tail call. Programas de recursão e tail do audit original foram descartados do golden porque o bun entra em laço
infinito neles; os casos medidos aqui (24 programas, não do TSV) substituem a medição e estão na tabela acima.
Simulação à mão do caso `return f.apply(this, arguments)` estrito: `emit_call_varargs_in_tail_position` ->
`op_tail_call_varargs` -> `varargs.rs` monta o frame em `first_free` -> `call_prepared_frame(tail = true)` ->
`replace_frame_for_tail_call` move o frame para `cfr + argument_area - argc` -> `LoopExit::TailCall` ->
`dispatch_loop` reentra sem `check_depth`: pilha nativa constante.

## Passada de correção (sem cargo)

- Medição reproduzível: `scripts/gen-tailcall-golden.js` grava `tests/golden/tailcall_bun.tsv` (22 casos, strict e
  esm e sloppy autêntico). O "0 em sloppy" daquela passada era ESM; a conclusão de "artefato do JIT" foi
  retirada (ver divergência 1).
- Bound function em cauda: IMPLEMENTADO em `src/llint/dispatch.rs` (`unwrap_bound_function_for_tail_call`, chamado
  por `call_prepared_frame` com `tail` e `CodeForCall`): substitui o callee pelo alvo, o `this` pelo ligado e
  prefixa `bound_args`, em frame novo abaixo do original; repete para bound de bound; alvo não chamável segue
  pelo caminho nativo (TypeError normal). Sem unsafe. Não compilado nem testado (ordem da tarefa).
- Sloppy: sem `op_tail_call`, como o upstream. O teste `tests/tailcall_bun_golden.rs` não pula mais nada: os
  casos `esm_*` (estritos) e `sloppy_*` (`.cjs`, RangeError) contam, 33 no total.
- Destino host em cauda (`Reflect.apply` nativo, `f.call` em builtin) segue por `handle_host_call`, como `op_call`.

## Pendências sugeridas (não feitas, sem cargo)

- `strict_try_finally` e `sloppy_try_finally` (golden do bun: `RangeError`) saem com `R` undefined no porte.
  Leitura do código (2026-10-08, sem cargo): NÃO é tail call indevido. O bytecompiler já não emite
  `op_tail_call` dentro de `try/finally` (`has_finally_scopes`, cpp5b:531) e o `enter_frame` confere a
  profundidade nativa (`dispatch.rs:170`). `create_stack_overflow_error` (`runtime/error.rs:95`) também cria um
  `ErrorInstance` RangeError normal. Hipótese em aberto: o erro que chega ao `catch(e)` de cima tem `name`
  undefined, ou o rethrow do `finally` (op_catch, registrador de completion, `op_throw`) entrega outro valor
  depois de centenas de reentradas. Próximo passo, só com cargo: rodar o programa com profundidade 50 000 e
  imprimir `typeof e`, `e instanceof RangeError`, `e.name` e `e.message` dentro do `catch`.

- Testes novos em `tests/proper_tail_calls.rs` para: `f.call` e `Reflect.apply` estritos a 1e5, `try/finally`
  e `new` e generator dando RangeError, arrow estrita. Escrever sem rodar seria chute; ficam para quem puder
  rodar.

## Causa provável do `finally` com estouro de pilha (2026-10-08, leitura, sem cargo)

- O handler `finally` (`emit_out_of_line_finally_handler`) grava no registrador de completion a célula
  `Exception` (`store_caught_value`: `JSValue::from_cell(exception.cell_id())`), e o `op_throw` do fim do
  `finally` relança esse registrador. `IntoException for JSValue` (`runtime/throw_scope.rs`) fazia
  `Exception::create(vm, valor)` sempre, sem o `dynamicDowncast<Exception>` do C++: cada nível do `finally`
  embrulhava a `Exception` anterior numa nova, e o `catch(e)` de cima recebia a célula `Exception` em vez do
  `RangeError` (`typeof e` errado, `R` undefined). Isso explica por que só o `finally` quebra (o
  `catch(x){throw x}` relança o valor, não a célula) e por que `f(2000)` funciona (sem estouro o caminho
  de throw nunca roda).
- Correção: `IntoException for JSValue` reaproveita a `Exception` quando o valor é a célula de uma
  (`Exception::from_cell_id`). Não compilado nem rodado.
- Verificar com cargo: o programa `function f(n){try{return n?1+f(n-1):0}finally{}} ...` deve lançar o Error
  final com `R=object`. Se o script ainda terminar em silêncio, o próximo suspeito é `Err(Thrown)` sem
  exceção pendente (`unwind` devolve `None` com `vm.exception()` vazio e `value_or_pending_exception` vira
  `JSValue::empty` sem exceção, que `completion` lê como `Ok`); instrumentar `dispatch_loop`.
