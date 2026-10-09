# call_edge: duas divergências do golden (2026-10-08)

## 1. Recursão mútua `f(20000)` dando RangeError

Premissa do chamado não se sustenta: no programa `try { ... 'use strict'; function f(n) {...} ... }` o
`'use strict'` está dentro de um bloco `try`, então é uma expressão e não uma diretiva. O código é sloppy, não há
tail call nem no bun nem no porte (`ConditionalNode` já propaga a posição de cauda, ver `nodes_codegen_cpp4.rs`).
O bun passa porque são 40000 níveis não-cauda e o JIT inlina `g` em `f`, encolhendo o frame.

No porte o limite que dispara é a pilha de registradores do CLoop (`maxPerThreadStackUsage` = 5 MiB, fiel ao C++),
não a nativa (256 MiB na thread do teste) nem `MAX_NATIVE_DEPTH` (50000). `f(20000)` isolado (20000 níveis)
passa; a mútua (40000) passa de 5 MiB com o frame cheio do LLInt.

Correção: o teste `tests/call_edge_bun_golden.rs` sobe `Options::set_max_per_thread_stack_usage` para 64 MiB na
thread. Não medido (sem cargo nesta rodada): se ainda estourar, a causa seria a pilha nativa por nível de
`llint_execute` e o passo seguinte é medir os bytes por nível.

## 2. `new (Date.bind(null, 2020))().getFullYear()` dá 1970 em vez de 1969

Era fuso. `new Date(2020)` são 2020 ms desde a época, 1970-01-01T00:00:02.020Z; em America/Sao_Paulo (-03, a
máquina do gerador) o ano local é 1969. O porte lia o fuso do processo. O teste agora chama
`set_time_zone_spec_override(Some("America/Sao_Paulo"))` na thread, como em `tests/date_edge_bun_golden.rs`.

## 3. Lentidão medida (735 s): leitura de 2026-10-08, sem medir

### (b) `Math.max(...Array(1000000).fill(1))`, 124 s até o RangeError

O limite de pilha em si está certo e é barato: a pilha de registradores (`CLoopStack`) tem
`maxPerThreadStackUsage` = 5 MiB = 655 360 registradores, então `callee_frame_for_varargs`
(`llint/varargs.rs`) devolve `None` para 1e6 argumentos e `size_frame_for_varargs` lança antes de
`load_varargs`. O custo estava ANTES, no `op_spread` (`llint/handlers_iterator.rs::spread`): o modo
`FastArray` passava por `for_each_in_iterable`, ou seja, `iterator_step` + `iterator_value` por elemento, com
um objeto `{value, done}` alocado por elemento. Corrigido: `FastArray` agora copia direto com
`JSArray::get_by_index` (o que o `JSCellButterfly::createFromArray` do C++ faz), com checagem de exceção por
índice. Semântica igual quando o protocolo do array está intacto (é a condição do modo).
Não confirmado por medição: se o resíduo vier de `Array(1e6)` (forma ArrayStorage, `fill` por
`put_by_index`, que é O(1) por elemento pela leitura) ou de `construct_array`/`create_from_array`.
Medir com `CALL_EDGE_LINES=335-338`.

### (d) corrigido por leitura (2026-10-08, segunda passagem, sem medir)

Causa: `Interpreter::capture_stack_for_exception` (`interpreter/unwind.rs`) rodava, a CADA `throw` (e um
`throw e` cria `Exception` nova, o `stack_captured` só protege a mesma `Exception`), dois percursos de pilha com
`StackFrame` eager (nome via propriedade `name` do callee, URL, linha/coluna, strings alocadas): um de até
`exceptionStackTraceLimit` = 100 frames para `Exception::m_stack` (que NINGUÉM lê; `Exception::stack()` não tem
leitor) e outro de `Error.stackTraceLimit` frames para o `ErrorInstance`, mesmo quando o erro já tinha pilha
(`set_pending_stack` então não fazia nada). Rethrow recursivo de ~1e4 níveis = ~1e6 resoluções de frame, o que
bate com os 32 s. No C++ o `m_stack` é barato porque `StackFrame` é preguiçoso.
Correção: `m_stack` fica vazio (só marca `stack_captured`; divergência de custo documentada no código) e o
percurso do `ErrorInstance` só roda se `!ErrorInstance::has_stack_info()` (novo). Sem mudança observável: a pilha
de `error.stack` sai dos mesmos frames de antes. Pendente de medir com cargo.
Se ainda for lento: o `new Error` por nível ainda anda `Error.stackTraceLimit` (10) frames eager; o passo seguinte
é tornar `StackFrame` preguiçoso (guardar `CodeBlockRef`+`BytecodeIndex`+callee e resolver na formatação).

### (a) yield* e (c) bind com 65000 args: lidos, nada algorítmico achado

`yield*` recursivo é quadrático também no C++ (cada `next` atravessa os n níveis de `generatorResume`; os locais
do generator vivem no escopo do `op_create_generator_frame_environment`, não há `op_save`/`op_resume` copiando o
frame). g(2000) = ~2M retomadas x ~4 chamadas = ~8M chamadas em 258 s = ~30 us por chamada em debug, ou seja
custo constante por chamada, não complexidade. Percorridos sem achar O(n): `call_prepared_frame`, `register_code_block`
(HashMap), `is_safe_to_recurse`, `enter_frame`. `bind`: `final_args`/`unwrap_bound_function_for_tail_call` copiam
os argumentos ligados uma vez por chamada (linear, igual ao C++); os 40 s devem estar em outro lugar (candidato:
`arguments_span` do `HostCall` ou a montagem de 65000 args em `apply`/varargs). Medir com perf antes de mexer.

### (e) `bind` com 65 000 argumentos (leitura de 2026-10-08, sem medir)

O caso é `f.bind(null, ...Array(65000))` e `g(...Array(65000))`. O caminho do `bind` em si é linear e
fiel ao C++: `function_proto_func_bind` copia os argumentos uma vez para `bound_args`,
`bound_function_call` monta um `Vec` com ligados + chamada por chamada (o `boundFunctionCall` do C++ faz o
mesmo com um `MarkedArgumentBuffer`), `name`/`length` são preguiçosos, e `unwrap_bound_function_for_tail_call`
só roda em posição de cauda (não é o caso). Não há O(n^2) nem construção por elemento ali.
O custo estava no `op_spread` de `Array(65000)`, que é todo buraco: `JSArray::get_by_index` cai em
`JSObject::get_property_slot_by_index`, e `get_own_property_slot_by_index` montava
`Identifier::from_u32` (atomização na tabela de átomos) para CADA objeto da cadeia (array, Array.prototype,
Object.prototype), em cada buraco, só para entregar a `string_object_own_slot`, que descarta o nome se o objeto
não é `StringObject`. Corrigido em `runtime/js_object.rs`: o nome só nasce quando o tipo é `StringObject` ou
`DerivedStringObject`. Isso corta ~3 atomizações por buraco (390 000 só neste caso, mais as 130 000 de
`...Array(65000)` na chamada) e vale para qualquer leitura de buraco de array grande.
Não medido (sem cargo). Se ainda sobrar tempo, o próximo suspeito é `Array(65000)` em forma ArrayStorage e
o `put_arguments_specials`/`DirectArguments::create_by_copying` de 130 000 argumentos (lineares, uma vez).

### (a), (c), (d): NÃO localizados por leitura (texto da primeira passagem)

Percorridos sem achar custo por chamada acima de uma ordem de grandeza: `call_prepared_frame`,
`llint_execute`, `enter_frame` (Rc clone do global object e do CodeBlock, O(1)), `dispatch_loop_from`
(`instructions().at(pc)` clona um `Rc`, `set_current_vpc` e `set_top_call_frame` por instrução, O(1)),
`op_enter` (zera `num_callee_locals`, linear no tamanho do frame). 100 µs por retomada em debug não vem desses.
Suspeitos que ainda faltam ler, em ordem: o `generatorResume` (`Generator.js`) e como a retomada salva e restaura
os registradores do generator (`op_save`/`op_resume`, cópia do frame inteiro por yield), a captura de stack
trace em `create_error` (por `throw`, relevante em (d): 32 s para ~1e4 níveis é ~3 ms por nível, típico de
captura de stack O(profundidade) a cada rethrow, portanto quadrático) e o `bind` com 65 000 argumentos (cópia de
`bound_args` em cada `call`, `unwrap_bound_function_for_tail_call`). Próximo passo: perfil com `perf` sobre o
binário de teste em release nos casos 859-898 e nos de generator, antes de mexer.
