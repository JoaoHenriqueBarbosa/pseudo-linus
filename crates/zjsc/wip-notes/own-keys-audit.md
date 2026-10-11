# Auditoria de chaves próprias (Reflect.ownKeys) contra o bun 1.4.2

## O que mudou na medição

- `scripts/gen-own-keys-golden.js` agora mede 175 caminhos (eram 105): além dos que já existiam, os 12 TypedArray
  e seus protótipos, `Function.prototype`, `Intl` e cada `Intl.X` (construtor e `prototype`), `Temporal`, cada
  `Temporal.X`, `Temporal.Now`, `WebAssembly` e cada classe, e 16 objetos sem nome global, escritos como
  `%Nome%` (`%ArrayIteratorPrototype%`, `%MapIteratorPrototype%`, `%SetIteratorPrototype%`,
  `%StringIteratorPrototype%`, `%RegExpStringIteratorPrototype%`, `%GeneratorFunction%`,
  `%GeneratorFunctionPrototype%`, `%GeneratorPrototype%`, as quatro variantes Async, `%AsyncIteratorPrototype%`,
  `%TypedArray%`, `%TypedArrayPrototype%`).
- `tests/golden/own_keys_bun.json` regerado. `tests/builtin_own_keys_golden.rs` ganhou a tabela `specials` no
  `ORDERED_PROGRAM` (mesma do gerador) para resolver o primeiro segmento do caminho.
- Não rodei cargo: o teste pode acusar linhas novas (inclusive `ausente`) para tudo o que o porte ainda não instala.
  Membros que só existem no bun (`WebAssembly.compileStreaming`, `instantiateStreaming`, `promising`, `Suspending`,
  `SuspendError`) devem entrar em `bunOnly` se o porte não os tiver.

## Correções feitas

- `generator_prototype.rs`: `next`, `return`, `throw` (tabela estática) agora vêm antes do `@@toStringTag`.
- `async_generator_prototype.rs`: ordem `return`, `throw`, `next`, `@@toStringTag` (bun: `return,throw,next,constructor,@@toStringTag`).
  O comentário DIVERGÊNCIAS do cabeçalho ainda descreve a ordem antiga de `next`; atualizar.

## Divergências que sobraram (não corrigidas)

- `%GeneratorPrototype%` e `%AsyncGeneratorPrototype%`: no bun `constructor` fica ANTES de `@@toStringTag`; no porte
  o `constructor` é posto depois pela criação de `GeneratorFunction.prototype`, então o `@@toStringTag` precede.
  Precisa do `@@toStringTag` ser instalado depois do `constructor` (mexe na ordem de criação no global).
- `WeakMap.prototype` no bun: `delete,get,has,set,getOrInsert,getOrInsertComputed,constructor,@@toStringTag`.
  `weak_map_prototype.rs` define `getOrInsert`; conferir se `getOrInsertComputed` existe e a ordem.
- Não verificadas (sem tempo): Intl.*, Temporal.*, WebAssembly.*, iteradores Map/Set/String/RegExp, TypedArray,
  Proxy (`length,name,revocable`), WeakRef, FinalizationRegistry. O próprio teste vai listá-las.
- Sem `wasm`/`WebAssembly` além de `wasm_errors.rs` em `src/runtime`: provavelmente ausente por inteiro.
  (Superado abaixo: existe em `js_web_assembly.rs`.)

## Fechamento das pendências (sem cargo, não compilado)

- Generator e AsyncGenerator: o `@@toStringTag` saiu de `GeneratorPrototype::create` e `AsyncGeneratorPrototype::create`
  e passou a ser gravado por `link_generator_prototype` (`function_kind_intrinsics.rs`, novo parâmetro `to_string_tag`)
  logo depois do `constructor`. Ordem esperada: `next,return,throw,constructor,@@toStringTag` e
  `return,throw,next,constructor,@@toStringTag` (medida no bun 1.4.2). Imports mortos removidos.
- `Map.prototype` e `WeakMap.prototype` já têm `getOrInsert` e `getOrInsertComputed` no porte
  (`map_prototype.rs:191`, `weak_map_prototype.rs:112`), e o bun também os tem; nada a implementar. Conferir só a ordem
  no teste (bun Map: `...,set,getOrInsert,getOrInsertComputed,size,values,constructor,@@iterator,@@toStringTag`).
- Cabeçalho de `async_generator_prototype.rs` atualizado.
- `WebAssembly` existe (`install_web_assembly`, atrás de `Options::use_wasm`) e já instala `compileStreaming` e
  `instantiateStreaming` (length 1, rejeitam sem `Response`, como o bun). Faltam no porte `JSTag`, `promising`,
  `Suspending` e `SuspendError`: ficam em `bunOnly["WebAssembly"]` em `tests/builtin_own_keys_golden.rs`
  (JSPI e tag de exceção JS; no bun `promising.length` 0, `Suspending.length` 1, nome `WebAssembly.Suspending`).

## Intl: ordem de construtor e protótipo (sem cargo, não compilado)

- Golden do bun: nos construtores Intl (Collator, DateTimeFormat, DisplayNames, DurationFormat, ListFormat, NumberFormat,
  PluralRules, RelativeTimeFormat, Segmenter) `supportedLocalesOf` vem ANTES de `length,name,prototype`; nos protótipos os
  membros (`compare`, `format`, `of`, `select`, `segment`, `maximize`...) vêm ANTES de `constructor`.
- Correção em `IntlClass::install_with` (`intl_support.rs`): o construtor passa a nascer por
  `create_collection_constructor_with` (novo, `collection_support.rs`, gancho `before_finish` antes do `finish_creation`
  de length/name) e `supportedLocalesOf` entra nesse gancho. Os nove `install_xxx` (e Locale) agora põem os membros do
  protótipo dentro do closure de `install_with`, que roda antes do `constructor`.
- A ordem exata dos membros seguintes (`resolvedOptions`, `formatToParts`...) não foi conferida contra o golden; o teste dirá.

## BigInt e globalThis (2026-10-08)

- `BigInt` no bun: `asUintN, asIntN, length, name, prototype`. `BigIntConstructor::finish_creation` agora reifica
  `length` e `name` logo depois da tabela (`reify_lazy_property_if_needed`) e antes de `prototype`.
- `globalThis` no bun (JSC puro, medido com `vm.runInThisContext`): `Infinity, undefined, NaN, isNaN, isFinite, escape,
  unescape, decodeURI..., eval, globalThis, parseInt, parseFloat, ArrayBuffer...`. A propriedade `globalThis` saiu do
  `JSGlobalObject::finish_creation` e entra em `js_global_object_functions_natives.rs` logo depois de `eval`, e `eval`
  passou a ser criado antes de `parseInt`/`parseFloat`. O teste já filtra as chaves de host do bun pela lista `only`
  do golden, então não precisou de ajuste.
- Não validado (sem cargo): rodar `builtin_own_keys_golden` para conferir.

## Temporal e WebAssembly: tabela estática antes de length/name/prototype (sem cargo, não compilado)

- Padrão (já usado por `supportedLocalesOf` do Intl): `create_collection_constructor_with(..., before_finish)` põe as
  entradas da tabela estática no construtor ANTES de `finish_creation` (`length`, `name`) e de `prototype`.
  Aplicado aos sete construtores `temporal_*_constructor.rs` (`from,compare`; `PlainMonthDay` só `from`) e a
  `create_instant_constructor` em `temporal_instant.rs` (`from,fromEpochMilliseconds,fromEpochNanoseconds,compare`).
- `IntlClass::install_with_statics(global, ns, statics, members)` (novo em `intl_support.rs`): `statics` roda no
  construtor depois de `supportedLocalesOf`; `install_with` delega. `WebAssembly.Module` usa `statics`
  (`customSections,imports,exports`); `Instance`, `Memory`, `Table`, `Global` passam os métodos/acessores em `members`,
  que entram no protótipo antes de `constructor` (`exports`; `grow,buffer,...`; `length,grow,get,set`; `valueOf,value`).
  Removida `prototype_constructor` (morta).
- `WebAssembly.Suspending` (bun): `name` do construtor é `"WebAssembly.Suspending"` (length 1, prototype ReadOnly) e o
  protótipo NÃO tem `constructor` próprio nem `@@toStringTag`. `install_jspi` regrava o `name` e apaga o `constructor`.

## Golden object_edge e array_edge contra o bun (2026-10-08, sem cargo)

Corrigido (não compilado ainda, rodar `cargo test --test object_edge_bun_golden --test array_edge_bun_golden`):
- `src/runtime/js_array.rs`: `primitive_to_number` virou o `toNumber` completo (string e objeto chamam
  `valueOf`/`toString`, exceção pendente vira `PutError::Pending`); novo `array_length_from_value` faz
  `toUInt32` e depois `toNumber` (duas conversões, como `JSArray::put` e `defineOwnProperty`), usado nos dois
  caminhos. Cobre `defineProperty([],'length',{value:'3'})` e `{valueOf}`.
- `src/runtime/array_prototype.rs`: `index_u32` agora devolve `Option<u32>` (acima de `u32::MAX` o índice é um
  nome comum, `Identifier::from_double`, como o `Identifier::from(vm, uint64_t)` do C++). `has_index`,
  `get_index`, `get_property`, `put_index`, `delete_index` e `create_data_property_at` ganharam o caminho por
  `PropertyName` (splice/copyWithin/pop/indexOf com length 2**53-1). O `put_by_index` do `JSObject` já tratava
  `u32 > MAX_ARRAY_INDEX` por nome.

Não resolvido, achados:
- (1) `defineProperty(function f(a,b){}, 'length'|'name', {value})` não aplica o valor: não investiguei
  `JSFunction::defineOwnProperty` (reifyLength/reifyName); próximo passo é conferir se o caminho
  `js_function_reify.rs` roda antes do `define_own_property` genérico.
- (4) `sort`/`toSpliced`/`includes`/`indexOf` sobre Proxy de array logam só `get <método>` e `get length`, sem
  `has`/`get` dos elementos. O código de `array_prototype.rs` já é genérico (`get_property`/`get_index`) e o
  despacho de proxy existe em `get_property_slot_by_index`, então a suspeita é `to_length` voltando 0 para
  Proxy (`object.get` -> `slot.get_value_for` no caminho do proxy não devolve o valor do trap) ou
  `dense_snapshot`/`from_cell_id` aceitando o Proxy. Primeiro teste: `Array.prototype.includes.call(new Proxy([1],{}), 1)`.
- Demais divergências do object_edge (75) não lidas a fundo por falta de tempo.

## Rodada de ordem: globalThis, BigInt, Intl (2026-10-08, sem cargo, não compilado)

- `BigInt`: o bun lista `asUintN`, `asIntN`, `length`, `name`, `prototype`. O porte criava o construtor com
  `JSFunction::create_native`, que já deixa `length` na frente (por isso o `Number` bate com `length,name` primeiro),
  e o reify posterior não mudava nada. O C++ é um `InternalFunction`: `bigint_constructor.rs` agora usa
  `InternalFunction::new` com `BIG_INT_CONSTRUCTOR_S_INFO` (como `ArrayConstructor`), instala a tabela antes de
  `finish_creation(1, "BigInt", WithoutStructureTransition)` e só depois `prototype`. `create` ganhou o parâmetro
  `function_prototype` (chamada em `js_global_object_init.rs` atualizada). Risco a conferir no cargo: `BigInt`
  deixa de ser `JSFunction` (código que fizesse downcast do construtor BigInt para `JSFunction`).
- `Intl.NumberFormat.prototype` e `Intl.DateTimeFormat.prototype`: ordem do bun `format`, `formatRange`,
  `formatRangeToParts`, `formatToParts`, `resolvedOptions`; os `install_with` instalavam `formatToParts` antes.
- `globalThis`: `add_global_functions` (isNaN...parseFloat) era chamada depois de String/RegExp/Error; agora logo
  depois de `add_function_properties`, antes de qualquer construtor. A ordem completa do bun é `Infinity,
  undefined, NaN, isNaN, isFinite, escape, unescape, decodeURI, decodeURIComponent, encodeURI,
  encodeURIComponent, eval, globalThis, parseInt, parseFloat, ArrayBuffer, EvalError, RangeError, ReferenceError,
  SyntaxError, TypeError, URIError, AggregateError, Proxy, Reflect, JSON, Math, Atomics, Int8Array...BigUint64Array,
  DataView, Date, Error, Boolean, Map, Number, Set, WeakMap, WeakSet, WeakRef, FinalizationRegistry, Object,
  Function, Array, RegExp, Iterator, SharedArrayBuffer, String, Promise, BigInt, Symbol`. Ou seja, depois de
  `parseFloat` o porte ainda diverge (String, RegExp, Error, Symbol, Object, Array, Number, Boolean, BigInt vêm
  na frente de ArrayBuffer/Date/Math...). Próximo passo provável: mover os `put_direct` do global dos
  construtores para uma sequência final na ordem do bun (os objetos podem continuar sendo criados na ordem atual;
  só a gravação da propriedade global precisa seguir a lista), ou instalar `ArrayBuffer`, erros nativos, Proxy,
  Reflect, JSON, Math, Atomics, typed arrays e DataView antes dos demais.
- O teste (`tests/builtin_own_keys_golden.rs`) já relata todos os objetos divergentes numa rodada (um assert no fim);
  só o primeiro índice divergente por objeto era mostrado. Agora cada divergência de ordem também imprime a ordem
  obtida e a esperada inteiras, então a próxima rodada mostra a lista completa do `globalThis`.

## Função e Proxy nos edge goldens: raiz achada (2026-10-08, sem cargo, não compilado)

- Causa de `defineProperty(function,'length'|'name')` não aplicar o valor (e de `seal`/`freeze` de função lançar
  "not extensible"): `ObjectRef::from_value` tentava `JSObject::from_value` primeiro, e `as_js_object` do
  `cell_registry` faz `Deref` de `CellEntry::Function` até `JSObject`, então toda função virava `ObjectRef::Handle` e
  o `JSFunction::defineOwnProperty`/`getOwnPropertySlot` (reify de `length`/`name`/`prototype`) nunca rodava por esse
  caminho. Correção: `host_function_support.rs` testa `as_js_function` ANTES. Vale para todo `ObjectRef::from_value`.
- Causa de `sort`/`toSpliced`/`includes`/`indexOf` em Proxy logarem só `get length`: os métodos de `JSObject`
  (`get`, `has_property*`, `put*`, `delete_property*`) não despacham trap; só as funções livres de `proxy_object.rs`
  fazem. `array_prototype.rs` ganhou `is_proxy` e os helpers `to_length`, `has_index`, `get_index`, `get_property`,
  `put_index`, `delete_index`, `set_length` roteiam por `object_get/object_has_property/object_set/object_delete_property`
  quando o objeto é Proxy. Falta: `create_data_property_at` com alvo Proxy (`put_direct`) e `concat` com
  `Symbol.isConcatSpreadable` false em Proxy (devolve `[]`, `length` undefined: mesmo problema, em `concat`).
- Agrupamento das 75 do object_edge restantes (não corrigidas):
  1. ~40: mensagem `"X is not an object (evaluating 'f()')"` (bun) contra `"X is not an object"` (nós), em
     `Object.getPrototypeOf/keys/hasOwn/getOwnPropertyNames/defineProperties/create(null,null)`,
     `__defineGetter__/__lookupGetter__` e `Object.prototype.*.call(null)`: falta o sufixo ` (evaluating 'f()')` na
     mensagem de `toObject` desses chamadores (o texto vem do call site do bun, `f()` do harness).
  2. `Reflect.get/has([..],'length')` dá undefined/false e `Reflect.set(Proxy({a:1}),'a',2)` dá false: o caminho do
     Reflect não vê `length` de array nem faz set em Proxy sem trap (receiver = o próprio Proxy).
  3. `Reflect.defineProperty` com `length` inválido deve lançar RangeError; `length` não gravável não bloqueia índice
     novo; `a.length=0` com elemento não configurável deve falhar (`JSArray::defineOwnProperty` não aplica no Reflect).
  4. `freeze`/`seal` de `String` object, `RegExp` (`lastIndex` fica writable), TypedArray resizável (mensagem
     `Unable to prevent extension in Object.freeze`); depois do fix de função rever.
  5. `Map.groupBy` ausente, `URLSearchParams` ausente (host), ordem de chaves de `JSON` e `Symbol`
     (`isRawJSON,rawJSON,stringify,parse` e `for,keyFor` por último), `Reflect.construct(Uint8Array,[2],Array)`.
  6. `__defineGetter__` em Proxy não passa pelo trap `defineProperty`; em array `length` deve lançar.
- Do array_edge: 7 de 11 são índice acima de `MAX_ARRAY_INDEX` num ponto que ainda panica em
  `host_function_support.rs:309` (provável `put_by_index` de caminho não coberto pelo `index_u32`); 2 Proxy (corrigidas
  acima); 1 `concat` de Proxy; 1 `indexOf` com `length` 2**53+100.

## Ordem do globalThis: sequência final fiel ao bun (2026-10-08, sem cargo, não compilado)

- Qual das duas hipóteses explica o bun: a lista do bun (Infinity, undefined, NaN, funções globais, `ArrayBuffer`, erros
  nativos, `Proxy`, `Reflect`, `JSON`, `Math`, `Atomics`, typed arrays, `DataView`, `Date`, `Error`, `Boolean`, `Map`,
  `Number`, ... `Symbol`) não é ordem de criação nem a de um único array `GlobalPropertyInfo`: mistura a tabela
  estática do `JSGlobalObject::init` com os `LazyClassStructure`/`putDirectCustomAccessor` instalados por
  `initStaticGlobals`/`addConstructors` do próprio bun. Não foi possível derivar a ordem só do `JSGlobalObject.cpp`
  do upstream, então a medição do bun é a fonte de verdade (a lista está na rodada anterior).
- Implementação: `reorder_standard_globals` em `src/runtime/js_global_object_init.rs`, chamada logo depois de
  `init_promise` (último ponto em que um global padrão entra). Os objetos continuam nascendo na ordem das dependências;
  cada nome da lista é removido do global (`remove_property_transition`, ou `remove_property_without_transition` em
  dicionário, ignorando `DontDelete` como o init faz) e regravado com `put_direct` e os mesmos atributos, na sequência
  da lista. Acessor, `CustomAccessor`, `CustomValue` e `PropertyCallback` ficam onde estão. Nomes ausentes no porte
  são pulados; globais de host ficam antes dos reordenados (o golden filtra pela lista `only`).
- Riscos a conferir no cargo: o global vira dicionário (estrutura não cacheável) depois das remoções, então qualquer
  cache de offset de global tirado antes disso fica inválido; confirmar `Reflect.ownKeys` no
  `tests/builtin_own_keys_golden.rs` e a suíte de escopo global (`var` global, `globalThis.x`). `Float16Array` está
  na lista por posição natural; se o bun não a tiver, é ignorada pelo filtro.

## Reflect, defineProperty de array, preventExtensions de TypedArray redimensionável (2026-10-08, sem cargo, não compilado)

- `Reflect.get/has([..],'length')` (builtins JS `target[key]` e `key in target`): `JSObject::get_own_property_slot` não
  despachava para `JSArray::get_own_property_slot` (o `length` é virtual, vem do butterfly). Corrigido em `js_object.rs`
  com despacho só para o nome `length` em `ArrayType` (sem laço: o `JSArray` responde `length` sozinho).
- `Reflect.defineProperty` (e todo `object_define_own_property`) ignorava `JSArray::defineOwnProperty`: só
  `object_constructor::define_own_property_of` o chamava. Agora `object_define_own_property` (proxy_object.rs) despacha para
  o `JSArray`, o que traz RangeError de `length` inválido, encolhimento bloqueado por índice não configurável e a recusa
  de índice novo com `length` não gravável.
- `Reflect.set` com receptor diferente do alvo (inclusive o `Proxy` sem trap `set`, receptor = o próprio Proxy):
  `object_set` agora usa `ordinary_set_slow` quando `is_this_value_altered`, em vez de `put_through_method_table`.
- `Object.freeze/seal/preventExtensions` de TypedArray redimensionável ou de comprimento automático:
  `object_prevent_extensions` devolve `false` via `typed_array_dispatch::refuses_prevent_extensions` (como
  `JSGenericResizableOrGrowableSharedTypedArrayView::preventExtensions`), e `set_integrity_level` lança
  "Unable to prevent extension in Object.freeze|seal".
- RegExp `lastIndex` no `freeze`: o despacho de `define_own_property`/`put`/`delete` já existe em `js_object.rs`
  (regexp-lastindex-put.md); nada novo, conferir no golden depois do build.
- array_edge `host_function_support.rs:309` ("índice acima de MAX_ARRAY_INDEX (nome de propriedade por string)"): essa
  mensagem já não existe no código (removida pela correção de `index_u32` em `array_prototype.rs`); o golden em
  /tmp era anterior. Restam `Unported` equivalentes só em `js_generic_typed_array_view.rs:271,297`
  (`setFromArrayLike` com índice acima de `MAX_ARRAY_INDEX`).
- Risco a conferir no build: o novo `ordinary_set_slow` em `object_set` pode mudar o caminho de `Reflect.set` com receptor
  de `TypedArray` na cadeia (hoje `Unported` em `ordinary_set_with_own_descriptor`).

## setFromArrayLike, TypedArray na cadeia do [[Set]], varredura de Unported (2026-10-08, sem cargo, não compilado)

- `setFromArrayLike` (`js_generic_typed_array_view.rs`, as duas variantes): o `Unported` de índice acima de
  `MAX_ARRAY_INDEX` saiu. Novo `get_object_index_u64` em `string_regexp_support.rs` (o `get(globalObject,
  Identifier::from(vm, index))` do C++): índice até `MAX_ARRAY_INDEX` segue `get_object_index`, acima disso vira o
  `Identifier` decimal e passa por `JSObject::get`. A validação de intervalo, o `ToLength` do `length`, o RangeError
  "Offset out of range" e o `set_index` (que coage com `ToNumber`/`ToBigInt` e trata destacado/fora do limite) já
  estavam portados e ficam como estão.
- `ordinary_set_with_own_descriptor` (`proxy_object.rs`): o ramo `holder.is_some() && is_typed_array_type(...)` agora
  chama `typed_array_dispatch::put(current, ...)` (10.4.5.5): nome numérico canônico com receptor diferente devolve
  `true` se o índice é inválido (destacado ou fora do limite), senão cai no `OrdinarySet` do próprio TypedArray; receptor
  igual ao TypedArray grava o elemento; nome não numérico (`None`) segue o laço comum. `PutError` volta como `Thrown`
  pelo `From<PutError> for Thrown` existente. A recursão termina: `JSGenericTypedArrayView::put` chama
  `ordinary_set_slow` com o próprio TypedArray como objeto inicial (`holder` vazio), então o ramo não se repete.
- Conferir depois do build: `Reflect.set(ta, 0, v, {})` (grava no receptor, não no ta), `Reflect.set(ta, 99, v, {})`
  (`true` sem gravar), `Object.create(ta).x = 1` e `Object.create(ta)[0] = 1` (índice válido grava no filho, inválido
  é ignorado), e `Reflect.set(ta, 0, v, ta)`.
- Varredura de `Unported` em `src/runtime` e `src/llint` (170 ocorrências fora os dois pontos acima). A grande maioria
  é invariante interna do registro de células (callee/escopo/CodeBlock/JSCellButterfly que o registro não expõe como
  objeto, opcode sem handler no getter/setter), não alcançável por JS comum. O que um JS comum poderia alcançar:
  1. `temporal_calendar.rs:149`: calendário diferente de ISO 8601 (`new Temporal.PlainDate(..., 'gregory')` e afins);
     depende de ICU/Intl, não é porte simples.
  2. `js_object.rs:552` e `:1841`: `getPrototype`/`isExtensible` sobrescritos por tipo que não é Proxy nem
     JSGlobalProxy (hoje só existem esses dois tipos com o flag, então inalcançável até outro tipo ganhar o flag, por
     exemplo o objeto de namespace de módulo).
  3. `shadow_realm_*` (`moveFunctionToRealm` com argumento que não é `JSFunction`, callee que não é objeto): só com
     `ShadowRealm` habilitado.
  4. `proxy_object.rs:1434` e `:1454` (protótipo ou receptor "que não é `JSObject::from_value`"): a mensagem cita
     `JSFunction`, mas `from_value` já cobre a célula de função por `Deref`; só dispara para célula fora do registro
     (escopo), então não é alcançável por JS. A mensagem está desatualizada, vale limpar.
  5. `collection_support.rs:92`, `varargs.rs`, `dispatch.rs`, `handlers_*`: invariantes.
  Nenhum item simples o bastante restou para portar agora sem cargo; os pontos 1 e 3 são os candidatos reais.

## Correções após o golden de `Reflect.ownKeys` (globalThis e WebAssembly)

- `globalThis` posição 29: a lista `ORDER` de `reorder_standard_globals` estava errada, não o mecanismo. A ordem
  medida no bun é `Int8Array, Int16Array, Int32Array, Uint8Array, Uint8ClampedArray, Uint16Array, Uint32Array,
  Float16Array...`; a lista tinha `Int8, Uint8, Uint8Clamped, Int16, Uint16, Int32, Uint32`. Lista corrigida (o resto da
  ordem obtida já batia com a esperada).
- WebAssembly enumerável: os membros de `Module` (`customSections`, `imports`, `exports`), `Instance.prototype.exports`,
  `Memory.prototype` (`grow`, `buffer`, `toFixedLengthBuffer`, `toResizableBuffer`), `Table.prototype` (`length`, `grow`,
  `get`, `set`) e `Global.prototype.valueOf` usavam `put_method_on`/`put_getter_on` (DontEnum). Agora usam
  `put_enumerable_method` (atributos 0) e o novo `put_enumerable_getter` (só `ACCESSOR`), ambos em `js_web_assembly.rs`.
- `WebAssembly.Suspending.prototype`: o relatório `-constructor` significa FALTA (o "-" é do que o bun tem e nós não).
  O golden do bun tem `constructor` (`["constructor"]`, w=true, e=false, c=true, name `WebAssembly.Suspending`). O
  `install_jspi` apagava a propriedade por engano; removida a chamada `delete_property`, mantendo o ajuste do `name` do
  construtor. Ainda sem rodar o cargo: confirmar no próximo ciclo de teste.

## object_edge golden: 46 divergências agrupadas por raiz (2026-10-08, sem cargo, não compilado)

1. `freeze`/`seal`/`Reflect.*`/`delete` em `String` object (9 casos, corrigidos): o porte não tinha
   `StringObject::defineOwnProperty` nem `deleteProperty` por nome. `set_integrity_level` redefinia o índice `0` e o `length`
   com `{configurable:false}`, caía no `JSObject::defineOwnProperty` base e lançava "not extensible" (o objeto já estava
   sem extensão). Em `js_object.rs`: novo `string_object_of()`; `define_own_property` ganhou o ramo de
   `StringObject::defineOwnProperty` (nome `isStringOwnProperty`: `getOwnPropertyDescriptor` +
   `validate_and_apply_property_descriptor(object = None)`, com o `isExtensible` do objeto); `delete_property` devolve `false`
   para `length` e índice dentro da string (`StringObject::deleteProperty`); `delete_property_by_index` usa o
   `StringObject::delete_property_by_index` que já existia em `string_object.rs` e não era chamado.
   O mesmo conserta `Reflect.deleteProperty(new String('s'),'0')` (esperado `false`, vinha `true`). Os `arguments` do
   pedido não aparecem neste golden (nenhuma divergência de arguments).
2. `Reflect.construct(BigInt,[1],Object)` (corrigido): `BigIntConstructor` era criado com `construct = None`, então
   `isConstructor` dava `false` e o erro era "requires the first argument be a constructor". No C++ o `BigInt` tem
   `constructBigIntConstructor` (`createNotAConstructorError(callee)`); agora usa `symbol_constructor::construct_symbol`,
   que é o mesmo corpo.
3. 32 divergências são uma só raiz, ainda NÃO corrigida: a mensagem de `TypeError` de função nativa chamada em posição
   de cauda. O programa é `T(()=>Object.keys(null))` em strict mode: o `op_tail_call` do arrow tem callee nativo, o JSC
   elimina o frame do arrow (tail call) e o texto `(evaluating '...')` vem do chamador do arrow, `f()` dentro de `T`.
   O porte (`dispatch.rs`, `call_prepared_frame`) monta `ErrorSite { code_block, pc }` do frame que fez a tail call e
   deixa o frame do chamador no lugar (doc do módulo: "com callee nativo o frame do chamador fica"). Correção: com
   `tail == true` e callee que não é função de script, o `ErrorSite` (e o `current_vpc` visível ao percurso de pilha) deve
   ser o do `caller_frame` do `call_frame` (o frame que chamou o arrow), pulando o frame do arrow, como o
   `prepareForTailCall` faz. Afeta `defineProperties`, `getOwnPropertyDescriptors`, `getOwnPropertyNames`, `keys`,
   `getPrototypeOf`, `hasOwn`, `__defineGetter__` e família, `isPrototypeOf`, `toLocaleString`, `valueOf`,
   `Reflect.construct(Symbol...)`. Precisa de compilação para acertar a obtenção do `CodeBlock` do chamador.
4. `Reflect.ownKeys(JSON)` e `Reflect.ownKeys(Symbol)` (2 casos): NÃO são bug do porte. Os valores esperados
   (`isRawJSON,rawJSON,stringify,parse` e `for,keyFor` no fim) são a reificação preguiçosa da tabela estática do JSC no
   processo do gerador do golden (as propriedades de tabela entram na ordem do primeiro acesso, depois das de
   `putDirect`). Em estado fresco o bun mede `parse,stringify,isRawJSON,rawJSON` e `for,keyFor,length,name,...`
   (`tests/golden/own_keys_bun.json`), que é o que o porte produz. Esses dois casos do `object_edge` dependem do que o
   gerador executou antes; regerar o golden com cada caso em processo novo (como o `own_keys`) ou remover os dois.
5. `Map.groupBy` (4 casos: "undefined is not a function"): a instalação existe (`js_global_object_init.rs`, JS embutido
   `MapConstructorGroupByCode`, idêntico ao upstream, usa `new @Map`, `groups.@get`, `groups.@set`). A causa não foi
   achada por leitura (sem cargo). Hipótese a medir: o símbolo privado `@get`/`@set` do protótipo de `Map` ou
   `@MAX_SAFE_INTEGER` resolvido como `undefined` dentro do builtin; `Object.groupBy` usa o mesmo laço sem esses nomes.
   Próximo passo: `Map.groupBy([1],x=>x)` com `@throwTypeError` intermediário, ou dump do bytecode do builtin.
   **Causa achada (por leitura, sem cargo):** colisão de nomes no gerador de `builtin_names`. O C++ tem `Set` e `set`
   (`SetPrivateName()` e `setPrivateName()`); no porte a maiúscula ficou com `set_private_name()` e a minúscula virou
   `set_dup_private_name()`. `MapPrototype` e os cinco sítios do `NodesCodegen` (acessor privado `#x` com setter, em
   `nodes_codegen_cpp1b/cpp2/cpp3b`) chamavam `set_private_name()` (o de `Set`), então `Map.prototype.@set` nunca
   existia e `groups.@set(...)` dava "undefined is not a function". Corrigido para `set_dup_private_name()`.
   `@get`, `@MAX_SAFE_INTEGER`, `@Map`, `@call` conferem. `Object.groupBy` não usa `@set`, não precisava de mudança.
   Falta rodar o golden para confirmar. Atenção: `promise_dup` (`Promise`/`promise`) tem o mesmo risco, sem chamador hoje.
6. `URLSearchParams` (1 caso): API de host, fora do escopo.

## Atomics (42 de 5220) e async generator (4 de 420), 2026-10-08

Atomics, duas raízes, nenhuma é bug do porte:
1. `structuredClone` (17 casos, `ReferenceError: structuredClone is not defined`): API de host do bun, fora do escopo.
2. `Atomics.add is not a function ... 'Atomics.add' is 1` (25 casos): contaminação de estado do gerador do golden.
   `tests/golden/atomics_bun.tsv` tem programas (linhas ~3285 em diante) que fazem `Atomics.add = 1`; o bun
   rodou o resto no mesmo processo e as respostas seguintes saíram com `Atomics.add` já sobrescrito. Em reino novo o
   porte devolve o valor correto (`2`, `[16,0,1]`, o erro de buffer destacado). Mesma família do item 4 acima:
   regerar o golden com cada caso em processo novo, ou remover os casos posteriores ao que sobrescreve `Atomics.add`.

Async generator, uma raiz em dois sintomas (corrigida, sem compilar):
1. `for await (x of {[Symbol.asyncIterator]: 1})` e `yield*` com `next: 1`: `create_not_a_function_error(..., None)` caía
   em `native_call_site()`, que aponta o frame de fora (o `(0, eval)(src)` do harness). O C++ usa o `topCallFrame`, ou
   seja a instrução em curso. `open_iterator_call` (`handlers_iterator.rs`) e `async_iterator_next`
   (`handlers_async.rs`) agora passam `Some(&f.error_site())`, o que dá `1 is not a function (near '...')`.
2. `yield*` com `get next() { throw }`: `get_property` de `slow_paths_control.rs` não checava exceção pendente, então
   `slow_path_iterator_open_get_next` gravava `undefined` em `next` e devolvia `Ok`; o erro real (`gn`) se perdia e
   aparecia depois `g1.next is not a function`. `get_property` agora devolve `ControlResult` e propaga a exceção
   (`next`, `done`, `value`). Não compilado: rodar `async_gen_bun_golden` para confirmar.

Fora do que foi pedido e não verificado: o `DataView getFloat16/setFloat16` e a fila do `AsyncGeneratorPrototype.js`
não aparecem em nenhuma divergência destes dois goldens.

Acessores (`accessor_bun_golden`, 28 de 1018 em `/tmp/now3_accessor_bun_golden.txt`), quatro raízes, sem compilar:
1. Contaminação de estado do gerador (11 casos, "Object is not defined" e "Attempted to assign to readonly property"):
   `scripts/gen-accessor-golden.js` rodava tudo num processo só e dois programas vazavam estado: `delete globalThis.Object`
   (apagava o `Object` para os 8 casos seguintes) e `defineProperty(Array.prototype,'0',{writable:false})` seguido de
   `a.push(1)` que lança antes do `delete Array.prototype[0]` (o `push` de todo programa seguinte lançava). Os dois agora
   restauram em `finally`. Também saiu o `structuredClone===undefined?0:` (API de host do bun). `tests/golden/accessor_bun.tsv`
   foi regerado com `bun scripts/gen-accessor-golden.js` (1018 programas, o resto da coluna de resultado idêntica).
2. Pânico `assertion failed: i < storage.length()` (4 casos, `[0,,2]` com getter em `Array.prototype[1]`, que põe o VM em
   "bad time" e faz o literal virar `ArrayStorage`): `JSObject::set_index_quickly` no ramo `ArrayStorage` só afirmava; o C++
   (`setIndexQuicklyForArrayStorageIndexingType`) conta o valor novo em `m_numValuesInVector` e estende `length` quando o slot
   estava vazio. Corrigido em `js_object.rs`; o `initializeIndex` (que no C++ só grava e afirma) ganhou o ramo próprio em
   `JSArray::initialize_index`, senão `tryCreateUninitializedRestricted` (que já deixa `numValuesInVector = length`)
   contaria duas vezes.
3. `new String('ab')`: `s[0]='z'`, `s.length=5` em modo estrito não lançavam. `StringObject::put`/`putByIndex` não tinham
   despacho: `JSObject::put` agora lança `ReadonlyPropertyWriteError` para `length`, e `put_by_index` para índice de caractere
   (`can_get_index`). `delete s[0]`/`delete 'abc'.length`, `defineProperty(s,0,{value})` e o `'f()'` do texto de erro com
   `tail call` nativo já estavam no código (`delete_property`, `define_own_property`, `tail_caller_site`): o build do
   `/tmp/now3` é anterior a eles. Confirmar no próximo build.
4. Os 6 `(evaluating 'f()')` de `Object.defineProperties({},undefined)`, `__defineGetter__.call(null)` etc.: tail call
   estrito para função nativa (`handle_host_call` com `tail`), já coberto por `tail_caller_site`; mesma observação do item 3.

Nota: o `tests/golden/accessor_bun.tsv` foi escrito por redirecionamento do `bun` (geração de código), não por Write/Edit.
