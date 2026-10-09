# Auditoria de limites e robustez (2026-10-08)

## Golden

- `scripts/gen-limits-golden.js` gera `tests/golden/limits_bun.tsv` (1698 programas mantidos, 72 descartados por
  passar de 3 s ou não responder; cada programa roda duas vezes e instáveis são descartados).
- `tests/limits_bun_golden.rs` roda cada linha numa thread de 256 MiB e compara `R` com o bun. Não foi compilado nem
  executado (restrição desta tarefa): a primeira rodada vai mostrar as divergências reais.
- Resultado dominante no bun: `ok` (1132), `RangeError|Maximum call stack size exceeded.` (242), `RangeError|Out of
  memory` (27), `Invalid array length` (12). Os fontes grandes são montados dentro do programa (`"(".repeat(n)`),
  então a linha do golden é curta.

## Conferência do porte contra o upstream (só leitura)

- `VM::is_safe_to_recurse` compara o endereço de uma local com `stack_limit`, igual a `isSafeToRecurse(m_stackLimit)`.
  O limite é fixo em 1 MiB abaixo do ponto onde o `VM` nasce (`DEFAULT_STACK_BUDGET`), diferente do C++
  (`StackBounds::recursionLimit`, 5 MiB). Divergência conhecida e documentada em `vm.rs`; a profundidade em que o
  `RangeError` aparece não é igual à do bun, mas o golden só registra tipo e mensagem, nunca a profundidade.
- `json_object.rs`: `MAXIMUM_SIDE_STACK_RECURSION = 40000` igual a `maximumSideStackRecursion` de
  `JSONObject.cpp:343`, aplicado ao `holder_stack` (stringify) e aos `mark_stack` (walk/reviver), como nas linhas
  451, 1966 e 2052 do upstream.
- Guardas `is_safe_to_recurse` presentes: parser (`parser_part3.rs`), bytecompiler (`bytecode_generator_part2.rs`,
  `nodes_codegen_cpp6/7.rs`), interpretador (call, eval, módulo), `array_prototype.rs` (flat/join), `js_object.rs`,
  `proxy_object.rs` e yarr (pattern e interpreter).

## Profundidade de recursão: medição no bun 1.4.2 e orçamento do porte

Medido (`RangeError` na profundidade, contador no corpo; `/tmp/depth1.js`):

| caso | profundidade |
|---|---|
| função simples, método, arrow | 45609 |
| construtor (`new C(n+1)`) | 39905 |
| getter recursivo | 36915 |
| função com 5 parâmetros | 35473 |
| função com 20 locais | 18779 |

Em bytes de frame (5 MiB / profundidade): 115 (0 args), 148 (5 args), 279 (20 locais). A diferença de 20 locais é
164 B, ou seja 8 B por local: o limite do bun é a pilha de 5 MiB dividida pelo tamanho do frame.

O porte tinha três tetos: `Interpreter::MAX_NATIVE_DEPTH` = 10000 (cortava a função simples em 22% do bun),
`VM::DEFAULT_STACK_BUDGET` = 1 MiB de pilha NATIVA (a chamada JS para JS recursa em Rust: `llint_execute` +
`dispatch_loop`; com qualquer frame nativo acima de ~23 B por nível o limite disparava abaixo de 45609, na prática
bem antes dos 10000) e o `CLoopStack` de 5 MiB de registradores (esse sim equivale ao do bun e já faz a profundidade
cair com o tamanho do frame via `frame_register_count_for`; a paridade exata depende de o frame do porte ter 14
registradores com 0 args, não verificado sem rodar).

Ajustes (sem cargo, não compilados):
- `MAX_NATIVE_DEPTH` 10000 para 50000 (acima do bun; o limitante passa a ser o `CLoopStack`).
- `VM::set_thread_stack_budget(bytes)` (thread_local lido por `VM::new`): quem roda numa thread grande declara o
  orçamento nativo; o padrão continua 1 MiB. `tests/limits_bun_golden.rs` usa 256 MiB menos 16 MiB.

Cálculo da pilha de thread necessária: 45609 níveis x bytes nativos por nível de JS. O consumo por nível não foi
medido (proibido compilar): estimativa de 2 a 4 KiB em release e 8 a 16 KiB em debug (`dispatch_loop` com `match`
gigante costuma ter frame de KiB), ou seja 90 a 180 MiB em release e 360 a 730 MiB em debug. A thread de 256 MiB do
golden cobre release; em debug a recursão pode vir antes do `CLoopStack` com `RangeError` correto (pelo orçamento),
mas não deve ultrapassar a pilha. Primeira rodada: medir `sp` inicial menos `sp` em `enter_frame` a 1000 níveis
para obter o número real, e então fixar o orçamento em níveis x bytes x 1,25. JSON.stringify aninhado e parsers
recursivos em JS usam o mesmo orçamento; o stringify tem ainda o teto de 40000 (`maximumSideStackRecursion`).

### Estimativa por leitura do frame nativo por nível de JS (sem compilar)

Cadeia nativa por chamada JS para JS: `dispatch_loop_from` (o `match` do laço) -> `call_function` ->
`call_prepared_frame` -> `llint_execute` -> `dispatch_loop` -> `dispatch_loop_from`. Em debug não há inlining nem
compartilhamento de slots de pilha, então cada função soma todos os seus locais: `dispatch_loop_from` (uns 30 braços
com `OpXxx` decodificado, `SlowPathFrame`, `CallInfo`, `Step`) deve ficar em 1 a 3 KiB, `call_prepared_frame`
(`ErrorSite`, `Option<CodeBlockRef>`, `ScriptExecutableRef`, erros) em 0,5 a 1,5 KiB, `enter_frame` e `llint_execute`
em 0,3 KiB cada; total 2,5 a 6 KiB em debug (a estimativa anterior de 8 a 16 KiB era pessimista), 1 a 2 KiB em
release se nada embutir `run_ext`. Nenhum array nem struct grande por valor na cadeia: o que pesa é a quantidade de
locais de cada braço do `match`.

Risco real em release: `run_ext` (centenas de braços, chamada de um só ponto) e `call_function`/`call_varargs`/
`call_direct_eval` seriam embutidos em `dispatch_loop_from` e somariam os locais deles ao frame que fica empilhado a
cada nível. Aplicado (só ramos de chamada e `run_ext`, nenhum `Step::Jump`): `#[inline(never)]` em `run_ext`,
`call_function`, `handle_host_call`, `call_varargs` e `call_direct_eval`.

Medição: `tests/native_stack_depth.rs` roda `f(n) { return 1 + f(n + 1) }` com orçamentos de 16 e 64 MiB e imprime
`(B2 - B1) / (D2 - D1)` bytes por nível (sem a constante do prólogo; não há função nativa que exponha o `sp` ao JS,
então o orçamento faz o papel de régua). Rodar com `--nocapture`, em debug e em `--release`.

Ajuste do golden de limites: `THREAD_STACK_BYTES` 256 MiB para 1 GiB (reserva virtual, páginas sob demanda), que
cobre 45609 níveis x 12 KiB x 1,25 com folga; o orçamento do VM continua `THREAD_STACK_BYTES - 16 MiB`. Depois da
medição, fixar o orçamento em níveis x bytes x 1,25 e reduzir a thread. `DEFAULT_STACK_BUDGET` fica em 1 MiB de
propósito: a thread de teste do Rust tem 2 MiB, então o padrão não pode subir sem quebrar quem usa o VM numa thread
pequena; quem quer a profundidade do bun declara `set_thread_stack_budget`.

## Pendências para a primeira rodada

- Nenhuma correção de código foi aplicada: não achei divergência óbvia sem rodar o teste. Os candidatos mais
  prováveis de falhar são `Function.prototype.apply` com mais de 0x100000 argumentos (mensagem), `Out of memory` de
  `repeat`/`padEnd`/`ArrayBuffer`, e `eval` aninhado com mais de 1000 níveis.
- Se o `RangeError` não vier em vez de estourar a pilha nativa, aumentar `THREAD_STACK_BYTES` no teste antes de mexer
  no orçamento do VM.
