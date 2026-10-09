# Limites de pilha: lógicos, não da thread nativa

## O que o C++ faz

- `Options::maxPerThreadStackUsage` = 5 MiB, `softReservedZoneSize` = 128 KiB, `reservedZoneSize` = 64 KiB
  (`runtime/OptionsList.h:127-129`).
- `VM::updateStackLimits` (`VM.cpp:1285`): `m_stackLimit = StackBounds::recursionLimit(startOfStack, maxPerThreadStackUsage,
  reservedZoneSize)`, onde `startOfStack` é o `m_stackPointerAtVMEntry`. Ou seja, a pilha de usuário é
  `5 MiB - 64 KiB` a partir da entrada no VM, e é limitada também pela pilha real da thread
  (`StackBounds.h:95`). Toda recursão nativa (parser, gerador de bytecode, `isSafeToRecurse`) compara o ponteiro de
  pilha com esse `m_stackLimit`.
- Na build CLoop (a que o porte segue), a recursão JS não usa a pilha nativa: usa a `CLoopStack` (5 MiB de
  registradores) e `VM::updateSoftReservedZoneSize` (`VM.cpp:322`) chama `cloopStack().setSoftReservedZoneSize(128 KiB)`.
- `JSON.parse` (`LiteralParser`) e `JSON.stringify` (`Stringifier`) são iterativos; só têm tetos próprios
  (`maximumSideStackRecursion` = 40000 no stringify). Regexp: `YarrMatchingContextHolder` usa
  `stack.recursionLimit(reservedZoneSize)`, isto é, a pilha inteira da thread; `StackCheck` do compilador idem.

## O que o porte fazia (o defeito)

- `VM::new` gravava `stack_limit = sp - STACK_BUDGET`, com o orçamento vindo de `set_thread_stack_budget`. Os goldens
  de pilha funda passavam `256 MiB - 16 MiB`. Logo o limite de parser, bytecode e JS dependia do TAMANHO DA THREAD, e
  era dezenas de vezes maior que os 5 MiB do bun.
- `Interpreter::MAX_NATIVE_DEPTH` = 50 000 era um teto de profundidade que o C++ não tem.
- A zona suave da `CLoopStack` nunca era configurada (0 em vez de 128 KiB).
- `StackCheck` do Yarr: 512 KiB de pilha NATIVA, que varia com perfil de compilação.

## Princípio

Dois testes separados:

1. Lógico (decide a profundidade, fiel ao bun): bytes de pilha que os frames do C++ ocuparam. Independe da thread e do
   perfil (debug/release) do Rust.
2. Nativo (rede de segurança): o `m_stackLimit` derivado de `set_thread_stack_budget`. A thread grande existe só para
   que este teste nunca dispare antes do lógico.

## Aplicado

- `VM::logical_stack_limit()` = `maxPerThreadStackUsage - reservedZoneSize` (5 MiB - 64 KiB).
- `VM::enter_logical_frame(cost)` devolve um `LogicalStackFrame` (RAII). Soma `cost` bytes lógicos; falha se passar do
  limite lógico ou se a pilha nativa acabou. `VM::is_safe_to_recurse()` passa a conferir também o contador lógico.
- `stack_cost::PARSER_LEVEL` (512): bytes de pilha lógica de um nível do ciclo `parseStatement`/`parseAssignment...`.
  Usado em `parse_statement` e na macro `fail_if_stack_overflow!` (os 8 pontos de `failIfStackOverflow`). A macro agora
  guarda o frame até o fim do bloco, como o frame C++ vive até o retorno.
- JS: removido `MAX_NATIVE_DEPTH`. O limite lógico é a `CLoopStack` (5 MiB), agora com a zona suave de 128 KiB
  (`Interpreter::new` chama `set_soft_reserved_zone_size(Options::softReservedZoneSize())`). O teste nativo em
  `enter_frame` ficou só como rede de segurança.
- `JSON.parse/stringify`: nada a mudar, já são iterativos com os tetos do C++.
- `can_recurse` do parser removido (repasse).

## Medições no bun 1.4.2 (2026-10-08, bisseção; script em /tmp/calib/measure.js)

Maior n que NÃO lança (RangeError/SyntaxError "Stack exhausted"), `(0,eval)` salvo indicação:

| caso | n |
|---|---|
| `(`*n + `1` + `)`*n | 3503 |
| `if(1)`*n + `1` | 5756 |
| `{`*n + `}`*n (blocos) | 2879 |
| `function f(){`*n + `}`*n | 3071 |
| `a=>`*n + `1` | 2442 |
| `f(`*n + `1` + `)`*n | 3837 |
| `1?1:`*n + `1` | 7861 |
| `!`*n + `1` | 64643 |
| `new Function("return "+"("*n...)` | 7496 |
| `[`*n + `]`*n | sem limite até 1 000 000 (quirk do bun, não calibrar) |
| `1+1+...` | iterativo, sem limite |
| `({a:`*n | n=2000 ABORTA o bun (4 GiB de RSS); não usar como caso |
| JSON.parse `[`*n, `{"a":`*n | sem limite até 4 000 000 (iterativo) |
| JSON.stringify array / objeto aninhado | 39999 (teto 40000 do C++) |
| `new RegExp("("*n + ")"*n)` | 32768 |
| `new RegExp("(?:"*n + ")"*n)` | 37041 |

Recursão JS (jit ligado / `BUN_JSC_useJIT=0`, este último é o frame do LLInt/CLoop, o que o porte segue):

| caso | jit | sem jit |
|---|---|---|
| `f(n){return n?1+f(n-1):0}` | 54686 | 39905 |
| `f(n,a,b,c)` | 40037 | 35471 |
| `f(n)` com 10 `let` locais | 23436 | 24556 |
| getter recursivo | 78124 | 4372 (medida suspeita: o contador `this.n` roda no tier baixo) |
| via `f.call` / `f.apply` | 32225 | 31924 (call) |
| construtor `new C(n-1)` | 62499 | 39904 |

Os números com JIT variam com o tier em que a função está quando estoura; o alvo do porte é a coluna sem JIT.

## Custos derivados do parser (aplicado)

Orçamento = 5 MiB - 64 KiB = 5 177 344 bytes. Pontos de `failIfStackOverflow` por nível sintático:

- `(`: parsePrimary + parseExpression + parseAssignment = 3 pontos. 5177344 / (3503 * 3) = 492,7, logo
  `stack_cost::PARSER_LEVEL` = 493 (ponto de expressão).
- `if(1)`: 1 ponto (parseStatement). 5177344 / 5756 = 899,5. `{`: 2 pontos (parseStatementListItem + parseStatement),
  5177344 / 2879 / 2 = 899,2. Os dois concordam: `stack_cost::STATEMENT_LEVEL` = 899 (frame de `parseStatement` é
  maior que o do ciclo de expressão, por isso um custo único não serve).
- Sanidade: `function f(){` dá 843 por ponto, `a=>` 2120 por 2 pontos de expressão (+ corpo de arrow), `f(` 1349
  por nível (3 pontos de expressão e a lista de argumentos): variações de ±10% do frame real, aceitas.
- Previsão do porte: `(`: 5177344 / 493 / 3 = 3500; `if(1)`: 5758; `{`: 2879. Falta conferir rodando (cargo não
  foi rodado nesta passagem).

## Contas do tamanho de frame (2026-10-08, estático, cargo não rodado)

Oráculo (`BUN_JSC_useJIT=0 BUN_JSC_dumpGeneratedBytecodes=1`, `(0,eval)("function f(n){return n?1+f(n-1):0}")`):
`f` tem 16 callee registers e `call ... argc:2, argv:16`. Conta do C++: locais 0..3 são callee saves, `loc4` escopo,
`loc5` resultado, `loc6` callee; `CallArguments` com argc 2: `(5 + 2) % 2 != 0` logo argvSize 3, mais 1 do ajuste = 4
temporários (`loc7..loc9`, mais um `loc10` quando `(-index(argv[1]) + 5)` é ímpar, e é: `this` = `loc10`).
`stackOffset = -index(loc10) + headerSizeInRegisters = 11 + 5 = 16`; `numCalleeLocals` = 11 arredondado a 2 = 12, e o
`newTemporary` do `this` empurra o máximo a 16 (o dump manda). Custo por nível de recursão = 16 registradores
(o callee frame começa em `cfr - argv`).

Orçamento: 5 MiB / 8 = 655360 registradores; zona suave de 128 KiB = 16384; sobram 638976; 638976 / 16 = 39936,
e o bun mede 39905 (31 níveis a menos: os frames do programa e do eval). Já `45609 * 14 = 638526` mostra que a medida
antiga do porte equivale a 14 por nível, mas o porte confere com o C++ em todos os pontos lidos:
`CallArguments::new` (`nodes_codegen_cpp2.rs`), `HEADER_SIZE_IN_REGISTERS` = 5, `call_function` (`callee = cfr - argv`),
`frame_register_count_for` (`numCalleeLocals + 0`, `maxFrameExtentForSlowPathCallInRegisters` = 0 como em
`MaxFrameExtentForSlowPathCall.h`), `CLoopStack::grow` (soma a zona suave) e `Options` (5 MiB, 128 KiB). O golden de
bytecode (`tests/golden/bytecode_eval.txt`) já cobre `h`: 16 callee registers e `argv:16`. Logo a diferença de 14 contra 16
não está no tamanho do frame: a medição de 45609 foi feita antes da zona suave existir ou com outro limite. Precisa
remedir com o binário atual (esperado ~39930 para `f(n)`).

Causa achada do `call_edge`: `tests/call_edge_bun_golden.rs` chamava `Options::set_max_per_thread_stack_usage(64 MiB)`,
o que torna a pilha lógica 12,8 vezes maior e faz `f(100000)` terminar. Removido; o teste passa a usar os 5 MiB do bun.
Risco: a recursão mútua de 40000 níveis (que no bun com JIT cabe porque `g` é inlinada) pode passar a estourar no porte;
se acontecer, é fidelidade ao LLInt (sem JIT o bun estoura igual), e o caso se trata como divergência do JIT.

### Os outros casos no oráculo (2026-10-08, `BUN_JSC_useJIT=0 BUN_JSC_dumpGeneratedBytecodes=1`, função chamada uma vez)

Só o bloco da função `f#...` conta (o dump do eval e do `bun:main` não entram). Callee registers e `argv` do `call` recursivo:

| Fonte | callee registers de `f` | call recursivo | níveis medidos |
|---|---|---|---|
| `f(n)` | 16 | `argc:2, argv:16` | 39905 |
| `f(n,a,b,c)` | 18 | `argc:5, argv:18` | 35471 |
| `f` com 10 `let` | 26 | `argc:2, argv:26` (locais `loc5..loc14`, callee em `loc16`) | 24556 |
| `new F(n-1)` | 18 | `construct argc:2, argv:18` | 39904 |
| `f.call(null,n-1)` | 20 | dois `call` atrás de `jneq_ptr` (atalho de `Function.prototype.call`): `argc:2, argv:20` e `argc:3, argv:18` | 31924 |

Conferência de conta: 638976 / 18 = 35498 (bun 35471), 638976 / 26 = 24576 (bun 24556), 638976 / 20 = 31948 (bun 31924),
638976 / 16 = 39936 (bun 39905): em todos o custo por nível é `callee registers`, e a diferença de ~25 a 31 níveis é o
frame do programa e do eval. O `.call` NÃO vira `call_varargs`: é `call` comum, com o desvio `jneq_ptr` para o caso de
`f.call` não ser a `Function.prototype.call` original, e o frame vale 20 porque o caminho do atalho aloca `argv:20`.

Lado do porte, por leitura (cargo não rodado): `CallArguments::new` (`nodes_codegen_cpp2.rs`) confere linha a linha com
`NodesCodegen.cpp:1246` (`argvSize`, ajuste de alinhamento, `+1` do `stackOffset`, escolha do `argv` por `-index(argv[1]) + 5`),
e `CallFunctionCallDotNode` (`nodes_codegen_cpp3.rs`) emite o atalho com `emit_jump_if_not_function_call`. Não achei registrador
reservado a mais ou a menos por leitura. Para fechar com evidência, o golden `tests/golden/bytecode_eval.txt` ganhou os cinco
casos acima (`scripts/gen-bytecode-golden.js`, agora com cada função chamada uma vez para o bloco `f#...` aparecer): a próxima
rodada de `e2e_bytecode_golden` compara `callee register(s)` e `argv` do porte com o bun. Se algum falhar, a divergência está
ali, não no teto de pilha.

## Pendente (depende de medir o bun, não foi feito)

0. JS: o bun sem JIT dá 39 905 níveis para `f(n)` simples contra 45 609 do porte antes: o porte tem frame menor que o
   LLInt. Falta comparar `frame_register_count` com `numCalleeLocals` (+ `CallFrame::headerSizeInRegisters` = 5) para
   `f(n)`, `f(n,a,b,c)` e 10 `let`: o bun tem 5 MiB / 8 / 39905 = 16,4 registradores por nível em `f(n)`
   (5 de cabeçalho + argumentos com `this`, 2 + locais/temporários + ~4 de alinhamento/args do callee). Não feito.

1. Calibrar `PARSER_LEVEL`: achar no bun a profundidade em que o parser dá "Stack exhausted" (por exemplo `((((...))))`
   e `[[[[...]]]]`, blocos aninhados) e fixar `custo = logical_stack_limit() / profundidade`. O 512 é estimativa.
2. A profundidade JS depende do tamanho do frame em registradores (o porte emite frames possivelmente menores que o JSC
   em alguns casos). O bun reportado em ~10-13k para recursão sloppy simples contra 45 609 medidos antes para
   `function f(n){return n?1+f(n-1):0}` indica que o custo por nível varia com a forma da função; conferir o
   `frame_register_count` contra o `numCalleeLocals` do JSC naquela função no oráculo, não ajustar o teto.
3. Yarr: trocar `StackCheck` (512 KiB nativos) e o `MatchingContextHolder::COMPILER_THREAD_STACK_BUDGET` por contagem
   lógica (`enter_logical_frame`) com orçamento da pilha da thread do bun (8 MiB menos `reservedZoneSize`), nos pontos
   `isSafeToRecurse` do `YarrPattern.cpp` e do `YarrInterpreter.cpp`. Os sítios não têm guarda de escopo hoje; precisam
   de um `LogicalStackFrame` por nível.
4. Gerador de bytecode (`bytecode_generator_part2.rs`, `nodes_codegen_cpp7.rs`): ainda usa `is_safe_to_recurse()` (agora
   com o contador lógico, mas sem custo próprio). Acrescentar `stack_cost::CODEGEN_LEVEL` calibrado.
5. Os goldens `run_*_big_stack` podem manter a thread de 256 MiB; o orçamento nativo deve ficar bem acima do lógico
   (parser: 5 MiB / 512 B = ~10k níveis de ciclo, cada um alguns KiB nativos).
