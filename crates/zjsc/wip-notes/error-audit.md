# Auditoria das divergências de mensagem de erro (2026-10-08)

Fontes: `/tmp/now_class_edge_bun_golden.txt` (175 de 1310) e `/tmp/now_error_message_bun_golden.txt` (149 de 1765).
Nada foi compilado nem rodado (sem cargo, a pedido). As correções abaixo estão por conferir no próximo build.

## Raiz 1: private (`o.#p`, `o.#p = 2`, `o.#p()`, `typeof o.#p`) sem ` (evaluating '...')`

O `ExpressionInfo` e o `append_source_to_error_message_in_block` estão corretos. O erro nascia sem passar pelo site:

- `handlers_object.rs`: `to_object_ref` chamava `value.to_object(global)` direto, que lança o `createNotAnObjectError`
  sem site para `undefined`/`null`. Era também uma função de repasse. Removida; `get_private_name` e
  `put_private_name` chamam `to_object_for_access` (`slow_paths_object.rs`), que usa `throw_not_an_object` com o site.
- `createRedefinedPrivateNameError`, `createPrivateMethodAccessError` e `createReinstallPrivateMethodError`
  (`ExceptionHelpers.cpp`) usam `defaultSourceAppender`; o porte lançava a mensagem pura com `throw_type_error`.
  Novos: `create_type_error_with_default_appender` (`exception_helpers.rs`, o molde dos quatro erros fixos, e
  `create_invalid_private_name_error` passou a usá-lo) e `throw_default_appended_type_error` (`slow_paths_object.rs`).
  Usados em `put_private_name` (redefine), `set_private_brand` e `check_private_brand`
  (`handlers_private_brand.rs`, que também passou a `to_object_for_access`).

## Raiz 2: erro criado dentro de função nativa sem o trecho do fonte (`Object.keys(null)`, `a.set(null)`)

No C++, `ErrorInstance::finishCreation` lê o `topCallFrame` e apende o texto da instrução JS que chamou o nativo.
O porte só apendava com `site` explícito, e o nativo não tem site. Correção:

- `VM::native_call_site` (`vm.rs`, com `replace_native_call_site` e `native_call_site`): `CodeBlockRef` mais
  `BytecodeIndex` da chamada em curso.
- `dispatch.rs` (`handle_host_call`): guarda o site antes de `invoke_native` e restaura depois.
- `exception_helpers.rs`: `create_error_for_value` e `create_type_error_with_default_appender`, quando `site` é
  `None`, caem em `append_native_call_site`.

Cobre as ~27 divergências `null/undefined is not an object` sem sufixo. Risco a vigiar: um nativo que chama JS
de volta e depois lança vê o site restaurado (correto); `execute_call.rs:162` (nativo chamado por nativo) mantém
o site do JS externo, como o `topCallFrame`.

## Famílias de outra raiz, NÃO corrigidas

- `Type error` (39, spread `[...1]`, `Math.max(...{})`, `f(...true)`): `iterator_operations.rs` e `set_prototype.rs`
  lançam a literal "Type error" no lugar das mensagens do bun: "Spread syntax requires
  ...iterable[Symbol.iterator] to be a function" (o JS do `builtins_combined.js` já a tem, então o `op_spread`/
  `op_spread` nativo está tomando o caminho de `iteratorForIterable` em vez do builtin) e
  "Iterator result interface is not an object" sem o ponto final (bun difere do upstream aqui, conferir no
  oráculo antes de mudar a constante).
- Globais ausentes (`setTimeout`, `URL`, `atob`, `structuredClone`, `TextDecoder`, `queueMicrotask`...): são do
  runtime do bun, não do JSC. O oráculo ("bun") as tem; decidir se o harness deve filtrar esses casos.
- `Error.captureStackTrace(1)`: bun diz `invalid_argument`, o JSC upstream diz outra coisa (comportamento do bun).
- `Function.prototype.bind()` como alvo de `instanceof`: esperado `ok`, veio "invalid prototype property"
  (bound function sem `prototype` deve cair no `[[HasInstance]]` da bound).
- class_edge: `typeof`/`name`/`toStringTag` de classes (`"A"` no lugar de `"g"`/`"z"`, `"string"` no lugar de
  `"function"`): nome da classe com `static name`/computed e `Object.getOwnPropertyDescriptors(A)`; investigar à parte.
- "The value of the superclass's prototype property is not an object or null." (veio "undefined is not an
  object or null"): mensagem de `createInvalidPrototypeError` de `extends`.
- Panics a corrigir antes de qualquer conclusão: `function_executable.rs:601 RefCell already borrowed`
  (centenas de vezes), `host_function_support.rs:309 índice acima de MAX_ARRAY_INDEX`,
  `internal_function.rs:294 object.is_callable()`.

## Segunda passada (sem cargo, por conferir no build)

- `function_executable.rs` (`to_string_slow`, ramo de classe): `cache(js_string(&vm, &this.borrow().class_source().view()))`
  mantinha o `borrow()` temporário vivo até dentro do `cache`, que faz `borrow_mut`. Corrigido calculando o
  `js_string` antes (o `Ref` morre no `;`). O outro `cache` (builtin privado) já separava o `borrow`.
- `Type error` literal: NÃO é bug em `iterator_operations.rs`/`set_prototype.rs`. O bun mede `new Set(1)` e
  `new Map(1)` = "Type error" (o `throwTypeError(globalObject, scope)` sem mensagem de IteratorOperations.cpp),
  e "Iterator result interface is not an object." COM ponto final. A raiz era `slow_path_spread`
  (`handlers_iterator.rs`): o C++ chama `iteratorProtocolFunction` (JS) que lança "Spread syntax requires ...iterable
  not be null or undefined" / "...iterable[Symbol.iterator] to be a function"; o porte usava `for_each_in_iterable`
  direto. Agora valida antes com as duas mensagens (bun: `[...1]`, `f(...1)`, `Math.max(...{})` confirmam).
- `host_function_support.rs:309`: não é o ponto, é o `panic!` do `PutError::Unported(what)` (linha 312 hoje),
  com `what` = "índice acima de MAX_ARRAY_INDEX (nome de propriedade por string)"; origem do `Unported` não achada
  por grep (a string não existe mais no código atual, o golden é de build anterior). Reconferir no próximo run.
- `internal_function.rs:294` (`get_function_realm`, `debug_assert!(object.is_callable())`): chamadores
  `slow_path_create_this` (`constructor` = callee, deveria ser chamável), `object_constructor.rs:533`,
  `function_constructor.rs:244`, `js_global_object_inlines.rs:49`, `get_derived_structure_in_realm`. Não isolado;
  suspeita: `is_callable_type` não cobre `JSBoundFunction`/`JSRemoteFunction`/classe como `CellEntry` próprio. Medir
  com `Reflect.construct(f, [], boundFn)` no próximo run.

## Terceira passada (sem cargo, por conferir no build)

- `get_function_realm`: o predicado NÃO era o problema para bound/remote/proxy. `JSBoundFunction` e
  `JSRemoteFunction` são `CellEntry::Function` (`JSFunctionType`), `ProxyObject` entra por `m_isCallable`
  (`is_callable_cell`), e as constantes de `InternalFunction` usam `InternalFunctionType`. A cadeia
  bound -> target, remote -> target, proxy -> target (revogado: TypeError) já estava no laço; o
  `debug_assert!(object.is_callable())` só validava a primeira volta e derrubava o processo se algum
  chamador passasse um valor não chamável (ex.: callee que o registro não expõe como função, valor vazio).
  Agora não há assert: valor que não é objeto, ou objeto sem realm na `Structure`, vira
  `Thrown::type_error` em vez de pânico. Chamador exato do pânico não isolado sem rodar; se o TypeError
  novo aparecer num golden, a mensagem diz qual dos dois casos é.
- `PutError::Unported` em `host_function_support.rs`: é só o `panic!` do tradutor. A string do golden
  ("put de length com valor não primitivo ou string (ToNumber chama valueOf)") não existe mais no código,
  o golden `/tmp/now_object_edge_bun_golden.txt` é de build anterior (`Object.defineProperty([],'length',
  {value:'3'})` e `{valueOf(){return 4}}`); reconferir no próximo run, não portado de novo.
- Construtores de `Unported` hoje (grep): inalcançáveis na prática por `JSObject::from_value` já aceitar
  `JSFunction` (`array_prototype.rs:150`, `proxy_object.rs:1423/1443`); raros: TypedArray na cadeia do
  `ordinarySet` (1404), `js_scope.rs` (with sem objeto), WebAssembly (`js_web_assembly.rs`), `Atomics.wait`
  sem prazo, índice acima de MAX_ARRAY_INDEX em `setFromArrayLike`, `collection_support.rs:92`,
  (achado class_edge, 2026-10-08, sem cargo) As 24 divergências "The value of the superclass's prototype
  property is not an object or null." têm a mensagem e o appender corretos (`invalid_prototype_source_appender`,
  `create_invalid_prototype_error`, `emit_direct_set_prototype_of` com o divot da classe, tudo idêntico ao
  C++). O que faltava era o sufixo do site em erro lançado por função nativa (`veio` sem `(evaluating ...)`
  nem a frase do appender, igual a `Object.keys(null)` no golden de error_message): `append_native_call_site`
  só age se `Vm::native_call_site` estiver posto, o que `handle_host_call` (`llint/dispatch.rs:668`) passou a
  fazer às 18:21, dois minutos antes do golden de 18:23 ser gerado. Provável que já esteja corrigido; reconferir
  esse golden após o próximo build antes de mexer em código.
  `js_property_name_enumerator.rs:261`. Nenhum aparece nos goldens `/tmp/now_*_bun_golden.txt` além do de length.
# Auditoria de erros do zjsc (template edge e destructuring contra o bun)

Sem correção aplicada nesta passada (sem cargo, tempo curto). Achados por raiz, para a próxima sessão.

## Template edge (47 de 671)

### Raiz A: `String.raw` devolve "" em toda chamada (38 casos)
Tudo o que passa por `String.raw` (tagged ou chamada direta, `.call`, `.apply`, `Reflect.apply`) volta `""`:
inclusive `String.raw({raw:['a','b']}, 1)`. Os casos de `Symbol()` que deviam lançar TypeError também voltam `""`,
então a função sai antes de converter qualquer segmento: ou `literal_count == 0`, ou um `JSValue::empty()` é
devolvido sem exceção pendente (o `globalThis.R = <empty>` aparece como `""`).
Código: `src/runtime/string_constructor_natives.rs`, `string_raw` (linhas 84 a 154), que parece fiel ao
`stringRaw` do C++. Já descartados por leitura: `to_length_checked`, `concat`, `code_units`, `to_wtf_string_value`,
`arguments_span`, o `length` virtual de `JSArray` (`js_object.rs:946`). Hipótese principal restante: a leitura de
`length` por `ObjectRef::get` (`host_function_support.rs`, `get_property_slot` do `Handle`) devolvendo `undefined`
para o array literal, ou `for_property_lookup`/`to_object` devolvendo `None` sem exceção. Próximo passo:
instrumentar `string_raw` (eprintln de `length_value` e de cada retorno antecipado) com um teste mínimo.
O caso do Proxy (`esperado "axbraw"`, veio `"raw"`) e o `new String.raw\`a\`` (mensagem com `""` no lugar de `"a"`)
são o mesmo sintoma. O caso `crlf` vs `lf` também: é `'' === '\n'`, não um bug do Lexer.

### Raiz B: `RefCell already borrowed` em `function_executable.rs:589`
`\`${function f(){...}}${class A {}}\``: `to_string_slow` (`cache` faz `this.borrow_mut()`) roda com um
empréstimo ainda vivo em quem chamou (provável: `Function.prototype.toString`/`to_string` chamado de dentro de um
`borrow()` do executable). Descartado: os `borrow()` dentro de `to_string_slow` e do `to_string` (temporários já soltos
antes de `cache`). Achar o chamador que segura o `Ref`.

### Raiz C: `\`${this}\`` num `(0, eval)` global: "TypeError: No default value"
O `ToString(globalThis)` do template falha no `ToPrimitive` do objeto global (`toString` do protótipo não é achado
ou o global proxy não resolve o método). Verificar `String(globalThis)` e `\`${globalThis}\`` fora do eval; é o
caminho `op_to_string`/`to_primitive` com `JSGlobalProxy`, não o template.

### Raiz D: `\`${import('x')}\`` em eval: pânico "LinkTimeConstant ImportModule não inicializada"
`js_global_object.rs:535`. É host (carregamento de módulo), fora do escopo de template.

## Destructuring (5 de 1350)

1. `var [a] = {[Symbol.iterator](){return {}}}` e `next: 1`: a mensagem do bun leva o sufixo `(near '...[a]...')`;
   o porte emite sem o trecho. É o `handleTypeError`/`ExpressionInfo` do `next` não callable no
   `ArrayPatternNode::bindValue` (`src/bytecompiler/nodes_codegen_cpp6.rs`, linha 582): a chamada de `next` precisa
   levar a divot da expressão do padrão.
2. `var [a] = Object.create([1,2])` devolve undefined (esperado 1): iterador de array sobre objeto cujo protótipo
   é um array; o `ArrayIterator.next` não vê o elemento indexado via cadeia de protótipos (provável fast path que
   exige `JSArray` próprio). Checar `get_property_slot_by_index` e o iterador de array.
3. `[o.x] = mk([1])` com setter que lança: o `ret` (iterator close) não é chamado. O bun fecha o iterador quando o
   alvo da atribuição lança (try/finally implícito em `ArrayPatternNode::bindValue` para destructuring assignment
   com alvo que não é identificador). Verificar o `iteratorClose` no caminho de exceção da atribuição em
   `nodes_codegen_cpp6.rs`. Dois dos cinco casos (com e sem `retThrow`).

## Segunda passada (só leitura, sem cargo, sem correção aplicada)

### `String.raw`: leitura completa, nenhuma causa visível
Conferidos contra o upstream e sem divergência: `string_raw` (`string_constructor_natives.rs:84`), `arguments_span`
(`call_frame.rs:408`, `Vec` sem `this`, índice `index + 1` correto), `to_length` (`js_value_conversions.rs:432`),
`ObjectRef::get`/`for_property_lookup` (`host_function_support.rs`), `JSObject::get_property_slot` e
`get_property_slot_by_index` (`js_object.rs:1076`, `1144`; `JSArray::STRUCTURE_FLAGS` liga
`OVERRIDES_GET_OWN_PROPERTY_SLOT`, então o `length` virtual de array é achado também pela cadeia),
`JSArray::get_own_property_slot` (`js_array.rs:740`). Existe teste que cobre `String.raw` com arrays e objetos
(`tests/regexp_string_methods.rs:201`); o golden `template_edge_bun_golden` é o que falha. Diferença a investigar: o
caminho de chamada do golden é tagged template / `.call` / `Reflect.apply` (o objeto template é um array congelado com
`raw` congelado), e `String.raw` registrada com `put_direct_native_function_without_transition` (comprimento 1). A
hipótese de `literal_count == 0` por `length` indefinido não se sustenta pelo código; falta confirmar com
`eprintln!` em cada `return` de `string_raw` e um teste mínimo rodando `String.raw({raw:['a','b']},1)` pelo mesmo
`evaluate_script_sequence_result` do golden (e não por `is_true`), para ver se a diferença está na leitura de `R`
(`globalThis.R` guardando um `JSString` de `concat` que o `to_wtf_string` do harness lê vazio).

### `var [a] = Object.create([1,2])`
O `next` do iterador de array é o builtin `arrayIteratorNextHelper` (`ArrayIteratorPrototype.js`): lê `array.length`
(`@toLength(array.length)`) e `array[index]`. Em objeto cujo protótipo é um array, `array.length` vai por
`get_property_slot` (cadeia) e `array[0]` por `get_by_val` -> `get_by_index` -> `get_property_slot_by_index`
(`slow_paths_object.rs:627`), ambos corretos na leitura. Suspeita restante: o `get_by_val` do LLInt/fast path de
`arrayIteratorNextHelper` ou o intrínseco do iterador (`isArrayIterator`/campos internos) tratando `array` como se
exigisse `JSArray` próprio, ou `Symbol.iterator` herdado resolvendo para `Array.prototype.values` com `this` do objeto
derivado e `emit_iterator_open` com fast path `@arrayIteratorProtocolIsFast`. Verificar nesse fast path.

### `[o.x] = mk([1])` com setter que lança
`ArrayPatternNode::bind_value` (`nodes_codegen_cpp6.rs:583`) já espelha o upstream: `bind_value_can_throw`
(`nodes_codegen_cpp7.rs:515`) devolve `true` para alvo que não é `ResolveNode`, então o ramo
`emit_try_with_finally_that_does_not_shadow_exception(emit_bind_value, emit_iterator_close)` é usado, com `done`
carregado `false` antes e `true` antes de cada `emit_iterator_next`. Logo a falta do `return` não está no
`bind_value`; olhar `emit_try_with_finally_that_does_not_shadow_exception` (o handler precisa executar o
`emit_iterator_close` e relançar) e se `done` ainda vale `true` no handler (o `emit_iterator_next` deve devolvê-lo a
`false` depois do `next` bem-sucedido; se não o faz, o `jump_if_true(done)` do close pula o `return`).

## Terceira passada (golden now3, 40 de 1732; sem cargo, correções aplicadas sem compilar)

### Corrigido
1. Spread (`[...x]`, `f(...x)`, `Math.max(...x)`) com iterador genérico: o `slow_path_spread` do C++ chama o
   `iteratorProtocolFunction` (IteratorHelpers.js), um laço JS que não passa por `iteratorNext`. Por isso `next`
   inexistente diz `undefined is not a function` (e `{next:1}` diz `1 is not a function`), e o resultado que não é
   objeto diz `Iterator result interface is not an object` SEM ponto final (o `@throwTypeError` do builtin).
   Antes o Rust usava `for_each_in_iterable` (`iteratorNext` com `Type error` e com ponto). Agora
   `handlers_iterator.rs::spread` mantém `for_each_in_iterable` só nos modos rápidos (`get_iteration_mode` diferente
   de `Generic`) e faz o laço do builtin no genérico. 12 casos.
2. `var [a] = ...` e `for (x of ...)` com `next` não chamável: o `op_iterator_next` genérico é um `op_call` no
   C++, então o erro leva `(near '...[a]...')`. `iterator_next` passa agora `f.error_site()` ao
   `create_not_a_function_error` (antes `None`). 4 casos (conferir o texto do trecho no próximo run).
3. `class A extends Math.max.bind(){}`: `function_construct_data` (`call_data.rs`) não tinha o ramo de
   `JSBoundFunction` do `getConstructDataInline`: função vinculada só constrói se `canConstruct()` (alvo ao fim da
   cadeia constrói). `Math.max` não constrói, então o `op_is_constructor` agora dá falso e a mensagem sai
   `The superclass is not a constructor.`. Vale também para `instanceof`/`new` com bound function de nativa.

### Não corrigido, com a causa provável
- `Cannot redefine existing private field (evaluating 'super(o)')` e os dois `near '...(function () { })...'` /
  `near '...obalThis.R = T(()=>{class A{static x=A.y...'`: o erro de dentro do inicializador de campo usa o
  texto-fonte de outro trecho no bun (o frame do inicializador sintético é pulado ou tem fonte própria). O Rust usa
  o divot do próprio campo (`#y`, `this.y.z`), igual ao `DefineFieldNode::emitBytecode` do upstream, então a
  diferença está em qual `CodeBlock`/`bytecode_index` o `ErrorInstance` escolhe como frame do topo: para
  inicializador de campo (`SourceParseMode::ClassFieldInitializerMode`) o bun usa o chamador (`super(o)`) ou a fonte
  sintética do executável. Investigar `append_source_to_error_message` e o stack walk de `ErrorInstance`.
- `Array.apply(null,{length:-1})` devia dar `ok` e dá `Cannot get function realm from a non-object`: o `Array`
  chamado com `newTarget` indefinido (via `apply`) está indo ao `getFunctionRealm(newTarget)` em vez de usar o realm
  da própria função; olhar o `constructor` de Array (`array_constructor.rs`) quando `newTarget` é `undefined`.
- WebAssembly: `new WebAssembly.Module(u8)` e `new WebAssembly.Instance(1)` sem o sufixo `(evaluating '...')`
  (a nativa deve lançar via o appender de erro de chamada nativa, como as demais); `new WebAssembly.Memory({})`
  devia lançar `Expect an integer argument in the range: [0, 2^32 - 1]` (falta a validação de `initial`
  ausente); `new WebAssembly.Module()` e `WebAssembly.validate(1)` devem dizer `first argument must be an
  ArrayBufferView or an ArrayBuffer (evaluating '...')`, não `Argument 0 must be a buffer source`.
- Host do bun, ignorar: `Error.captureStackTrace(1)` (`invalid_argument` é do Bun, não do JSC), `URLSearchParams`,
  `DOMException`, `AbortSignal`, `Event`, `EventTarget`, `Blob`, `Headers`, `Request`, `Response`.

## Terceira passada (goldens now3: regexp, class, template, destructuring)

Corrigido (sem cargo, por conferir no build):
- `regexp_edge` (4 de 5): `RegExp::match_ovector` não portava `matchInlineAtCodePointBoundaries`
  (`RegExpInlines.h:115`). Padrão `u`/`v` sobre entrada de 16 bits agora recua um quando o início cai no meio de um
  par substituto (`splitsSurrogatePair`) e descarta casamento que começa no meio de um par, recomeçando em `start+1`.
  Cobre `/\u{1F600}/uy` com `lastIndex=1`, `/./gu` e `/./gv` com `lastIndex=1`, `/(?:)/gu` (esperado índice 0).
  O miolo anterior virou `match_ovector_once` (o `matchInlineOnce`).
- `template_edge` (`${import('x')}`): o upstream sempre cria a `LinkTimeConstant::importModule` (`initLater`,
  JSGlobalObject.cpp:2007), com ou sem host de módulos; aqui só `install_module_loader` a criava, e o compilador
  entrava em pânico num global sem carregador. Extraído `install_import_module_constant` (`js_module_loader.rs`),
  chamado por `init_link_time_constants` e por `install_module_loader`. Sem host, `import()` devolve promessa
  rejeitada com o `ResolveMessage` do bun (ver `wip-notes/module-loader.md`, fatia 3), como o resultado esperado `ok` exige.
- `destructuring` `(near '...[a]...')` (2 casos): `iterator_next` genérico já passa o `error_site()` ao
  `create_not_a_function_error` (alteração concorrente em `handlers_iterator.rs:329`); falta o mesmo em
  `open_iterator_call` (linha 120, `Symbol.iterator` não chamável) e em `handlers_iterator.rs:381`/`handlers_async.rs:125`.

Sem correção:
- `regexp_edge` `RegExp.$1+RegExp.lastMatch+RegExp.input`: artefato do gerador do golden. O esperado
  `T(T(()=>RegExp.$1+...` é o `RegExp.input` deixado por uma regex do próprio harness do bun. Não é bug do motor; o caso
  deve ser reescrito para executar uma regex antes (`/x/.exec('abc')`) ou removido do `regexp_edge_bun.tsv`.
- `destructuring` `[o.x] = mk([1])` com setter que lança, `ret` ausente (2 casos): `ArrayPatternNode::bind_value`,
  `bind_value_can_throw`, `emit_try_with_finally_that_does_not_shadow_exception` e `emit_iterator_generic_close`
  conferidos linha a linha contra o upstream, idênticos. Resta o runtime: `slow_path_iterator_next_get_done` grava
  `done` a partir de `iteratorReturn.done`; conferir no build, com um dump do bytecode do caso, se o handler
  `SynthesizedFinally` cobre o `put_by_id` do setter (faixa `try_start..try_end`) e se `done` vale `false` ali.
- `class_edge` (6): sem tempo para fechar.
  - Dois casos de `Cannot redefine existing private field`: o esperado é `(evaluating 'super(...args)')` (o
    construtor derivado implícito), o atual é `(evaluating '#x')`. O upstream não emite `emitExpressionInfo` no
    `op_define_private_field` do inicializador; o texto vem da expressão mais próxima antes dele no construtor
    (o `super(...args)`). Procurar no emissor de campos de instância (`emit_instance_field_initialization_if_needed`
    e `ClassFieldNode`/`ClassExprNode`) um `emit_expression_info` a mais com o divot de `#x`.
  - Quatro casos de `Cannot access invalid private field` em inicializador que lê `#p` declarado depois: o esperado é
    `(near '...obalThis.R = T(()=>{class A { static x =...')` ou `(near '...(function () { })...')`, o atual
    `(evaluating 'this.#p')`. Mesma família: a mensagem do upstream usa `defaultSourceAppender` com o divot do
    bytecode anterior à leitura (sem expression info próprio do `get_private_name`), então o texto-fonte vira
    `near`. Conferir se o emissor de `DotAccessorNode` com nome privado emite `emit_expression_info` antes do
    `get_private_name`, como o upstream (`NodesCodegen.cpp`, `DotAccessorNode::emitBytecode`, ramo `isPrivateField`).

## native_call_site em tail call para nativa (2026-10-08, sem cargo, não compilado)

Raiz das cerca de 32 divergências `(evaluating 'f()')` do golden object_edge: em `"use strict"`, `f = () => Object.keys(null)`
faz `op_tail_call` para a nativa, o JSC reaproveita o frame do arrow e o `topCallFrame` do nativo passa a ser o
chamador do arrow (`f()` dentro de `T`). Correção em `src/llint/dispatch.rs`: `handle_host_call` ganhou o parâmetro
`tail`; com `tail == true`, `tail_caller_site` sobe dois frames a partir de `callee_frame` (`callee_frame.caller` é o
frame que fez a tail call, o `caller` dele é o frame de baixo) e devolve o `CodeBlock` (`Interpreter::code_block(id)`) e o
`bytecode_index` (high word de `argumentCountIncludingThis`, posto por `set_current_vpc` na chamada de `T`) para
`VM::native_call_site`. Frame de baixo nativo, sem `CodeBlock` ou sem chamador (entrada) cai no site do próprio frame.
Chamadas não-tail seguem com o site do frame atual. O `ErrorSite` dos erros não-nativos (not a function) ficou como
está. Pendente: compilar e rodar `object_edge` para confirmar.

### Inicializador de campo de classe: frame privado é pulado (sem cargo, por conferir no build)
Causa: `ErrorInstance::finishCreation` chama `getBytecodeIndex(vm, topCallFrame)` (`Error.cpp`), que percorre a pilha
com `FindFirstCallerFrameWithCodeblockFunctor` e PULA frame nativo, sem `CodeBlock` e com
`isImplementationVisibilityPrivate()`. A função sintetizada de campos (`emitNewClassFieldInitializerFunction`) nasce
com `ImplementationVisibility::Private`, então o erro lançado dentro dela cita o frame que a chamou: o construtor
(`super(...args)` do construtor derivado padrão, `super(o)` do explícito; o texto aproximado `(function () { })` do
construtor base padrão, cujo fonte é sintético) ou, nos campos estáticos, o frame da classe com o divot do `class`
(`near '...obalThis.R = T(()=>{class A { static x =...'`). O Rust usava sempre o `CodeBlock` do frame em execução.
Correção: `BlockErrorSite` ganhou `visible_caller` (exception_helpers.rs) e `SlowPathFrame::error_site`
(slow_paths_arith.rs) o preenche com `visible_caller_site`, um `StackVisitor::visit` que devolve o primeiro frame
visível acima do corrente (`None` se o corrente já é visível). O divot da chamada do inicializador já era o do
upstream (`emitInstanceFieldInitializationIfNeeded(..., m_scopeNode->position() x3)` e `node.position()` x3 no
estático), nada a mudar ali.
Fechado (sem cargo, por conferir no build): `visible_caller_site` virou função livre em `exception_helpers.rs`
(o método de `SlowPathFrame` saiu; `error_site` a chama direto). `ErrorSite` ganhou `vm` e `call_frame` e o método
`visible_location()`, calculado só no caminho de erro (a chamada comum não paga a caminhada). Ele serve a
`append_source` ("not a function"/"not a constructor", `dispatch.rs`), ao erro de `Function.prototype.apply`
(`varargs.rs`, que agora grava o `currentVPC` antes do `size_frame`, pois o frame privado o lê) e ao fallback do
`native_call_site` em `handle_host_call`. No C++ todos esses erros nascem em `ErrorInstance::finishCreation`, que
sempre consulta `getBytecodeIndex(vm, topCallFrame)`, e o `topCallFrame` de uma chamada é o frame que executa a
instrução; logo o frame visível vale em todos eles, e o `tail_caller_site` (frame abaixo da tail call) continua
como estava. Falta conferir os 6 casos de `class_edge_bun_golden` no build, em especial se o construtor padrão
base/derivado tem `ExpressionInfo` com o divot esperado.

## Auditoria de TDZ em campo de classe e fechamento de iterador em destructuring (2026-10-08, só leitura)

Comparado linha a linha com o upstream (`BytecodeGenerator.cpp`, `NodesCodegen.cpp`, `CommonSlowPaths.cpp`):

- Idênticos ao C++, sem divergência: `emit_tdz_check_variable`, `emit_tdz_check`, `needs_tdz_check`,
  `emit_tdz_check_if_necessary`, `emit_expression_info`, `ResolveNode::emit_bytecode`,
  `TypeOfResolveNode::emit_bytecode`, `ClassExprNode::emit_bytecode` (campos estáticos e de instância, ordem do
  `put_to_scope` do nome da classe antes do inicializador estático), `ArrayPatternNode::bind_value`,
  `AssignmentElementNode::bind_value_can_throw`, `emit_try_with_finally_that_does_not_shadow_exception`,
  `emit_iterator_open`, `emit_iterator_next`, `emit_iterator_generic_close`, `CodeBlock::expression_info_for_bytecode_index`
  e `slow_path_check_tdz` (`create_tdz_error_from_source_range`).
- Divergência provada e corrigida: `ResolveNode::emit_bytecode`, ramo de variável não local com TDZ, chamava
  `emit_tdz_check(&unchecked_result)` (operando `undefined`); o C++ chama `emitTDZCheck(uncheckedResult, m_ident)`,
  que grava o nome como constante. Agora usa `emit_tdz_check_variable` com `Variable::from_ident`. Efeito
  direto só no operando (a mensagem vem do trecho do fonte), mas o bytecode dump e qualquer leitor do operando
  passam a ver o nome.

Não reproduzido por leitura (nenhuma divergência de gerador achada, causa provável fora do gerador):

1. TDZ em campo de classe (`class_edge_bun.tsv` linhas 203 e 231, nomes `B` e `A`): o gerador emite o mesmo
   bytecode do C++. Suspeitos restantes, a conferir no build: o `source()` do `CodeBlock` do inicializador de
   campo (a conta `divot + source_offset` usa `owner_executable.source().start_offset()`, que precisa coincidir com
   o `m_scopeNode->source().startOffset()` do gerador do inicializador sintético), e o `cachedParentTDZ` herdado
   pelo inicializador de campo (decide se o `check_tdz` é emitido).
2. Fechamento de iterador com setter que lança em `[a.b] = iter`: o gerador tem o `try/finally` sintetizado e o
   `emit_iterator_generic_close` iguais ao C++. Suspeitos restantes: o tratamento da exceção pelo handler
   `SynthesizedFinally` no interpretador (`emit_out_of_line_finally_handler`, `emit_finally_completion`) e o
   `ErrorSite` de `op_iterator_next`/`op_iterator_open` em `llint/handlers_iterator.rs`, de onde sai o trecho
   `(near '...[a]...')`; o divot usado é o do `ArrayPatternNode`, igual ao C++.

Próximo passo (exige build, fora desta passada): rodar `destructuring_bun` e `class_edge_bun` e imprimir o
`ExpressionInfo` do bytecode index que lança.

## Sufixo dos 6 casos de campo privado em class_edge (leitura, sem cargo)

Causa provada pela leitura do upstream, não é divot errado: o codegen do Rust já é idêntico ao C++.
- `DotAccessorNode::emitBytecode` emite `emitExpressionInfo(divot, divotStart, divotEnd)` antes de
  `emitGetPropertyValue`; o ramo privado (`emitGetPrivateName`/`emitPrivateFieldPut`) não emite nada próprio, e
  `emit_get_private_name` (`bytecode_generator_cpp4.rs`) e `nodes_codegen_cpp1b.rs:961` conferem com o C++.
- O que muda o sufixo é o frame escolhido, não a instrução. `ErrorInstance::finishCreation` chama
  `getBytecodeIndex(vm, topCallFrame)` (`Error.cpp`), cujo `FindFirstCallerFrameWithCodeblockFunctor` pula frames
  nativos e `isImplementationVisibilityPrivate()`. O inicializador de campos de classe é criado com
  `ImplementationVisibility::Private` (`emitNewClassFieldInitializerFunction`, BytecodeGenerator.cpp:3597;
  Rust em `bytecode_generator_cpp4.rs:1157`), então o erro lançado dentro dele cita o chamador:
  (a) construtor derivado padrão: `super(...args)` (os dois `Cannot redefine existing private field`);
  (b) construtor base padrão, de fonte sintético `(function () { })` (`BuiltinExecutables.cpp:56`), para campo de
  instância (`near '...(function () { })...'`); e para campo estático o código do corpo que chama o
  inicializador, onde o divot vazio mais recente é o início da classe (`near '...obalThis.R = T(()=>{class A { static x =...'`).
- O mecanismo já existe no Rust: `SlowPathFrame::visible_caller_site` (`llint/slow_paths_arith.rs:93`, com
  `BlockErrorSite.visible_caller`), usado por `throw_invalid_private_name` e `throw_default_appended_type_error`
  (`slow_paths_object.rs`). Ele foi escrito às 18:47 de 2026-10-08; o golden com as 6 divergências
  (`/tmp/now3_class_edge_bun_golden.txt`) é das 18:35, portanto anterior. Nenhuma edição de código foi
  necessária; reconferir `class_edge_bun` no próximo build.
- Lacuna restante, não afeta estes casos: o functor do C++ deixa `m_bytecodeIndex = 0` quando o `CodeBlock`
  achado é de função builtin; `visible_caller_site` não faz esse zero. Também `ErrorSite` (`dispatch.rs:489`,
  `varargs.rs:281`) não aplica a regra de visibilidade privada. Se `class_edge` ainda divergir, olhar estes dois
  pontos e se `frame.bytecode_index()` do chamador é o do `op_call` do inicializador.

## stack_format_bun_golden: 381 divergências agrupadas (2026-10-08, sem cargo, saída de /tmp/now3_stack_format_bun_golden.txt)

Das 381, 233 diferem só na coluna; o resto (147) é real. Causas por volume:

1. **Coluna (233 casos): dado de ambiente do bun, não mexer.** O golden roda `bun arquivo.js`, que transpila e
   remapeia a posição pelo source map: a coluna cai no início do último token antes do divot (`Error` em
   `new Error(`, `m` em `.m()`), daí os deltas 5, 1, 7, 4 (comprimento do identificador). Medido: o mesmo código via
   `vm.runInThisContext` no bun dá `1:33` e `1:74`, idêntico ao nosso (divot no `(`, `NewExprNode`/`CallNode`).
   Conferir coluna exige gerar o golden com `vm.runInThisContext(src, {filename})` em vez de arquivo (mudança no
   `scripts/gen-stack-format-golden.js`, ainda não feita: regenerar o TSV inteiro).
2. **Programa que lança sem gravar `R` (24 casos): o teste punia.** Esperado `<undefined>` (o bun morre, `R` fica
   sem valor) e o runner devolvia "lançou exceção". `tests/stack_format_bun_golden.rs` agora aceita esse erro
   quando o esperado é `<undefined>`.
3. **`getFunctionName`/`getMethodName` (24 casos, "null" vs ""): corrigido.** Medido no bun: função anônima,
   programa e `eval` devolvem `""`, nunca `null`. `function_name_value` em `call_site_prototype.rs`.
4. **Frame `[Symbol.replace]` extra entre o callback e `replace` (~9): corrigido.** O JSC faz o atalho
   `isSymbolReplaceFastAndNonObservable` em `stringProtoFuncReplace`; sem ele o nativo `[Symbol.replace]` aparecia.
   `call_builtin_reg_exp_replace` (reg_exp_prototype_natives.rs) roda o corpo sem frame quando o método é o nativo
   original, chamado por `call_symbol_method` (replace e replaceAll).

Pendentes, não tocados: frames `async` ausentes (~17: `at async a`, `at async <anonymous>`), `new Function` mostra
`at anonymous` sem `(file:///error_stack_case.js:3:17)`, `new B` de construtor derivado implícito sem
`(unknown:1:28)`, método computado (`['co'+'mp']`) e `[Symbol.toPrimitive]` sem nome (`<anonymous>` em vez do nome
inferido), `stackTraceLimit` com `rec(4).then` (`nostack` vs 2).
### Oráculo de coluna regerado (2026-10-08) e frames `async`

- `scripts/gen-stack-format-golden.js` agora mede com `vm.runInThisContext(src, {filename: 'error_stack_case.js'})`
  num bun filho novo por programa (mesmo filename do teste). Os frames do runner (`runInThisContext`, `zz_runner.js`)
  saem do texto; cada programa roda duas vezes (0 e 3 funções de embrulho) e é descartado se o resultado muda, o que
  pega truncamento por `stackTraceLimit`. TSV regerado: 511 mantidos, 26 descartados (quase todos `prepareStackTrace`
  com `CallSite` e contagens com limite alto, contaminados pelos frames do runner); piso de 400 do teste mantido.
  Conferido por amostra: `class K { m() { return new Error('x').stack } }` dá `2:33` e `2:74` (divot no `(`), igual ao porte.
- **Efeito colateral a decidir:** em `vm` o frame de código global sai como `at error_stack_case.js:2:74` (sem nome),
  não `at <anonymous> (error_stack_case.js:2:74)` como no modo arquivo (`error_stack_bun.tsv`, onde `<anonymous>` é o
  embrulho do módulo CJS do bun). O porte imprime a forma com `<anonymous>`, então ~245 linhas do novo TSV divergem só
  por isso. O `error_stack_bun.tsv` e este golden ensinam formas opostas para o mesmo frame global; o JSC puro é a forma
  sem nome (`StackFrame::toString`). Não mexi em `display_name` para não quebrar o outro golden.
- **Frames `async` (porte de `Interpreter::getAsyncStackTrace`)**, medido no bun em 10 casos (`await` encadeado de 2 e 3
  níveis, IIFE async `at async <anonymous>`, `try/catch`, método de classe, `Promise.all`, `await 1` antes, e os dois
  negativos: `return a()` sem `await` e função síncrona no meio não geram frame). Em `src/interpreter/unwind.rs`:
  `get_async_stack_trace`/`parent_generator` (await simples, combinadores, race), índice de bytecode pela última tabela de
  salto (`state`), nome `async nome` (`async <anonymous>` no `display_name` de `stack_frame.rs`). O `VMEntryRecord::m_context`
  do C++ não existe: `enter_async_origin` (thread_local em `unwind.rs`) é armado por `async_function_resume` e
  `async_generator_driver_resume` em `js_microtask.rs` pela duração do corpo; os frames entram depois dos síncronos
  (insert pos = fim, sem o caso de várias entradas aninhadas). Refatorado `line_and_column_for` (stack_visitor.rs).
  Lacuna: `InternalFieldTuple` de contexto ALS do Bun não é desembrulhado. **Não compilado nem rodado (sem cargo)**;
  usei script Python (troca de bloco) em unwind.rs, stack_visitor.rs, stack_frame.rs e js_microtask.rs em vez de Edit.

Nada disso foi compilado nem rodado (sem cargo). Edição de `reg_exp_prototype_natives.rs` e
`string_prototype_natives_part2.rs` foi feita com script Python (troca de bloco exata), não com Edit.

## Passagem now4 (14 de 1732 divergiam; build recente)

Todas as 14 divergências eram do Bun, não do JavaScriptCore, e saíram do golden (agora 1718 programas):

- 13 casos de globais do WebCore/Bun (`URLSearchParams`, `DOMException`, `AbortSignal`, `Event`, `EventTarget`, `Blob`,
  `Headers`, `Request`, `Response`): não existem no JSC puro, `ReferenceError` é o comportamento correto do porte.
- 2 casos de `Error.captureStackTrace(1)` e `Error.captureStackTrace()`: o bun lança `TypeError: invalid_argument`
  (implementação própria do Bun), o JSC em `upstream/JavaScriptCore/runtime/ErrorConstructor.cpp:117` lança
  `captureStackTrace expects the first argument to be an object`, que é o que o porte já faz. Medido no bun 1.4.2.
- Correção: removidos de `scripts/gen-error-message-golden.js` (com comentário do porquê) e do
  `tests/golden/error_message_bun.tsv` (linhas apagadas por `grep -v`, só 14, conferido por contagem 1732 para 1718;
  usei Bash por ser remoção de linhas inteiras muito longas). Nenhum código do porte mudou. Não rodei cargo.

## Passagem scope golden (6 divergências restantes de 1919; sem cargo)

- **TDZ com nome vazio** (`class C { static s = D; }` e `class K { a = b; }`, esperado `Cannot access ''`):
  `slow_path_check_tdz` (CommonSlowPaths.cpp:312) calcula o nome com `getBytecodeIndex(vm, callFrame)`, o primeiro
  frame VISÍVEL (o inicializador de campo sintético é `Private`), tira a expression info do `CodeBlock` desse frame
  (o chamador) e lê o trecho do `provider` do `codeBlock` corrente. O porte usava o `bytecode_index` do próprio
  frame privado. `create_tdz_error_from_source_range` agora recebe o `BlockErrorSite` (`f.error_site()`, que já traz
  `visible_caller`) e reproduz as duas fontes. Texto vazio vem de a expression info do chamador (construtor padrão
  sintético, divot zerado) não cobrir nada no fonte. Não verificado em execução.
- **`[super.x]() {}` em eval, sem sufixo `(evaluating ...)`**: o erro sai por `JSValue::get_prototype`/`to_object`,
  que criam o erro sem `site` e consultam `vm.native_call_site()`, ainda apontando para a chamada nativa do `eval`
  (`(0, eval)(...)`). No C++ o `topCallFrame` já é o frame do código do eval. `llint_execute` (dispatch.rs) agora zera
  `native_call_site` durante o laço do código JS que um nativo inicia e restaura na saída (vale também para callbacks
  de `map` etc.). Efeito colateral esperado: erros sem `site` dentro de callbacks viram mensagem pura em vez de citar a
  chamada do nativo; o ideal é cada handler passar o `site` (lacuna).
- **`({ __proto__: 1, __proto__ })` em eval** (esperado `TypeError ... (evaluating '})')`, porte devolve `ok`):
  NÃO resolvido por falta de tempo. Pista: o texto `'}'` sugere um `emit_throw`/checagem de objeto no fim do literal
  em `PropertyListNode::emitBytecode`; investigar o ramo `__proto__` com valor primitivo no bun.
- Edições feitas com script Python (troca de bloco) em `exception_helpers.rs` e `handlers_misc.rs`; as demais com Edit.

- **Exceção de setter como última instrução do `try` não era capturada** (`try { o.x = 1 } catch(e){}` / `finally`
  com `put_by_id`/`put_by_val` no fim do corpo; com uma instrução depois o `finally` rodava): causa achada por leitura
  em `GetterSetter::call_setter` (`runtime/js_getter_setter.rs`), que mapeava `Err(Thrown)` para `Ok(true)` (o `callSetter`
  do C++ faz isso porque o chamador faz `RETURN_IF_EXCEPTION`). O `slow_path_put_by_id`/`put_by_val` do porte devolviam
  `Ok`, `Step::Next` não confere o `VM`, e a exceção pendente só era vista numa instrução posterior, cujo `currentVPC`
  já estava fora da faixa do `try`. O gerador de bytecode e a busca de handler (`unwind`) estão corretos. Correção:
  `call_setter` devolve `Err(PutError::Pending)` quando o setter lança. Não verificado em execução (sem cargo).
- **PANIC em `[o.x, o.x] = it`** (setter que lança): provavelmente o mesmo defeito (exceção pendente ignorada, a
  próxima chamada, `next()` do iterador ou `IteratorClose`, roda com exceção pendente e dispara asserção). Não
  reproduzido por falta de execução; se persistir após a correção, rodar com backtrace.
