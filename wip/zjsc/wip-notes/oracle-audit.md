# Auditoria do golden esnext contra o bun (314 de 2456 divergem)

Fonte: `/tmp/now3_esnext_bun_golden.txt` (teste `esnext_apis_match_bun`). Nada foi compilado nem rodado nesta
passagem (cargo proibido): as correções abaixo estão conferidas só contra o C++ em `upstream/`.

## Causas, por número de casos

| Casos | Causa | Tipo | Estado |
|---|---|---|---|
| 187 | `LinkTimeConstant AsyncFromSyncIteratorCreate não inicializada` (pânico em `Array.fromAsync` com iterável síncrono) | API ausente | corrigido |
| 92 | `Map.groupBy` lança `TypeError: undefined is not a function` | indeterminado, suspeita de contaminação | não mexi no código |
| 15 | `Promise.withResolvers`: `r.promise` vem `undefined`, chaves saem `resolve,reject,Promise` | valor/nome | corrigido |
| 11 | programa do golden é JS inválido (`.[...take(2)]`, `(class It extends Iterator {} ; ...)`) e o bun gravou `sync-throw SyntaxError: ...`; o teste lia `R` e via `<undefined>` | harness do teste | corrigido no teste |
| 3 | `Iterator.from(o).return()` com `PrivateSymbol.assert` | derivado da causa 1 ou do `wrapForValidIterator` | não investigado |

## 1. `asyncFromSyncIteratorCreate` nunca instalado (187)

`function_kind_intrinsics.rs` documentava que o `LinkTimeConstant::asyncFromSyncIteratorCreate` não era instalado
("pertence a outro arquivo do C++"), mas `Array.fromAsync` (`builtins_combined.js`, `@asyncFromSyncIteratorCreate`)
o lê e o `JSGlobalObject` entra em pânico. C++: `JSGlobalObject.cpp:1944` (`JSFunction::create(..., 1,
"asyncFromSyncIteratorCreate", asyncFromSyncIteratorCreatePrivate, Private, NoIntrinsic)`) e
`IteratorOperations.cpp:439` (`syncIterator` precisa ser objeto, senão `TypeError: Only objects can be wrapped by
async-from-sync wrapper`; depois `createAsyncFromSyncIterator`).

Correção: `create_async_from_sync_iterator_create_function` em `iterator_operations.rs` (junto do
`create_async_from_sync_iterator`, como no C++) e instalação em `install_function_kind_intrinsics`. Diferença
assumida: o C++ usa `initLater`, aqui nasce junto do global, igual aos demais `LinkTimeConstant` do arquivo.

## 2. `Map.groupBy` (92): NÃO é bug do `Map.groupBy` até prova em contrário

O `Map.groupBy` está instalado (`js_global_object_init.rs`), usa só `@Map`, `@get`, `@set` (todos presentes no
`MapPrototype`) e os outros golden (`collection_async_bun_golden`, `collection_mutation_bun_golden`,
`recent_apis_bun_golden`) cobrem a mesma API. A falha só aparece neste teste, que roda os 2456 programas na MESMA
thread, um `VM` novo por caso, com `catch_unwind` em volta. Os 187 pânicos da causa 1 desenrolam a pilha no meio do
interpretador e deixam estado de thread (registro de células, pilha de frames, orçamento de pilha) sujo para os
casos seguintes: hipótese de contaminação por pânico, não confirmada. Teste a verificar depois do rebuild: se os 92
sumirem sem tocar no `Map.groupBy`, era contaminação. Se sobrarem, rodar um deles isolado
(`evaluate_script_sequence_result` com o programa só) antes de mexer no código. Nada foi alterado.

## 3. `Promise.withResolvers` (15)

`createPromiseCapabilityObjectStructure` (`js_promise_capability.rs`) usava `vm.property_names.promise`, que no Rust é
o `Promise` com P maiúsculo (`macro(Promise)` do `CommonIdentifiers.h`); o C++ usa `vm.propertyNames->promise`, o de
minúsculo, que no Rust é `promise_dup`. Resultado: `r.promise` era `undefined`, o que quebrava também `r.promise.then`,
`.catch`, `.constructor` e `Promise.all([r.promise, ...])`. Trocado para `promise_dup`. O `Object.keys(r)` agora sai
`resolve,reject,promise`.

## 4. Harness do teste (11)

`scripts/gen-esnext-golden.js` roda o programa em `try { runInThisContext(...) } catch (e) { R = 'sync-throw ' +
name + ': ' + message }`. Onze programas do gerador são sintaticamente inválidos (typo `.[...take(2)]`, e uma
classe seguida de `;` dentro de parênteses); o bun gravou o SyntaxError. O teste Rust ignorava o `errors` de
`evaluate_script_sequence_result` e lia só `R`. `run()` agora devolve `sync-throw Nome: mensagem` quando o script
lança, o mesmo formato do gerador. Se a mensagem do parser divergir do bun, aparece como divergência real de
SyntaxError no próximo rodar. Os programas inválidos continuam no golden (são oráculo de mensagem de parse); se
forem consertados no gerador, regravar.

## Contaminação do golden por globais mutados

Não encontrei programa que mute globais e afete o seguinte: cada caso tem `VM` e global próprios
(`new_global_object`). A única contaminação suspeita é a de pânico (item 2), que é de thread, não de global JS.

## Lote 3: number_edge, ctor_this, weak_more (nenhum código alterado)

Os três outputs foram gerados de um snapshot defasado (`target-zjsc-snap/zjsc2`); `diff -rq` mostra dezenas de
arquivos diferentes do fonte atual.

- **weak_more (2) e ctor_this `Map.groupBy` (1): já corrigido no fonte.** O snapshot ainda tem
  `define_with_private(&names.set, builtin_names.set_private_name(), ...)` em `map_prototype.rs`; o fonte atual usa
  `set_dup_private_name()`. `groups.@set(...)` no `Map.groupBy` dava `undefined is not a function`. Revalidar no
  próximo snapshot.
- **ctor_this `Promise.withResolvers` (2): já corrigido** (`promise_dup` em `js_promise_capability.rs`, item 3 acima).
- **ctor_this `Array.of`/`Array.from` com `this` construtor (pânico e `B.of`): defasado.** A mensagem do pânico
  (`Array.of com this construtor que não é Array`) não existe mais em `host_function_support.rs`; revalidar.
- **number_edge (4): dado de ambiente, sem mudança.** Só o sinal do NaN difere (`7ff8...` contra `fff8...`) em
  `Math.acos(-Infinity)` e `Math.log1p(x < -1)`. O bun gera `acos` positivo e `log1p` negativo (medido); o libm do host
  decide o bit de sinal do NaN, que o JS não observa fora de `Float64Array`/`BigUint64Array`. Fica fora do alvo.
- **ctor_this, pendentes reais (não corrigidos, tempo esgotado), com causa provável:**
  1. `Proxy(function f(a){}).bind(null)` dá `length` 0 e não dispara o trap `getOwnPropertyDescriptor`: em
     `function_proto_func_bind` (`function_prototype.rs`) o `target.has_own_property` não passa pelo trap do
     `ProxyObject` (esperado `g:bind,d:length,g:length,g:name`).
  2. `Array.bind(null, 3)` + `new B()`: `get_function_realm` (`internal_function.rs:299`) recebe valor que não é objeto
     no caminho `construct` de `JSBoundFunction` quando o `new.target` é a própria bound.
  3. `f.apply(null, function(a,b){})` deve dar 2 (função como array-like): é `varargs.rs`, de outro agente.
  4. Mensagens de erro de campo privado e TDZ de classe (`evaluating 'super(...args)'`, `near '...'`,
     `Cannot access '' before initialization`): divot e nome de variável do bytecode generator, de outra frente.
  5. `structuredClone` ausente (global do Bun, não do JSC): golden contaminado por ambiente, descartar o caso.
  **Correções posteriores (sem cargo, a validar no próximo build):**
  - Item 1: `function_proto_func_bind` agora consulta `length` por `proxy_object::own_property_slot` (o despacho
    de `methodTable()->getOwnPropertySlot`), então o `Proxy` dispara o trap `getOwnPropertyDescriptor`.
  - Item 2: a causa não era `get_function_realm` (ele já segue `JSBoundFunction`, `JSRemoteFunction` e `Proxy`). O
    `Array` registrava o mesmo corpo para chamada e construção, e o `newTarget` lia o `this` do quadro: em
    `B()` com `Array.bind(null, 3)` o `this` é `null`, que não é vazio nem `undefined`, e caía no
    `get_function_realm(null)`. Agora `array_constructor_call_host` passa `JSValue::empty()` (como
    `callArrayConstructor`), via `run_array_function_with_new_target`; `is_array_constructor` compara com ele.
    Outros construtores nativos que dividem corpo entre chamada e `new` e lêem `call_frame.new_target()` podem ter o
    mesmo defeito: conferir.
  - Item 6 (slice com proxy): `put_direct_index_slow_or_beyond_vector_length` ia pelo `define_own_indexed_property`
    ordinário para um `Proxy`; `define_own_index_property_by_class` agora despacha para
    `define_own_property_from_proxy` (trap `defineProperty`, mensagem `Proxy's 'defineProperty' trap returned falsy
    value for property '0'` já existia em `perform_define_own_property`).
  - Item 6 (`Symbol.species` em `B.from`/`B.of`/`new B(1)`): não localizei leitura de `@@species` nesses caminhos
    do Rust (`array_constructor_of`, `construct_array_with_size_quirk`); fica pendente, pode ser snapshot defasado
    ou o builtin JS de `from`. Revalidar após o build.
  6. `Array[Symbol.species]` não deve ser lido por `B.from`/`B.of`/`new B(1)` (esperado 0 chamadas) e o `slice` com
     proxy `defineProperty` falso deve citar `Proxy's 'defineProperty' trap returned falsy value for property '0'`.

## Lote 3: locale_methods e proxy_reflect (2026-10-08)

`locale_methods_bun_golden` (60 de 1513) agrupado por causa:

- Dado de ambiente (ICU do bun tem CLDR completo, o porte tem o recorte do icu4x compilado): nomes de fuso
  (`Coordinated Universal Time` em de/ja/ar/hi/tr), era (`n. Chr.`, `西暦`), calendário japonês e islâmico
  localizados, mês curto em de/hi, `dayPeriod`, `fractionalSecondDigits` com vírgula, unidades
  `km/h`/`litros` em pt, `Reiwa`. Não é bug de lógica; fecha com mais dados, sem mexer no código agora.
- Dado de ambiente: `currencySign: 'accounting'` em pt-BR e tr-TR e `currencyDisplay: 'name'` (`Euros`, `€`):
  o ICU do bun não tem o padrão contábil nesses locales. Medido: `-US$ 1.234.567,90`.
- Corrigido: `ar-EG` percent com marca de letra duplicada (`٪؜؜`, 5 casos): `percent_parts` mantinha o `U+061C`
  do padrão além do que já vem no sinal; agora a marca do padrão sai sempre.
- Corrigido: `numberingSystem: 'arab'` em locale sem símbolos árabes próprios (en, de, ja, pt, tr, hi; ~9 casos):
  o ICU cai nos símbolos do `root` (`٬`, `٫`, menos com `U+061C`); `DecimalFormat` agora faz o mesmo.
- Aberto: `ar-EG` com `numberingSystem: 'deva'` (símbolos latinos do root com LRM no menos), `Å`.localeCompare
  (equivalência canônica no collator), mensagem de `Array.prototype.toLocaleString.call(null)` sem o
  `(evaluating ...)`.

`proxy_reflect_bun_golden` (21 de 1795):

- Corrigido: `hasOwnProperty` e `propertyIsEnumerable` não chamavam o trap `getOwnPropertyDescriptor` do Proxy
  (8 casos); agora passam por `own_descriptor` (`object_constructor`), que já despachava o Proxy.
- Aberto, 7 casos: pânico `i < storage.num_values_in_vector()` em `js_object.rs` ao fazer `o['0'] = 1` com
  protótipo Proxy e `set` com receptor que é o objeto (caminho de `put_by_index` sem storage de vetor).
- Aberto: `Reflect.ownKeys` de Proxy sobre função vazio (falta materializar `length/name/prototype`),
  `Object.freeze(proxy de função)` (idem), `Reflect.apply(Array, null, [3])` (realm de `null`),
  `Function.prototype.bind` sobre Proxy sem `getOwnPropertyDescriptor:length`/`get:length`, `Array.of` com
  `this` não Array (pânico em `host_function_support.rs:312`).

Não rodei cargo (ordem do lote): as duas correções estão por compilar e testar.

### Lote 3, continuação: os dois pânicos (sem cargo, por compilar)

- `Array.of` com `this` construtor que não é `Array`: o pânico de `host_function_support.rs:312` era o
  `PutError::Unported` que `array_constructor_of` devolvia. Agora segue `arrayConstructorOf`: `construct(this,
  [length])` com a mensagem `Array.of did not get a valid constructor`, `putDirectIndex` estrito por argumento e
  `setLength` no fim (`array_constructor.rs`; `set_length`, `create_data_property_at` e `array_error_from_llint` de
  `array_prototype.rs` passaram a `pub(crate)`). `Array.from` é builtin JS (`ArrayConstructor.js`), não passa por aqui.
- `o['0'] = 1` com protótipo Proxy: o ramo que assumia slot existente era o `ARRAY_STORAGE_SHAPE` de
  `set_index_quickly` (o `o` vira `SlowPutArrayStorage` em `putByIndexBeyondVectorLength`, o `put` do Proxy volta no
  receptor com slot vazio). O outro agente já trocou o `debug_assert` pelo `was_empty` de
  `setIndexQuicklyForArrayStorageIndexingType`, que é o C++; conferi `put_by_index`,
  `put_by_index_in_array_storage_vector` e `put_by_index_beyond_vector_length` e batem com o JSObject.cpp.
  Nenhuma edição adicional; revalidar os 7 casos no próximo snapshot. Obs.: o `debug_assert!(i <
  storage.num_values_in_vector())` restante é o de `initialize_index` em `js_array.rs`, correto lá.
- Nota de processo: as três trocas de visibilidade em `array_prototype.rs` saíram por script Python (substituição
  simples em lote), não por Edit.

## call_edge_bun.rs: auditoria de leitura do travamento (2026-10-08)

Sem cargo. Conferido contra o C++: `Interpreter::MAX_ARGUMENTS = 0x100000` (Interpreter.h:208), o
`size_of_varargs` e o `size_frame_for_varargs` de `src/llint/varargs.rs` já lançam o RangeError acima do
limite, e a pilha de 5 MiB (655360 registradores) faz `ensure_capacity_for` falhar para 1e6 argumentos
(linhas 335 e 338 do tsv). `{length: 1e7}` (linha 312) falha rápido em `size_of_varargs`. Nenhum
laço sobre o `length` inteiro antes da checagem nesses caminhos.

Não achei por leitura um laço infinito ou quadrático definitivo. Suspeitos restantes, em ordem:
1. Recursão profunda (linhas 859 a 898, f(100000) e f(1000000)): `get_stack_trace` pula frames em cauda e
   builtins privados sem contá-los no `limit`, então o percurso do `StackVisitor` pode cobrir a pilha
   inteira (linear, mas uma vez por erro) e vale medir o tempo por programa.
2. `Array(n).fill` e `push.apply` com 1e5 a 1e6 elementos (linhas 333 a 341): `put_index` genérico por
   elemento; `count_elements` em `put_by_index_beyond_vector_length_without_attributes` é O(n) mas só
   nos pontos de crescimento (fator 1,5), então amortizado.
3. `op_spread` de 1e6 elementos materializa tudo antes de checar o limite (linear, sem corte como o JSC).
Próximo passo: rodar o tsv linha a linha com timeout por programa para isolar o culpado.

## Auditoria de call vs construct nos construtores nativos (2026-10-08, sem cargo)

No quadro nativo `newTarget()` é o slot do `this`, então um corpo de chamada que lê `new_target()` confunde
`this` com new.target (o defeito do `Array`). Conferi cada construtor contra o `.cpp`:

- Já corretos (funções `call`/`construct` separadas, e o corpo de chamada não lê `new_target()`): Object
  (`call_object_constructor` / `construct_with_object_constructor`, `None` na chamada), Function e as variantes
  Generator/Async/AsyncGenerator (macro com `None` na chamada), Error e as nativas (`call_error` usa a estrutura do
  realm, `construct_error` deriva de `newTarget`), AggregateError, SuppressedError, RegExp (`new_target_of` devolve
  `None` na chamada), Date (`callDate` devolve a string), String, Number, Boolean, Symbol (construct lança
  not-a-constructor), BigInt, Proxy, Map, Set, WeakMap, WeakSet, WeakRef, FinalizationRegistry, DisposableStack,
  ArrayBuffer/SharedArrayBuffer, DataView, TypedArrays concretos, Temporal.*, Intl.Locale, PluralRules, ListFormat,
  RelativeTimeFormat, DisplayNames, Segmenter, DurationFormat (chamada lança `cannot be called as a function`).
- `%TypedArray%`: `constructTypedArrayView` serve a chamada e a construção também no C++ (idêntico). Promise é
  `JSFunction` sobre builtin JS, não tem par nativo.
- Defeito achado e corrigido: `Intl.Collator`, `Intl.NumberFormat` e `Intl.DateTimeFormat` registravam o mesmo
  corpo (`construct_*`) como `call` e `construct`, e `derived_structure` lia `call.new_target()`. Numa chamada
  sem `new` isso é o `this`: `Intl.Collator()` (this = objeto `Intl`) ou `Intl.Collator.call({})` derivavam a
  estrutura de um `new.target` falso. Agora há `call_collator`, `call_number_format` e `call_date_time_format`
  (`callCollator` etc.), via `intl_support::call_instance` e `collection_support::callee_structure`, que usam o
  `prototype` do próprio construtor e nunca leem `newTarget()`. `derived_structure` passou a reaproveitar o
  mesmo helper (`callee_base_structure`).
- A validar no próximo build: `Intl.Collator.call({}) instanceof Intl.Collator`, `Intl.NumberFormat.bind(null)()`.

## Lote 3: ShadowRealm (58 de 960) e Wasm JS API (52 de 589)

ShadowRealm, causa dominante (cerca de 45 dos 58): `JSRemoteFunction` deixava a exceção do alvo escapar
crua. O bun a converte num `TypeError` do reino de quem chama: primitivo vira a conversão em string
(`throw 1` dá "1", `null` dá "null"), `Symbol`, `Proxy`, função, array e objeto comum (inclusive com `message`
próprio) dão "Type error", instância de `Error` leva a mensagem dela, e mensagem vazia vira "Type error".
Corrigido em `js_remote_function.rs` (`cross_realm_throw`, medido com `bun`).
- `new Proxy(function(a,b){}, {}).length` dava 0: `copy_name_and_length` chamava o `getOwnPropertySlot` de
  `JSObject`, que não despacha para `ProxyObject` (virtual no C++). Agora despacha pelo trap.
- `evaluate("throw new Error('')")` entrava em pânico no `debug_assert!(!message.is_empty())` de
  `error.rs`: o `ErrorInstance` do C++ só grava `message` quando não vazia. `create_error_instance` idem.
- A validar no próximo build: `Error` com getter de `message` (o bun dá "Type error"; `instance.message()`
  pode ler o valor); `evaluate` que lança objeto com `message` (a cópia de `createTypeErrorCopy` ainda lê).

Wasm JS API: nada alterado (as 52 divergências são quase todas `Thrown::Unported` de referência não nula,
GC sem instância, importação com resultado múltiplo e `v128`, mais o overflow de `js_web_assembly.rs:1044`,
tudo na área do agente wasm_gc). Mensagens a conferir depois: `new WebAssembly.Module(1)` ("first argument
must be an ArrayBufferView or an ArrayBuffer") e `new WebAssembly.Memory({})` (deve lançar TypeError
"Expect an integer argument in the range: [0, 2^32 - 1]"). Ordem das promessas na instanciação assíncrona
(`m1`/`p`, `imp2`/`after`) também diverge.

## Desempenho do golden de mensagens de erro (now4, 2026-10-08)

`error_message_matches_bun` levou 316 s para 1732 programas (média de 0,18 s por caso, em build de depuração). Não
medi por caso (sem cargo). Suspeitos, por inspeção do TSV (`tests/golden/error_message_bun.tsv`, linhas 883 a 910):
22 programas de estouro de pilha (`f(1e6)`, `1+f(1e7)`, `JSON.stringify` aninhado a 1e5, `eval('('.repeat(1e5))`,
`new Function` com 1e5 parênteses, regex `/(?:a|b)*/` sobre `'a'.repeat(1e6)`) e `apply` com `new Array(1e6)`
(`Math.max`, `String.fromCharCode`, `concat`). Em depuração o laço até o limite de pilha e o `fill` de 1e6 custam
muito; o custo anormal provável é o estouro de pilha (cada quadro em `debug` é grande e o limite só dispara
tarde). Próximo passo ao liberar cargo: rodar com `--nocapture` e cronometrar por linha para separar estes 22 do resto
antes de mexer em limite de pilha. Hipótese, não medida.

## now4: object_edge (1 de 1834) e destructuring (2 de 1350)

object_edge, `Object.fromEntries(new URLSearchParams('a=1&b=2')).b`: contaminação do golden. `URLSearchParams`
é API da web (WHATWG URL), não do ECMAScript; o bun a traz do runtime dele e o JavaScriptCore puro (o `jsc`
do Debian, o nosso alvo) não a define. A linha foi removida de `tests/golden/object_edge_bun.tsv` (agora 1833;
removida com `sed -i` numa linha de TSV gigante, por impraticabilidade do Edit). Nenhuma mudança de código.

destructuring, `[o.x] = mk([1])` com setter que lança (com e sem `return` que lança): o bun chama o
`return` do iterador (IteratorClose na conclusão abrupta, ECMA-262 8.6.2 passo 6), nós não (`log` sem `ret`).
Bug real, ainda NÃO corrigido. Conferido contra o upstream, sem divergência: `ArrayPatternNode::bindValue`
(`nodes_codegen_cpp6.rs`), `AssignmentElementNode::bindValueCanThrow` (retorna `true` para `o.x`),
`emitTryWithFinallyThatDoesNotShadowException` (`bytecode_generator_cpp5.rs`), `unwind` e o `storePC` por
instrução (`dispatch.rs`). Logo o defeito está depois do codegen: a hipótese é o caminho de exceção de
`put_by_id` com setter (`slow_path_put_by_id` / `put_to_object`) deixando a exceção pendente e só a
sinalizando num pc fora da faixa `[try_start, try_end)` do handler sintetizado, ou o `done` não ser relido
no handler. A validar com `bytecode_dump` de `try { [o.x] = it } catch {}` com setter lançando e
`try { try { o.x = 1 } finally { log } }` (se este também falha, é o caminho de `put_by_id`).

## Custo do estouro de pilha e de apply/spread gigantes (lido, não medido: cargo proibido)

Pergunta: por que `error_message` levou 316 s (1732 programas) e `call_edge` passou de 4 min.

Conferido e sem custo O(profundidade) por erro:
- `Interpreter::get_stack_trace` (`unwind.rs`) para no `limit` (`IterationStatus::Done` quando `frames.len() >= limit`),
  como o `Interpreter::getStackTrace` do C++. `capture_stack_for_exception` usa `exceptionStackTraceLimit` (100) e
  `capture_frames` usa `Error.stackTraceLimit` (padrão 100): duas passadas de no máximo ~100 frames cada, e a
  `Exception` só captura uma vez (`stack_captured`). O custo é O(limite) por erro, não O(profundidade).
  Só continua além do limite quando `caller` não está na pilha (`Error.captureStackTrace(obj, fn)` com `fn` ausente), igual ao C++.
- `ensure_capacity_for`/`grow` (`cloop_stack.rs`) são O(1); `is_safe_to_recurse` compara um endereço local.
- `size_of_varargs` (`varargs.rs`) já lança `StackOverflow` quando `length > MAX_ARGUMENTS` (0x100000) antes de
  alocar ou copiar; `enter_callee` faz o mesmo com `args.len()`.

Achados corrigidos (causas plausíveis de travar por minutos):
1. `Reflect.apply`/`Reflect.construct` (`list_from_array_like`, `proxy_object.rs`) só limitava a lista a `u32::MAX`
   itens: `Reflect.apply(f, null, {length: 2**32 - 1})` fazia 4 bilhões de `[[Get]]` com identificador novo por
   índice. No C++ o `@apply` cai em `op_call_varargs`, que lança `RangeError` por `maxArguments`. Agora o `length`
   é conferido antes do laço contra `Interpreter::MAX_ARGUMENTS` (parâmetro `max_length` de `for_each_in_array_like`;
   o trap `ownKeys` do Proxy segue sem teto, `u64::MAX`).
2. `slow_path_spread` (`handlers_iterator.rs`) percorria o iterador de um `JSArray` esparso de `length` até 2^32 - 1
   antes de `JSCellButterfly::createFromArray` rejeitar com `OutOfMemoryError`. Agora um `JSArray` com
   `length > JSCellButterfly::MAXIMUM_LENGTH` falha antes, com o mesmo erro (`MAXIMUM_LENGTH` virou `pub`).
   O C++ tem `trySpreadFast` como atalho para arrays densos; o porte não o tem, o que fica como diferença de custo (não de resultado).

Hipótese principal ainda não medida para os 316 s (média de 0,18 s por programa): custo fixo por programa de criar o
`VM` e o realm (pilha de registradores de `max_per_thread_stack_usage` zerada em `CLoopStack::new`, builtins
parseados a cada VM), mais recursão até `set_thread_stack_budget` (dezenas de milhares de níveis nativos). Para
confirmar, medir um programa vazio versus um de recursão infinita, com cargo liberado.
Nada foi compilado: conferir `cargo check` em `proxy_object.rs` e `handlers_iterator.rs` quando o cargo for liberado.

## now4: esnext_bun_golden (3 de 2456) e regexp_edge_bun_golden (1 de 2542)

- esnext (3 casos de `Iterator.from(...).return()/next()`): bug do porte, não do oráculo. O `WrapForValidIteratorPrototype`
  usa `@assert(...)`, e `FunctionCallResolveNode::emitBytecode` só descarta a chamada quando `!ASSERT_ENABLED`. O porte
  usava `!cfg!(debug_assertions)`, que no perfil `test` vale falso, então a chamada ficava e o `assertCall` (que o porte
  deliberadamente não instala, ver `js_global_object_static_globals.rs`) dava `ReferenceError: Can't find private
  variable: PrivateSymbol.assert`. Correção em `src/bytecompiler/nodes_codegen_cpp2.rs`: usar `options::assert_enabled()`
  (sempre falso, a regra do porte), igual ao C++ de release. Não compilado nem rodado (cargo proibido nesta rodada).
- regexp_edge (1 caso, `RegExp.$1+RegExp.lastMatch+RegExp.input` sem exec prévio): contaminação do gerador. O `bun -e`
  isolado devolve `""`; o esperado do tsv (`"T(T(()=>...)"`) veio do estado estático do `RegExp` deixado por um
  programa anterior do mesmo processo do gerador. Caso removido de `scripts/gen-regexp-edge-golden.js` e de
  `tests/golden/regexp_edge_bun.tsv` (2541 linhas). O caso com `exec` explícito antes (`/(a)(b)/.exec`) já cobre os
  acessores estáticos.
