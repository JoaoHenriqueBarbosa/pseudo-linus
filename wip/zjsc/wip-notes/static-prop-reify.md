# Plano: propriedades estáticas preguiçosas (`staticPropHashTable`, reify)

Só plano, nada implementado. Medido no bun 1.4.2 (scripts descartáveis em /tmp) e lido no upstream
(`JSObject.cpp`, `JSObjectInlines.h`, `Lookup.h`, `Lookup.cpp`, `Structure.h`).

## 1. Problema

O porte põe as entradas das tabelas `.lut.h` na `Structure` na criação (`put_direct_*_without_transition`).
No JSC elas ficam fora da `Structure` até o primeiro acesso. Efeito no dump do `e2e_bytecode` para `Array`:

- esperado `(8/8){length, name, prototype, Symbol.species, of, isArray, @from, fromAsync}` (o `from` do dump é o
  nome privado `@from` impresso sem o `@`; o `from` público da lut NÃO está lá);
- o porte tem o `from` público eager em `array_constructor.rs` (`create`, antes do `finish_creation`), logo 9
  propriedades e capacidade 16.

## 2. Semântica do JSC (o que precisa ser reproduzido)

`ClassInfo.staticPropHashTable` + `TypeInfo::HasStaticPropertyTable` + bit `Structure::staticPropertiesReified`
(o porte já tem o bit em `structure.rs:224`, copiado na transição em `:338`, e o flag `HAS_STATIC_PROPERTY_TABLE`
em `js_type_info.rs:11`). `hasNonReifiedStaticProperties() = flag && !bit`.

| Operação | Upstream | Efeito |
|---|---|---|
| `getOwnPropertySlot` | `JSObject::getOwnPropertySlot` chama `getOwnStaticPropertySlot` quando a `Structure` não tem o nome (percorre `classInfo` e pais; `getStaticPropertySlotFromTable` devolve false se `staticPropertiesReified`) | `setUpStaticFunctionSlot` faz `reifyStaticProperty` (um `putDirect` com os atributos da lut) e devolve o slot. Se o nome já está na `Structure`, vale a `Structure`. |
| own keys | `getNonReifiedStaticPropertyNames` (JSObjectInlines.h:962) em `getOwnNonIndexPropertyNames` | Adiciona os nomes da lut ANTES dos da `Structure`, na ordem da tabela (ordem do fonte do `@begin`, não ordem de hash), pais depois do filho. O `PropertyNameArray` deduplica, então vale a 1a posição. Não reifica. Filtra DontEnum quando `mode == Exclude`; se a `Structure` sombreia o nome e é DontEnum no modo Exclude, pula. |
| `delete` | `JSObject::deleteProperty` (2373) | Se `hasNonReifiedStaticProperties` e o nome está na lut: DontDelete devolve false direto; senão `reifyAllStaticProperties`. Depois apaga normal. |
| `defineOwnProperty` / `put` | `putInlineFastReplacingStaticPropertyIfNeeded`, `JSObject::defineOwnProperty` via `getOwnPropertySlot` (reify do nome) | O nome é reificado antes de substituir/redefinir. |
| `reifyAllStaticProperties` | JSObject.cpp:2949 | Converte para dicionário, reifica cada entrada da lut que ainda não está na `Structure` (ordem da tabela, pais por último), liga o bit `staticPropertiesReified`. Chamado por `delete`, `Object.assign` (target/source), `copyDataProperties` (JSGlobalObjectFunctions.cpp:909/1003), `ObjectConstructor.cpp` 393/422/571. |
| `hasProperty` fast (`JSObjectInlines.h:357`) | `hasNonReifiedStaticProperties` + `table->entry(name)` | Conta a lut como existente. |
| `Structure` / IC | `CommonSlowPaths.h:173`, `ObjectConstructor.cpp` (fast paths desligados quando não reificado), `JSONObject.cpp:1524` | Nada de fast path de enumeração enquanto não reificado. |

## 3. Medição no bun 1.4.2

`Reflect.ownKeys` / `getOwnPropertyNames`, SEM acessar nada antes e DEPOIS de acessar:

```
Array (antes)            from,length,name,prototype,of,isArray,fromAsync,Symbol(Symbol.species)
Array (depois Array.from) idem (a ordem não muda: nome da lut vem primeiro e é deduplicado)
Array (fromAsync/isArray acessados antes) idem
Array (gOPD(Array,'from'), 'zzz' definido, defineProperty(Array,'from')) from,length,name,prototype,of,isArray,fromAsync,zzz
Object (antes e depois de Object.is/hasOwn/entries) getPrototypeOf,setPrototypeOf,getOwnPropertyDescriptor,...,fromEntries,length,name,prototype,hasOwn,groupBy
Date                      parse,UTC,now,length,name,prototype
```

Depois de `delete Object.keys` (nome da lut, dispara `reifyAllStaticProperties`), a ordem passa a ser a da
`Structure`: `length,name,prototype,hasOwn,groupBy,` + os já acessados na ordem de acesso (`defineProperty,
getOwnPropertyNames,getOwnPropertyDescriptor,is`) + o resto na ordem da tabela (`getPrototypeOf,setPrototypeOf,...`).
`delete Array.isArray` (fora da lut) NÃO reifica: a ordem segue com `from` na frente.

Conclusões: (a) o que o usuário observa em `ownKeys` já é igual ao do porte HOJE para `Array` (o porte pôs `from`
primeiro de propósito, comentário em `array_constructor.rs`), mas por acaso; (b) o que difere é a `Structure` real
(dump de bytecode, `inline capacity`, ICs) e a ordem pós-`delete`/`Object.assign`; (c) Math, JSON, Reflect, Atomics,
String, Symbol, Intl: bun lista tudo na criação ou na ordem da lut, sem diferença de ordem observável por
`ownKeys` entre antes e depois do acesso, porque lut vem primeiro e é deduplicada.

## 4. O que o porte tem hoje

- `ClassInfo { class_name, parent_class, inherits_js_type_range }` (`class_info.rs`): sem tabela. O cabeçalho
  diz que é divergência assumida.
- `HAS_STATIC_PROPERTY_TABLE` está nos `STRUCTURE_FLAGS` só do `DateConstructor` e do `JSGlobalObject`
  (o resto dos `*_prototype.rs` documenta que ficou de fora). O bit `static_properties_reified` existe mas nada o lê.
- `array_constructor.rs::create`: `from` público eager, antes do `finish_creation` (para o `ownKeys` sair com
  `from` na frente), depois `@from` e `fromAsync` eager como no upstream.
- Ganchos existentes: `JSObject::get_own_property_slot` (js_object.rs:986, já com cadeia de especiais),
  `own_property_names::get_own_non_index_property_names` (own_property_names.rs:152), `delete_property` (:1791),
  `put` (:1359), `define_own_property` (:2030), `has_property`/`has_own_property` (:1291/:1303).

## 5. Quem usa lut no upstream (candidatos; fonte `ClassInfo ... &xxxTable`)

Construtores: Array (`from`), Object, Number, String, Symbol, Promise, BigInt, Date, RegExp, Temporal*,
Intl*Constructor (Collator, DateTimeFormat, DisplayNames, DurationFormat, ListFormat, NumberFormat, PluralRules,
RelativeTimeFormat, Segmenter). Protótipos: Number, Boolean, BigInt, Symbol, String, Date, Error, Promise,
Generator, AsyncGenerator, IteratorHelper, DataView, Intl*Prototype, Temporal*Prototype, ShadowRealm.
Objetos: JSON, Reflect, Intl, Temporal, Temporal.Now, JSGlobalObject. (Math e Atomics NÃO têm lut: instalam em
`finishCreation`.)

Hoje o porte põe todos eager. Observável fora do dump: só a `Structure`/capacidade e a ordem pós-reify.
Prioridade: Array (único que o `e2e_bytecode` pega agora) → Object, Promise, Number, String, Symbol,
RegExp, Date, Error.prototype, os demais só quando um golden tropeçar (regra SEM DERIVA).

## 6. Estrutura de dados proposta

1. `ClassInfo` ganha `static_prop_hash_table: Option<&'static HashTable>` (campo novo, `None` nas ~todas as
   `static` atuais: o literal `ClassInfo { .. }` precisa de `..` ou do campo em cada uso; fazer `const fn new`
   e migrar só as usadas, as demais usam `static_prop_hash_table: None` via sed-free Edit, em fatias).
2. Novo módulo `runtime/lookup.rs` (porte de `Lookup.h`): `HashTable { entries: &'static [HashTableValue],
   class_for_this: &'static ClassInfo }`; `HashTableValue { key: &'static str, attributes: u32, intrinsic,
   kind: Kind }`; `Kind::{ Function{ host, length }, Builtin{ index: BuiltinCodeIndex, length }, Accessor{..},
   Constant(i32), LazyProperty(fn(&VM,&JSObject)->JSValue), CustomAccessor{..} }`. Busca por nome linear
   (tabelas pequenas; `table.entry(name)` pode virar `HashMap` estático depois se medir).
3. Ordem = ordem do fonte do `.lut.h` (`@begin`), já suficiente para o `ownKeys` medido.
4. Funções (todas no `lookup.rs`/`js_object.rs`, sem repasse):
   - `get_static_property_slot_from_table(vm, class_info, table, this, name, slot) -> bool`
   - `reify_static_property(vm, class_info, name, entry, this)` (cria a função/builtin e `put_direct` com os atributos)
   - `JSObject::reify_all_static_properties(vm)` (converte para dicionário se couber no porte; senão
     `put_direct` simples, o dicionário só importa p/ dump de `Structure`, ver risco 2)
   - `JSObject::get_non_reified_static_property_names(vm, names, mode)`
   - `JSObject::has_non_reified_static_properties()`
5. As `create` dos objetos migrados passam a registrar a tabela no `ClassInfo` e a NÃO fazer
   `put_direct` das entradas da lut (mantém só o que o `finishCreation` do C++ faz). As fábricas hoje em
   `*_ENTRIES`/`*Entry` (ex.: `DATE_CONSTRUCTOR_ENTRIES` em date_constructor.rs) já são o protótipo da tabela.

## 7. Pontos de hook (porte)

1. `JSObject::get_own_property_slot` (js_object.rs:986): depois da busca na `Structure` falhar e antes de
   devolver false, se `has_non_reified_static_properties()`, percorrer `class_info` e pais chamando
   `get_static_property_slot_from_table` (reifica um nome e preenche o slot; se exceção pendente devolve false).
2. `get_own_non_index_property_names` (own_property_names.rs:152): antes do laço da `Structure`, chamar
   `get_non_reified_static_property_names` (só se `!static_properties_reified`).
3. `delete_property` (js_object.rs:1791): como upstream (DontDelete retorna false direto; senão
   `reify_all_static_properties`).
4. `put` / `put_inline_fast` / `define_own_property`: como o `get_own_property_slot` já reifica o nome antes de
   `defineOwnProperty` ler o descritor atual (verificar que `define_own_property` passa por ele); `put` precisa do
   equivalente de `putInlineFastReplacingStaticPropertyIfNeeded` (reificar o nome da lut antes de substituir).
5. `has_property`/`has_own_property` (:1291/:1303): contar a lut como existente sem reificar (equivalente de
   JSObjectInlines.h:357), ou simplesmente passar pelo `get_own_property_slot` (reifica, observável só na ordem
   de `Structure`, aceitável apenas se o golden de ordem passar).
6. `Object.assign` / `copy_data_properties` / `Object.defineProperties` (ObjectConstructor.cpp 393/422/571,
   JSGlobalObjectFunctions.cpp 909/1003): chamar `reify_all_static_properties` onde o upstream chama, já que
   o porte percorre `Structure` direto nesses atalhos.
7. `Structure` de `typeof`/ICs do interpretador (CommonSlowPaths.h:173): o porte não tem JIT/IC
   equivalente? Conferir `put_by_id`/`get_by_id` do LLInt-porte: se leem a `Structure` direto sem passar por
   `get_own_property_slot`, precisam do mesmo teste `has_non_reified_static_properties`.
8. `HAS_STATIC_PROPERTY_TABLE` nos `STRUCTURE_FLAGS` de cada classe migrada (só ligar junto com a tabela).

## 8. Riscos

1. O flag `HAS_STATIC_PROPERTY_TABLE` ligado sem o gancho 1 faz propriedades sumirem: ligar tudo na mesma fatia
   por classe.
2. `convertToDictionary` pode não existir no porte; o dump de bytecode compara só `Structure` de objetos
   com tabela intocada (`Array` no dump nunca chegou ao reify), então um dicionário fiel é opcional na 1a leva.
3. Ordem de reify: `reifyAllStaticProperties` itera a tabela por pai (filho primeiro) e pula nomes já
   presentes: copiar literal.
4. Propriedades sobrescritas por `Object.defineProperty` com atributos diferentes devem ganhar da lut
   (ela só vale se a `Structure` não tem o nome).
5. Dependências nos goldens: `tests/golden/own_keys_bun.json` e `builtin_own_keys_golden.rs` testam a ordem
   `ownKeys`; devem continuar iguais (a seção 3 mostra que a ordem observável é a mesma).

## 9. Fatias de 5 minutos

1. FEITA (sem cargo, ainda não compilada): `src/runtime/lookup.rs` com `HashTable`/`HashTableValue`/`Kind`
   (`NativeFunction`, `BuiltinGenerator`, `Accessor`, `BuiltinAccessor`, `Constant`, `LazyProperty`) + testes de
   busca por nome e de ordem; registrado em `runtime/mod.rs`. Na 1a compilação conferir: `Debug` dos ponteiros
   de função com lifetime, `Intrinsic::NoIntrinsic` e os imports.
   Nota para a 2: há 172 literais `ClassInfo { .. }` (grep `ClassInfo {`), cada um precisa de
   `static_prop_hash_table: None`; fazer em lotes por arquivo, ou trocar por `ClassInfo::new(..)` const.
2. FEITA: campo `static_prop_hash_table` no `ClassInfo` (`None` em todos os literais).
3. FEITA (sem cargo, não compilada): em `lookup.rs`, `reify_static_property` (`NativeFunction` e
   `BuiltinGenerator`; as outras etiquetas devolvem `false` sem tocar o objeto) e
   `get_static_property_slot_from_table(vm, table, this, name, slot)`. Sem chamador (o gancho é a fatia 4).
   Na 1a compilação conferir: `Identifier::from_uid(..).utf8()`, `name.string().string()` e `table` por
   parâmetro (o C++ percorre `classInfo` e pais; esse laço entra no gancho da 4).
4. FEITA (sem cargo, não compilada): gancho 1 em `JSObject::get_own_property_slot`, logo depois da busca na
   `Structure` e antes da `SymbolTable` do global (como `JSGlobalObject::getOwnPropertySlot`);
   `has_non_reified_static_properties(&StructureRef)` e `get_own_static_property_slot` (percorre `class_info` e
   pais) em `js_object.rs`. Custo no caminho comum: só o teste do flag. Sem tabela em nenhum `ClassInfo`, o
   laço não acha nada e o comportamento não muda. Na 1a compilação conferir: `type_info().has_static_property_table()`
   e `static_properties_reified()` em `StructureRef`.
5. FEITA (sem cargo, não compilada): gancho 2 em `own_property_names.rs`: `get_non_reified_static_property_names`
   (privada, percorre `class_info` e pais, ordem da tabela, pula DontEnum no modo Exclude e o nome que a
   `Structure` sombreia como DontEnum; não reifica) chamada em `get_own_non_index_property_names` antes do laço
   da `Structure`. Sem tabela em nenhum `ClassInfo`, retorna cedo. Na 1a compilação conferir:
   `Identifier::from_span`, `structure.get_with_attributes` (tupla offset, attrs), `DontEnumPropertiesMode::Exclude`.
6. FEITA (sem cargo, não compilada): `JSObject::reify_all_static_properties` (converte para dicionário, reifica
   filho primeiro e pais depois, liga o bit) e `reify_before_delete` em `js_object.rs`; gancho 3 em
   `delete_property` logo antes de ler a `Structure` (nome `DontDelete` da tabela devolve false com
   `set_nonconfigurable`; outro nome da tabela reifica tudo). Sem tabela em nenhum `ClassInfo`, nada muda
   (`has_non_reified_static_properties` só é verdadeiro com o flag, e `reify_before_delete` não acha entrada).
   Na 1a compilação conferir: `set_static_properties_reified` por `&self` em `StructureRef` (Rc, setter pode
   exigir `&mut`/Cell), `Structure::is_dictionary`, `DONT_DELETE` importado, `ClassInfo.static_prop_hash_table`
   como `Option<&'static HashTable>` com `.and_then(|t| t.entry(key))`.
7. FEITA (sem cargo, não compilada): ganchos 4/5 em `js_object.rs`: `reify_static_property_named` (privada,
   reifica só o nome pedido via `get_own_static_property_slot`, só com `has_non_reified_static_properties`)
   chamada em `put` antes de `can_perform_fast_put_inline` e em `define_own_property` antes de
   `define_own_non_index_property`. `has_property`/`has_own_property` já passam por `get_own_property_slot`
   (gancho 1), então reificam o nome (aceitável, ver 7.5). Risco da 6 conferido: `set_static_properties_reified`
   é `&self` sobre `Cell<u32>` (`bool_bit_accessors!` em `structure.rs`), nenhum ajuste necessário. Sem tabela em
   nenhum `ClassInfo`, nada muda.
8. FEITA (sem cargo, não compilada): `ArrayConstructor` migrado. `ARRAY_CONSTRUCTOR_S_INFO` aponta para
   `ARRAY_CONSTRUCTOR_TABLE` (`from`, `DONT_ENUM | BUILTIN`, `BuiltinGenerator ArrayConstructorFromCode`, length 1),
   `ArrayConstructor::STRUCTURE_FLAGS` inclui `HAS_STATIC_PROPERTY_TABLE`, e o `from` público eager saiu de `create`
   (ficam `length`, `name`, `prototype`, `@@species`, `of`, `isArray`, `@from`, `fromAsync` como no upstream).
   Teste novo `tests/array_static_props.rs` (ownKeys antes, depois do acesso, depois de `delete Array.from`, e
   descritor, valores medidos no bun). Na 1a compilação conferir: imports `put_direct_builtin_function_without_transition`
   e `builtin_names` ainda usados em `create`, o `static` com referência circular ClassInfo/HashTable, e o dump
   do `e2e_bytecode` `(8/8)`.
   Critério original: dump `(8/8)` e `ownKeys(Array)` do golden com `from` na frente.
9. FEITA (sem cargo, não compilada): gancho 6. `copy_enumerable_own_properties` (`copyDataProperties`/`cloneObject`)
   reifica a origem antes do `copy_via_structure` (a única leitura direta do `PropertyTable` fora do
   `own_property_names`) e `object_assign_generic` reifica origem e alvo, como `ObjectConstructor.cpp`.
   Conferidos sem mudança: `for-in` (`property_name_enumerator` passa por `own_property_names`, gancho 2),
   `get_by_id`/`put_by_id` do LLINT (passam por `get`/`put`, ganchos 1 e 4), `private_field_offset` (nomes
   privados nunca estão na lut) e `static_iterator_method` (lê a `Structure` direto, igual ao `trySpreadFast`).
   Na 1a compilação conferir: `&**target` (ObjectRef para JSObject) e `has_non_reified_static_properties` pública.
10. Migrar `ObjectConstructor` (já usa a lista `fromEntries`/`groupBy` no `finishCreation`; a lut tem os 20
    primeiros), depois Promise, Number, String, Symbol, RegExp, Date (já tem as flags), Error.prototype:
    uma classe por fatia, cada uma só quando o dump/golden de `Structure` dela diverge.
    10a. FEITA (sem cargo, não compilada): `ObjectConstructor` migrado. `OBJECT_CONSTRUCTOR_TABLE` (21 entradas na
    ordem do `@begin`, `DONT_ENUM|FUNCTION`, `fromEntries` como `BuiltinGenerator` `DONT_ENUM|BUILTIN`) no
    `OBJECT_CONSTRUCTOR_S_INFO`, `STRUCTURE_FLAGS` com `HAS_STATIC_PROPERTY_TABLE`, laço eager removido de `create`.
    Eager como no upstream: `length`, `name`, `prototype`, as privadas, `hasOwn` público (agora antes de `@hasOwn`,
    ordem do `finishCreation`), `groupBy`. Teste `tests/object_static_props.rs` (antes, depois de `Object.is/hasOwn/entries`,
    depois de `delete Object.assign`: `length,name,prototype,hasOwn,groupBy,is,entries,` + resto da tabela; medido no bun).
    Na 1a compilação conferir: `static` com `fn` item em `const fn native_entry` (coerção para `NativeFunction`),
    imports não usados (`put_direct_builtin_function_without_transition` ainda serve ao `groupBy`), e o dump de bytecode
    de `Object` se algum golden o cobre. Próxima: Promise.
    10b. FEITA (sem cargo, não compilada): `JSPromiseConstructor` migrado. `PROMISE_CONSTRUCTOR_TABLE` (7 entradas na
    ordem do `@begin`: resolve, reject, race, all, allSettled, any, withResolvers; `DONT_ENUM|FUNCTION`, intrinsics de
    resolve/reject) em `JS_PROMISE_CONSTRUCTOR_S_INFO` (base `JS_FUNCTION_S_INFO`), `create_structure` próprio com
    `HAS_STATIC_PROPERTY_TABLE`, laço eager removido de `create`. Eager como no upstream: `prototype`, `try`, `@@species`,
    `isPromise` (opção), `@resolve`, `@reject`. `init_promise` reifica o `resolve` (entrada 0) antes de ler
    `resolve_function`, o que reproduz o bun (`resolve` já está na `Structure` depois de `try`). Teste
    `tests/promise_static_props.rs` (antes, depois de `all`/`race`, depois de `delete Promise.all`, descritor).
    Na 1a compilação conferir: `reify_static_property(vm, &constructor, ..)` (deref de `JSFunctionRef` para `JSObject`),
    `host_function!` itens usáveis em `static` const, e imports não usados. Próxima: Number.
    10c. FEITA (sem cargo, não compilada): `NumberConstructor` migrado. `NUMBER_CONSTRUCTOR_TABLE` (isFinite, isNaN,
    isSafeInteger; `DONT_ENUM|FUNCTION`, comprimento 1, intrinsics) em `NUMBER_CONSTRUCTOR_S_INFO` (base
    `JS_FUNCTION_S_INFO`), `create_structure` próprio com `HAS_STATIC_PROPERTY_TABLE`; `JSFunction::create_native_with_structure`
    novo (o `create_native` o chama com a `hostFunctionStructure`). Eager como no upstream: `prototype`, as 7 constantes e
    `NaN`, `parseInt`, `parseFloat`, `isInteger` (confirmado: as constantes são `putDirectWithoutTransition` no
    `finishCreation`). Medido no bun: antes e depois de acessar `length,name,isFinite,isNaN,isSafeInteger,prototype,...,isInteger`;
    `delete Number.isInteger` não reifica; `delete Number.isFinite` dá `length,name,prototype,...,parseFloat,isInteger,isNaN,
    isSafeInteger`. Teste `tests/number_static_props.rs` (4 ordens + descritor). Na 1a compilação conferir:
    `global_object.function_prototype()` já existe quando `NumberConstructor::create` roda, e imports não usados
    (`put_direct_native_function_without_transition` ainda serve ao `isInteger`). Próxima: String.
    10d. FEITA (sem cargo, não compilada): `BigIntConstructor` migrado. `BIG_INT_CONSTRUCTOR_TABLE` (`asUintN`, `asIntN`,
    `DontEnum|Function`, comprimento 2) no `BIG_INT_CONSTRUCTOR_S_INFO`, `STRUCTURE_FLAGS` com `HAS_STATIC_PROPERTY_TABLE`
    usado no `Structure::create`, laço eager removido de `finish_creation` (ficam `length`, `name`, `prototype` e, com
    `useBigIntMathMethods`, os sete de Math depois, como no upstream). Medido no bun: antes e depois de acessar
    `asUintN,asIntN,length,name,prototype`; `delete BigInt.asUintN` sem acesso dá `length,name,prototype,asIntN`;
    acessando os dois e `delete BigInt.asIntN` dá `length,name,prototype,asUintN`. Teste `tests/bigint_static_props.rs`.
    Na 1a compilação conferir: `big_int_native_entry` const com os `host_function!` (itens usáveis em `static`),
    `for (name, length, function) in math_methods` (refs de tupla) e o import `FUNCTION`.

## 10. Classes do upstream com `@begin ...Table` (grep em `upstream/JavaScriptCore`) e estado no porte

Migradas (têm `static_prop_hash_table: Some`): ArrayConstructor, ObjectConstructor, JSPromiseConstructor,
NumberConstructor, StringConstructor, SymbolConstructor, BigIntConstructor, DateConstructor, JSONObject, ReflectObject,
IntlObject, TemporalObject, TemporalNow. RegExp está com outro agente; não tocar.

Ainda eager no porte (instalam as entradas da lut na criação), por grupo:

- Objetos: `JSGlobalObject` (já tem o flag, tabela própria). `JSWebAssembly` migrado (ver 10m ao fim).
- Construtores: `RegExpConstructor` (outro agente), `IntlCollator`, `IntlDateTimeFormat`, `IntlDisplayNames`,
  `IntlDurationFormat`, `IntlListFormat`, `IntlNumberFormat`, `IntlPluralRules`, `IntlRelativeTimeFormat`, `IntlSegmenter`
  (todos `*Constructor`), `Temporal{Duration,Instant,PlainDate,PlainDateTime,PlainMonthDay,PlainTime,PlainYearMonth,
  ZonedDateTime}Constructor`. `WebAssemblyModuleConstructor` migrado (10m).
- Protótipos: `NumberPrototype`, `BooleanPrototype`, `BigIntPrototype`, `SymbolPrototype`, `StringPrototype`,
  `DatePrototype`, `ErrorPrototype`, `JSPromisePrototype`, `GeneratorPrototype`, `AsyncGeneratorPrototype`,
  `JSIteratorHelperPrototype`, `JSDataViewPrototype`, `ShadowRealmPrototype`, `IntlSegmentsPrototype`,
  `IntlSegmentIteratorPrototype`, `IntlLocalePrototype`, `Intl{Collator,DateTimeFormat,DisplayNames,DurationFormat,
  ListFormat,NumberFormat,PluralRules,RelativeTimeFormat,Segmenter}Prototype`, `Temporal*Prototype`,
  `WebAssembly{Global,Table,Memory,Exception}Prototype`.
- Não são classe JS (fora de escopo): `parser/Keywords.table`. `ErrorConstructor` NÃO tem `@begin` (nada a migrar).

Próxima da fila (fora RegExp/Date): `JSONObject`, depois `ReflectObject`, depois os protótipos (`ErrorPrototype`,
`NumberPrototype`...), só quando um golden de `Structure` divergir.
    10d. FEITA (sem cargo, não compilada): `StringConstructor` migrado (`stringConstructorTable`: `fromCharCode`,
    `fromCodePoint`, `raw`; teste `tests/string_static_props.rs`: antes e depois do acesso
    `length,name,fromCharCode,fromCodePoint,raw,prototype`, `delete String.raw` reifica tudo e `prototype` passa à frente).
    10e. FEITA (sem cargo, não compilada): `SymbolConstructor` migrado (`symbolConstructorTable`: `for`, `keyFor`; teste
    `tests/symbol_static_props.rs`: nomes da tabela na frente de `length,name,prototype` e dos símbolos conhecidos;
    `delete Symbol.keyFor` dá os eager e depois `for`; símbolo conhecido não é apagável).
    10f. FEITA (sem cargo, não compilada): `DateConstructor` migrado. `DATE_CONSTRUCTOR_TABLE` (parse, UTC, now; `DONT_ENUM|FUNCTION`,
    comprimentos 1/7/0, `now` com `DateNowIntrinsic`) mora em `date_constructor_natives.rs` (referencia as cascas nativas) e é
    apontada por `DATE_CONSTRUCTOR_S_INFO`; o `STRUCTURE_FLAGS` já tinha `HAS_STATIC_PROPERTY_TABLE`. `DateConstructorEntry`,
    `DateConstructorFunction` e o laço eager de `create_date_constructor` saíram (ficam `length`, `name`, `prototype`).
    Teste `tests/date_static_props.rs`, medido no bun: antes e depois de acessar `parse,UTC,now,length,name,prototype`;
    `delete Date.parse` sem acesso dá `length,name,prototype,UTC,now`; `Date.now` acessado e `delete Date.UTC` dá
    `length,name,prototype,now,parse`; descritor `function,true,false,true,7,1,0,now`. Na 1a compilação conferir:
    ciclo de `static` entre `date_constructor.rs` (S_INFO) e `date_constructor_natives.rs` (tabela), imports não usados
    em ambos e o parâmetro `_global_object` de `create_date_constructor`. Próxima: RegExp (outro agente), Error.prototype.

    10g. FEITA (sem cargo, não compilada): `JSONObject` migrado (`json_object_native.rs`). `JSON_TABLE` (`parse` 2, `stringify` 3,
    `DONT_ENUM|FUNCTION`) em `JSON_OBJECT_S_INFO`, `HAS_STATIC_PROPERTY_TABLE` na `Structure` do `create_json_object`; ficam eager
    como no `finishCreation`: `@@toStringTag`, `isRawJSON`, `rawJSON`. Medido no bun: antes e depois de acessar
    `parse,stringify,isRawJSON,rawJSON,Symbol(Symbol.toStringTag)`; `delete JSON.parse` sem acesso dá `isRawJSON,rawJSON,stringify,@@toStringTag`;
    `stringify` acessado e `delete JSON.stringify` dá `isRawJSON,rawJSON,parse,@@toStringTag`; `delete JSON.rawJSON` (fora da tabela) não reifica.
    Teste `tests/json_static_props.rs`. Na 1a compilação conferir: imports não usados em `json_object_native.rs` (`Identifier` e
    `put_direct_native_function_without_transition` ainda servem a `isRawJSON`/`rawJSON`), `json_proto_func_*` do macro como itens `static`.
    10h. FEITA (sem cargo, não compilada): `ReflectObject` migrado (`reflect_object.rs`). `REFLECT_OBJECT_TABLE` (13 entradas na ordem do
    `@begin`; `apply`, `deleteProperty`, `get`, `has` como `BuiltinGenerator` `DONT_ENUM|BUILTIN`, as demais `DONT_ENUM|FUNCTION` com
    os intrínsecos `getPrototypeOf`/`ownKeys`), `STRUCTURE_FLAGS` com `HAS_STATIC_PROPERTY_TABLE`, `finish_creation` só com o
    `@@toStringTag`. O teste unitário do módulo reifica cada nome antes de ler. Teste `tests/reflect_static_props.rs` (antes, depois de
    acessar `has,get`, e `delete Reflect.apply` dando `has,get,construct,...`; medido no bun). Na 1a compilação conferir: imports
    removidos (`put_native_function`, `put_direct_builtin_function_without_transition`) sem uso residual e `builtin_names` ausente.
    Obs.: a edição de `json_object_native.rs` e `reflect_object.rs` foi feita via script Python (transformação de vários trechos).
    Próxima: `ErrorPrototype`, `NumberPrototype` (só quando um golden de `Structure` divergir), `IntlObject`/`TemporalObject`.
    10i. FEITA (sem cargo, não compilada): `BooleanPrototype` (`booleanPrototypeTable`: `toString`, `valueOf`, `DONT_ENUM|FUNCTION`,
    comprimento 0) e `SymbolPrototype` (`symbolPrototypeTable`: `description` como `CustomAccessor` `DONT_ENUM|READ_ONLY|CUSTOM_ACCESSOR`
    sem setter, `toString` com `SymbolPrototypeToStringIntrinsic`, `valueOf`) migrados. As tabelas moram nos próprios módulos e são
    apontadas pelo `S_INFO`; `STRUCTURE_FLAGS` ganhou `HAS_STATIC_PROPERTY_TABLE` (o Boolean passou a usar `Structure::create` direto,
    já que `JSWrapperObject::create_structure` tem flags fixas). `finish_creation` do Boolean ficou só com o valor interno; o do Symbol
    com `set_may_be_prototype`; `constructor`, `[Symbol.toPrimitive]` e `[Symbol.toStringTag]` seguem eager. Teste
    `tests/boolean_symbol_proto_static_props.rs`, medido no bun: antes e depois de acessar `toString,valueOf,constructor` e
    `description,toString,valueOf,constructor,Symbol(Symbol.toPrimitive),Symbol(Symbol.toStringTag)`; `delete Boolean.prototype.valueOf`
    dá `constructor,toString`; `delete Symbol.prototype.description` dá `constructor,toString,valueOf,...`; `delete Symbol.prototype.valueOf`
    dá `constructor,description,toString,...`; `delete ...constructor` não reifica. Na 1a compilação conferir: imports não usados nos dois
    módulos (`ImplementationVisibility` segue em uso no Symbol; no Boolean saiu) e `HashTableValue` com `getter` de `custom_getter!`
    em `static`.

    10i. FEITA (sem cargo, não compilada): `IntlObject` migrado (`intl_object.rs`). `INTL_OBJECT_TABLE_VALUES` (12 entradas na ordem do
    `@begin`: `getCanonicalLocales` e `supportedValuesOf` `DontEnum|Function` comprimento 1, e os dez construtores
    `DontEnum|PropertyCallback`) em `INTL_OBJECT_S_INFO`; `install_intl` só cria a `Structure` com `HAS_STATIC_PROPERTY_TABLE`,
    o `@@toStringTag` e o `Intl` no global. `lookup.rs::reify_static_property` passou a reificar `Kind::LazyProperty` (chama o callback
    e faz `put_direct` com os atributos da tabela). Cada callback (`lazy_constructor!`) chama o `install_*` da classe, que grava o
    construtor em `Intl` e cria o protótipo, e devolve o valor lido de volta (a regravação é substituição no lugar). Os construtores
    das classes `Intl` agora nascem no primeiro acesso, como o C++. Teste `tests/intl_static_props.rs`, medido no bun: antes e depois
    de acessar `getCanonicalLocales,supportedValuesOf,Collator,...,Segmenter,Symbol(Symbol.toStringTag)`; `delete Intl.getCanonicalLocales`
    reifica tudo; acessando `Segmenter` e `supportedValuesOf` e `delete Intl.Locale` dá `Segmenter,supportedValuesOf,getCanonicalLocales,
    Collator,...`; `delete Intl.Collator` (também da tabela) dá `getCanonicalLocales,supportedValuesOf,DateTimeFormat,...`. Na 1a
    compilação conferir: `intl.structure().realm()` devolvendo `JSGlobalObjectRef` que deref para `JSGlobalObject` no `install_*`,
    o macro `lazy_constructor!` com `get_direct_by_name`, `host_function!` como `fn` item coagível a `NativeFunction` em `static`, e
    se algum código assumia `Intl.X` já presente (ex.: testes que leem a `Structure` de `Intl` direto). Próxima: `TemporalObject`,
    protótipos só quando um golden de `Structure` divergir.
    10j. FEITA (sem cargo, não compilada): `TemporalObject` e `TemporalNow` migrados. `TEMPORAL_OBJECT_TABLE_VALUES` (9 `LazyProperty`
    `DONT_ENUM|PROPERTY_CALLBACK` na ordem do `@begin`: Duration, Instant, Now, PlainDate, PlainDateTime, PlainTime, PlainMonthDay,
    PlainYearMonth, ZonedDateTime; macro `lazy_temporal_class!` chama o `install_*` de cada classe) e `TEMPORAL_NOW_TABLE_VALUES`
    (6 funções `DONT_ENUM|FUNCTION` comprimento 0) nos `S_INFO`, `STRUCTURE_FLAGS` com `HAS_STATIC_PROPERTY_TABLE`; `install_temporal` só
    cria o objeto (`@@toStringTag`) e `TemporalNow::create` só o tag. Como o C++ tem `LazyClassStructure`, os oito acessores
    `*_structure()` do global (`plain_date_structure()` etc.) passaram a `lazy_temporal_structure` (`temporal_object.rs`), que reifica a
    entrada da tabela se a estrutura ainda não existe (`TemporalGlobalData.temporal` guarda o objeto): sem isso `Temporal.Now.plainDateISO()`
    sem acessar `Temporal.PlainDate` dava panic. Teste `tests/temporal_static_props.rs`, medido no bun: antes e depois de acessar
    `Duration,...,ZonedDateTime,Symbol(Symbol.toStringTag)`; acessando `Now,Duration,PlainTime` e `delete Temporal.PlainDate` dá
    `Now,Duration,PlainTime,Instant,PlainDateTime,PlainMonthDay,PlainYearMonth,ZonedDateTime,...`; `Now`: `instant,timeZoneId,...,zonedDateTimeISO`
    igual antes/depois, `delete Temporal.Now.timeZoneId` tira só ele. Na 1a compilação conferir: `lazy_temporal_class!` com `$install:path`
    e `&global_object.object_prototype()` (temporário), closures `|data| ...` coagindo para `fn` ponteiro em `lazy_temporal_structure`,
    imports não usados em `temporal_now.rs` (`Identifier`/`PropertyName` ainda servem ao `install_temporal_now`) e `temporal_object.rs`.
    Próxima: protótipos só quando um golden de `Structure` divergir.

    10j. FEITA (sem cargo, não compilada): `NumberPrototype` e `BigIntPrototype` migrados. `numberPrototypeTable` (`toLocaleString` 0,
    `valueOf` 0, `toFixed` 1, `toExponential` 1, `toPrecision` 1, todas `DONT_ENUM|FUNCTION`) e `bigIntPrototypeTable` (`toString`,
    `toLocaleString`, `valueOf`, comprimento 0) moram nos próprios módulos e são apontadas pelo `S_INFO`; `STRUCTURE_FLAGS` ganhou
    `HAS_STATIC_PROPERTY_TABLE` (Number passou a usar `Structure::create` direto). Seguem eager como no C++: `toString` do Number
    (`numberProtoToStringFunction`, com o intrínseco), `constructor`, e o `@@toStringTag` do BigInt. Teste
    `tests/number_bigint_proto_static_props.rs`, medido no bun: antes e depois de acessar
    `toLocaleString,valueOf,toFixed,toExponential,toPrecision,toString,constructor` e `toString,toLocaleString,valueOf,constructor,
    Symbol(Symbol.toStringTag)`; `delete Number.prototype.toFixed` dá `toString,constructor,toLocaleString,valueOf,toExponential,
    toPrecision`; `delete Number.prototype.toString` (fora da tabela) não reifica; `delete BigInt.prototype.valueOf` dá `constructor,
    toString,toLocaleString,@@toStringTag`. Na 1a compilação conferir: imports não usados (`NativeFunction` ainda serve às entradas) e
    se algum código lia `toFixed` etc. do Number.prototype pela `Structure` direto.

    10k. FEITA (sem cargo, não compilada): `StringPrototype` migrado. `stringPrototypeTable` (os 13 métodos HTML do Annex B na ordem do
    `@begin`: `anchor` 1, `big` 0, `bold` 0, `blink` 0, `fixed` 0, `fontcolor` 1, `fontsize` 1, `italics` 0, `link` 1, `small` 0, `strike` 0,
    `sub` 0, `sup` 0; todas `DONT_ENUM|FUNCTION`) mora em `string_prototype.rs` e é apontada por `STRING_PROTOTYPE_S_INFO`;
    `StringPrototype::STRUCTURE_FLAGS` (novo) soma `HAS_STATIC_PROPERTY_TABLE` e o `create_structure` o usa. Os 13 `define(..)` eager
    saíram de `add_string_prototype_properties`; seguem eager como no C++ tudo o que o `finishCreation` põe (`toString`, `valueOf`,
    `charAt`..., `trimStart`/`trimEnd` e aliases, `isWellFormed`, `toWellFormed`), mais `constructor` e `[Symbol.iterator]` (depois, em
    `StringConstructor::create`). Medido no bun: antes e depois de acessar `length,anchor,...,sup,toString,...,toWellFormed,constructor,
    Symbol(Symbol.iterator)`; `delete String.prototype.big` dá o resto da `Structure` e depois a tabela sem `big`; `link` acessado e
    `delete ...sub` dá `...constructor,link,anchor,big,...,sup` (o acessado entra na `Structure` primeiro); `delete ...trim` (fora da tabela) não reifica. Teste `tests/string_proto_static_props.rs`. Na 1a compilação conferir:
    ciclo de módulos `string_prototype` <-> `string_prototype_natives_part2` (só `static`s de `fn`), imports não usados em
    `string_prototype_natives.rs` (`part2` segue em uso nos não HTML), e se o reify de `Symbol.iterator`/`constructor` na posição certa
    depende de `StringConstructor::create` ter rodado antes de qualquer acesso (o teste de `delete` assume que sim). Próxima: `DatePrototype`.
    10j. FEITA (sem cargo, não compilada): `ErrorPrototype` (`errorPrototypeTable`: `toString`, `DONT_ENUM|FUNCTION`, comprimento 0) e
    `JSPromisePrototype` (`promisePrototypeTable`: `finally`, `DONT_ENUM|FUNCTION`, comprimento 1, `promise_proto_func_finally_host`) migrados.
    Em `error_natives.rs`: `ERROR_PROTOTYPE_S_INFO` segue sendo o `ErrorPrototypeBase` (native, Aggregate, Suppressed; sem tabela) e o novo
    `ERROR_PROTOTYPE_WITH_TABLE_S_INFO` (pai o base, `&ERROR_PROTOTYPE_TABLE`) é o do `Error.prototype`, com `error_prototype_structure`
    somando `HAS_STATIC_PROPERTY_TABLE`; o `install_first` do `toString` saiu (a closure ficou `|_| {}`). No `NativeErrorPrototype`
    não há `toString` próprio (medido: `delete TypeError.prototype.toString` não muda nada). Em `promise_prototype.rs` o `finally` eager saiu
    do `finish_creation` (ficam `then`, `catch`, `@@toStringTag`, `@then` privado) e `STRUCTURE_FLAGS` soma o flag. Medido no bun: antes e
    depois de acessar `toString,name,message,constructor` e `finally,then,catch,constructor,Symbol(Symbol.toStringTag)`; `delete Error.prototype.toString`
    dá `name,message,constructor`; `delete Promise.prototype.finally` dá `then,catch,constructor,Symbol(Symbol.toStringTag)`; `finally` acessado e
    `delete ...catch` (fora da tabela) dá `finally,then,constructor,Symbol(Symbol.toStringTag)`; descritores `function,true,false,true,0,toString`
    e `function,true,false,true,1,finally`. Teste `tests/error_promise_proto_static_props.rs`. Na 1a compilação conferir: imports não usados em
    `error_natives.rs` (`put_direct_native_function_without_transition`, `ImplementationVisibility` ainda servem ao `isError`), o `static` de
    `ERROR_PROTOTYPE_TABLE` apontando para `error_proto_func_to_string` (um `fn` comum), e `Intrinsic`/`ImplementationVisibility` em `promise_prototype.rs`.
    Obs.: a edição de `promise_prototype.rs` foi feita via script Python (três trechos do mesmo arquivo); o resto, com Edit/Write.

    10k2. FEITA (sem cargo, não compilada): `DatePrototype` migrado. `DATE_PROTOTYPE_TABLE` (44 entradas na ordem do `@begin`,
    `DONT_ENUM|FUNCTION`, comprimentos 0/1/2/3/4, intrínsecos `DatePrototype*Intrinsic` em `valueOf`, `getTime` e os getters) mora em
    `date_prototype_natives.rs` (referencia as cascas) e é apontada por `DATE_PROTOTYPE_S_INFO` (`date_prototype.rs`); o
    `STRUCTURE_FLAGS` já tinha `HAS_STATIC_PROPERTY_TABLE`. Saíram `DatePrototypeFunction`, `DatePrototypeEntry`, `native_function_for` e o
    laço eager de `create_date_prototype`; ficam eager como no `finishCreation`: `toUTCString`, `toGMTString`, `toTemporalInstant`
    (com `useTemporal`), depois `constructor` e `[Symbol.toPrimitive]` (instalados por quem cria o `Date`). Medido no bun: antes e depois de
    acessar a tabela e depois `toUTCString,toGMTString,toTemporalInstant,constructor,Symbol(Symbol.toPrimitive)`; `delete Date.prototype.toString`
    com `getTime,setYear,toJSON` acessados dá `toUTCString,...,constructor,getTime,setYear,toJSON,` + resto da tabela; `setSeconds` e `valueOf`
    acessados e `delete getYear` dá `...constructor,setSeconds,valueOf,toString,...`; `delete Date.prototype.toUTCString` (fora da tabela) não
    reifica; descritores `function,true,false,true,<length>,<name>`. Teste `tests/date_proto_static_props.rs`. Na 1a compilação conferir:
    ciclo de módulos `date_prototype` <-> `date_prototype_natives` (só `static`s de `fn`), `host_function!` itens coagíveis a `NativeFunction`
    em `static`, imports não usados em ambos, e se algum código lia `DATE_PROTOTYPE_TABLE` como array.

    10k. FEITA (sem cargo, não compilada): `GeneratorPrototype`, `AsyncGeneratorPrototype` e `JSIteratorHelperPrototype` migrados.
    `generatorPrototypeTable` (`next`, `return`, `throw`, `BuiltinGenerator` `DONT_ENUM|BUILTIN` comprimento 1),
    `asyncGeneratorPrototypeTable` (`return`, `throw`, `NativeFunction` `DONT_ENUM|FUNCTION` comprimento 1) e
    `jsIteratorHelperPrototypeTable` (`next`, `return`, `BuiltinGenerator` comprimento 0) nos `S_INFO`, `STRUCTURE_FLAGS` com
    `HAS_STATIC_PROPERTY_TABLE`, entradas eager removidas de `create`. Eager como no upstream: Generator e IteratorHelper só o
    `@@toStringTag`; AsyncGenerator `next` (o `asyncGeneratorPrototypeNextFunction`) e depois o `@@toStringTag`. O `@@toStringTag` de
    Generator/AsyncGenerator saiu de `link_generator_prototype` (`function_kind_intrinsics.rs`, que perdeu o parâmetro `to_string_tag`) para o
    `create`, na ordem do `finishCreation`; o `constructor` segue sendo instalado no link. Medido no bun (cada caso em processo novo, os
    protótipos são compartilhados): antes e depois de acessar `next,return,throw,constructor,@@toStringTag` (Generator),
    `return,throw,next,constructor,@@toStringTag` (AsyncGenerator), `next,return,@@toStringTag` (IteratorHelper); `delete G.next` dá
    `constructor,return,throw`; `delete A.throw` dá `next,constructor,return`; `delete A.next` e `delete G.constructor` não reificam;
    `delete I.next` dá `return`. Teste `tests/generator_proto_static_props.rs`. Na 1a compilação conferir: `async_generator_prototype_*_host`
    coagíveis a `NativeFunction` em `static`, imports não usados (`ImplementationVisibility`, `PropertyName`, `Intrinsic` ainda servem a
    `async_generator_prototype.rs`; `Identifier` e `put_direct_builtin_function_without_transition` saíram dos outros dois), e `[1].values().map`
    no teste. Nota de processo: o cabeçalho de `iterator_helper_prototype.rs` foi reescrito por um script Python por engano (regra de Edit/Write).

    10l. FEITA (sem cargo, não compilada): `JSDataViewPrototype` e `ShadowRealmPrototype` migrados. `dataViewTable` em
    `data_view_prototype.rs`: as 22 funções `getInt8`..`setBigUint64` (`DONT_ENUM|FUNCTION`, comprimento 1 nos get e 2 nos set, com os
    intrínsecos) mais `buffer` e `byteOffset` como `Kind::CustomAccessor` (`DONT_ENUM|READ_ONLY|CUSTOM_ACCESSOR`, sem setter; os corpos
    viraram `custom_getter!` com `this_value`). A tabela é montada por uma `const` que percorre `DATA_VIEW_FUNCTIONS` (evita reescrever as
    22 linhas). Eager como no C++: só o `byteLength` (`JSC_NATIVE_INTRINSIC_GETTER_WITHOUT_TRANSITION`) e o `@@toStringTag`, que o
    `js_array_buffer.rs` ainda instala depois do `constructor` (`put_to_string_tag_tail`). `shadowRealmPrototypeTable` em
    `shadow_realm_prototype.rs`: `evaluate` (comprimento 1) e `importValue` (2), `Kind::BuiltinGenerator` `DONT_ENUM|BUILTIN`; eager só o
    `@@toStringTag`. Medido no bun (cada caso em processo novo): antes e depois de acessar, DataView
    `getInt8..setBigUint64,buffer,byteOffset,byteLength,constructor,@@toStringTag`, ShadowRealm `evaluate,importValue,constructor,@@toStringTag`;
    ler `buffer`/`byteOffset` por `getOwnPropertyDescriptor` não reifica (`get buffer` e `get byteOffset`, comprimento 0, `set` undefined,
    não enumerável, configurável); `delete DV.getInt8` dá `byteLength,constructor,getUint8..setBigUint64,buffer,byteOffset`; `delete
    DV.byteLength` e `delete DV.constructor` não reificam; `delete SR.evaluate` dá `constructor,importValue`; `delete SR.constructor` não
    reifica; `DV.prototype.buffer = 1` lança `Attempted to assign to readonly property.` (modo estrito do bun). Teste
    `tests/dataview_shadowrealm_proto_static_props.rs`. Na 1a compilação conferir: o `const` com `while` e cópia de `HashTableValue`
    (precisa de `Copy` em `Kind`, `Intrinsic` e `NativeFunction`), `GetValueFunc` vindo de `custom_getter!` coagível em `const fn`, e se a
    última asserção de `ShadowRealm` (`evaluate.length`) bate com o `length` do builtin.

    10m. FEITA (sem cargo, não compilada): `IntlCollatorPrototype`, `IntlNumberFormatPrototype` e `IntlDateTimeFormatPrototype` migrados.
    `collatorPrototypeTable` (`compare` `DontEnum|ReadOnly|CustomAccessor`, `resolvedOptions` 0), `numberFormatPrototypeTable` e
    `dateTimeFormatPrototypeTable` (`format` `CustomAccessor`, `formatRange` 2, `formatRangeToParts` 2, `formatToParts` 1,
    `resolvedOptions` 0) moram nos próprios `intl_*.rs`, com `ClassInfo` `"Intl.Collator"`/`"Intl.NumberFormat"`/`"Intl.DateTimeFormat"`
    (pai `JS_NON_FINAL_OBJECT_S_INFO`). Em `intl_support.rs`: `IntlClass::install_with_table` (protótipo com `ClassInfo` próprio e
    `HAS_STATIC_PROPERTY_TABLE`; `install_with_statics` virou a mesma rotina com `None`) e os construtores const `intl_function_entry`,
    `intl_custom_getter_entry` e `intl_format_prototype_values` (as cinco entradas comuns a NumberFormat e DateTimeFormat). Os getters
    deixaram de ser `NativeFunction` (`put_getter_on`) e viraram `custom_getter!` (`this` e `PropertyName`). Eager como no C++: o
    `@@toStringTag` e o `constructor` (ordem `constructor`, `@@toStringTag` no bun). Medido no bun: antes e depois de acessar
    `compare,resolvedOptions,constructor,@@toStringTag` e `format,formatRange,formatRangeToParts,formatToParts,resolvedOptions,constructor,
    @@toStringTag`; descritor de `compare`/`format` é accessor (`get compare`/`get format`, comprimento 0, `set` undefined, não enumerável,
    configurável); `delete P.compare` dá `constructor,resolvedOptions,@@tag`; com `resolvedOptions` e `formatRange` acessados,
    `delete P.formatRangeToParts` dá `constructor,resolvedOptions,formatRange,format,formatToParts`; `delete P.constructor` não reifica.
    Teste `tests/intl_proto_static_props.rs`. Na 1a compilação conferir: `const fn` com ponteiros de função (`GetValueFunc` de
    `custom_getter!`, `NativeFunction` de `host_function!`) em `static`, imports não usados (`put_getter_on` segue em uso em Locale e outros),
    `HostCall` ainda usado nos três módulos, e se algo lia `Intl.*.prototype.format` pela `Structure`. Obs.: um `sed -i` pequeno (imports
    de `intl_support.rs`) escapou da regra Edit/Write.
    10m-2. FEITA (sem cargo, não compilada): `IntlPluralRulesPrototype`, `IntlRelativeTimeFormatPrototype`, `IntlListFormatPrototype` e
    `IntlDisplayNamesPrototype` migrados via `install_with_table`. Macro nova `intl_prototype_s_info!` (`intl_support.rs`, `#[macro_export]`)
    gera o `ClassInfo` (`"Intl.X"`, pai `JS_NON_FINAL_OBJECT_S_INFO`, `HashTable` com `class_for_this: None`) a partir das entradas
    `intl_function_entry`, em vez de três `static` repetidos por classe (DRY). Tabelas na ordem do `@begin`: PluralRules `select` 1,
    `selectRange` 2, `resolvedOptions` 0; RelativeTimeFormat `format` 2, `formatToParts` 2, `resolvedOptions` 0; ListFormat `format` 1,
    `formatToParts` 1, `resolvedOptions` 0; DisplayNames `of` 1, `resolvedOptions` 0. Os `put_method_on` dos quatro instaladores saíram
    (imports ajustados). Medido no bun: ordem antes e depois do acesso é a da tabela + `constructor` + `@@toStringTag`; descritor de
    `resolvedOptions` `function,true,false,true,0`; com `resolvedOptions` acessado, `delete P.resolvedOptions` dá `constructor`, o resto
    da tabela, `@@toStringTag`. Testes em `tests/intl_proto_static_props.rs` (`method_class_checks`; o caso `delete P.constructor` não
    reificar foi copiado do Collator, não medido nessas quatro). Na 1a compilação conferir: `&[...]` de `const fn` dentro do `static` da macro
    (promoção a `'static`), imports `intl_function_entry` e `put_method_on` não usado. PENDENTES: Locale (22 entradas, 10 métodos + 12
    getters `CustomAccessor`; hoje `put_getter_on`), Segmenter (`segment`, `resolvedOptions`), SegmentIterator (`next`), Segments
    (`containing`), DurationFormat (`format`, `formatToParts`, `resolvedOptions`). Obs.: o teste foi anexado com `cat >>` (Bash), escapou
    da regra Edit/Write.
    10m-3. FEITA (sem cargo, não compilada): `IntlLocalePrototype` (22 entradas: 10 `intl_function_entry` com length 0 e 12
    `intl_custom_getter_entry`, na ordem de instalação anterior, que bate com o bun) e `IntlDurationFormatPrototype` (`format` 1,
    `formatToParts` 1, `resolvedOptions` 0) migrados via `intl_prototype_s_info!` + `install_with_table`. Em `intl_locale.rs` os 12
    getters trocaram `host_function!` por `custom_getter!`: `optional_text` e os `*_body` agora recebem `(global, this_value, &PropertyName)`
    no lugar de `&HostCall`. Medido no bun: ordem antes e depois do acesso é a tabela + `constructor` + `@@toStringTag`; `baseName` é accessor
    (`get baseName`, length 0, sem `set`, não enumerável, configurável); métodos `function,true,false,true,0`; com `maximize` e `getWeekInfo`
    reificados, `delete P.baseName` dá `constructor,maximize,getWeekInfo`, o resto da tabela, `@@toStringTag`; `delete P.resolvedOptions` do
    DurationFormat dá `constructor,format,formatToParts,@@toStringTag`. Testes `locale_prototype` e `duration_format_prototype` em
    `tests/intl_proto_static_props.rs`. Na 1a compilação conferir: `HostCall` ainda usado em `intl_locale.rs` (sim, nos métodos), `put_getter_on`
    e `put_method_on` sem uso residual nos dois arquivos. NÃO migrados: Segmenter, Segments e SegmentIterator, porque `segment` (Segmenter) e
    `[Symbol.iterator]` (Segments) são `function_with_field` (função nativa com campo interno apontando para o protótipo seguinte), que a
    entrada `intl_function_entry` não expressa; `Segments.containing`/`SegmentIterator.next` sozinhos caberiam, mas o `@@iterator` e o
    `@@toStringTag` do iterador exigem a reificação de símbolo; medido no bun: Segmenter `segment,resolvedOptions,constructor,@@tag`,
    Segments `containing,@@iterator`, SegmentIterator `next,@@tag`. Obs.: os edits em `intl_locale.rs` e `intl_duration_format.rs` e o anexo
    do teste saíram por script/`cat >>` (Bash), escaparam da regra Edit/Write.

    10m-4. FEITA (sem cargo, não compilada): Segmenter, `%Segments%` e `%SegmentIteratorPrototype%` migrados. O campo interno de
    `segment` e `[Symbol.iterator]` era artefato do porte: no upstream (`IntlSegmenter.cpp:141`, `IntlSegments.cpp:102`) as
    funções leem `globalObject->segmentsStructure()` e `segmentIteratorStructure()` (os `LazyProperty` de `JSGlobalObject.cpp:1690-1705`,
    cujos protótipos nascem com `IntlSegmentsPrototype::create`/`IntlSegmentIteratorPrototype::create`); as três tabelas são `@begin`
    comuns: Segmenter `segment` 1 e `resolvedOptions` 0, Segments `containing` 1, iterador `next` 0, todas `DontEnum|Function`. Agora
    `JSGlobalObject` tem `segments_structure` e `segment_iterator_structure` (estruturas das instâncias, preenchidas por
    `install_segmenter`, acessores em `intl_segmenter.rs`), `function_with_field` e o uso de `JSFunctionWithFields`/`FunctionField` saíram do
    arquivo, e as três classes usam `intl_prototype_s_info!` (`SEGMENTER_`, `SEGMENTS_`, `SEGMENT_ITERATOR_PROTOTYPE_S_INFO`) com
    `HAS_STATIC_PROPERTY_TABLE` (Segmenter por `install_with_table`; os outros dois pelo helper local `table_prototype`).
    `@@iterator` de Segments e `@@toStringTag` do iterador ficam eager, como no `finishCreation`. Medido no bun: Segmenter
    `segment,resolvedOptions,constructor,@@toStringTag`, Segments `containing,@@iterator` (sem `constructor`), iterador `next,@@toStringTag`;
    métodos `function,true,false,true`, `segment`/`containing` length 1, `[Symbol.iterator]`/`next` length 0; o protótipo do iterador herda de
    `%IteratorPrototype%`; `delete` de `containing`/`next` deixa o resto. Teste `segmenter_prototypes` em `tests/intl_proto_static_props.rs`
    (escrito com Edit). Na 1a compilação conferir: `Cell` ainda usado (SegmentIteratorState), imports novos em `intl_segmenter.rs`,
    `impl JSGlobalObject` fora do módulo do tipo (campos `pub(crate)`), e se o `JSFunction::create_native` do `@@iterator` aceita
    `&WtfString::from_utf8(b"...")` como o código anterior (usava `name.as_bytes()`).

    10n. FEITA (sem cargo, não compilada): `TemporalDurationPrototype`, `TemporalInstantPrototype` e `TemporalInstantConstructor`
    migrados. `DURATION_PROTOTYPE_TABLE_VALUES` (23 entradas: 11 `DONT_ENUM|FUNCTION` e 12 `CustomAccessor`
    `DONT_ENUM|READ_ONLY|CUSTOM_ACCESSOR` sem setter) em `temporal_duration_prototype.rs`; `PROTOTYPE_TABLE_VALUES` (11 funções e
    `epochMilliseconds`/`epochNanoseconds`) e `CONSTRUCTOR_TABLE_VALUES` (`from`, `fromEpochMilliseconds`, `fromEpochNanoseconds`,
    `compare`) em `temporal_instant.rs`, apontadas pelos `S_INFO`, com `HAS_STATIC_PROPERTY_TABLE` nas três `Structure`
    (o construtor usa `InternalFunction::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE`). Ficam eager como no C++: `@@toStringTag` nos
    protótipos, `length`/`name`/`prototype` no construtor; `constructor` do protótipo segue instalado por quem cria a classe. Medido no
    bun: antes e depois de acessar a ordem é a da tabela, depois `constructor` e `@@toStringTag`; `delete Duration.prototype.with`
    (com `round`, `abs` acessados) dá `constructor,round,abs,negated,add,...`; `delete Instant.prototype.toString` dá
    `constructor,round,add,subtract,...`; `delete Instant.from` dá `length,name,prototype,compare,fromEpochMilliseconds,fromEpochNanoseconds`;
    `delete ...constructor` e `delete Instant.length` não reificam. Teste `tests/temporal_proto_static_props.rs`. Na 1a compilação
    conferir: `reify_static_property` com `this` um `InternalFunction` (deref para `JSObject`), `GetValueFunc` de `custom_getter!`
    coagível em `static`, imports não usados (`Intrinsic`, `PropertyName` ainda em uso) e testes unitários de `temporal_instant.rs` que
    leiam propriedades pela `Structure`. Pendentes da 10n: nenhum (ver 10o).

    10o. FEITA (sem cargo, não compilada): os seis protótipos restantes (`PlainDate` 15 métodos + 16 acessores, `PlainDateTime` 16+22,
    `PlainMonthDay` 7+3, `PlainTime` 11+6, `PlainYearMonth` 11+10, `ZonedDateTime` 20+28) e os sete construtores
    (`Duration`, `PlainDate`, `PlainDateTime`, `PlainTime`, `PlainYearMonth`, `ZonedDateTime` com `from`+`compare`; `PlainMonthDay`
    só `from`) migrados. Cada arquivo ganhou `PROTOTYPE_TABLE_VALUES`/`PROTOTYPE_TABLE` ou `CONSTRUCTOR_TABLE_VALUES`/`CONSTRUCTOR_TABLE`
    ligadas no `S_INFO`, com `HAS_STATIC_PROPERTY_TABLE` em `STRUCTURE_FLAGS` (protótipos) ou herdado por `collection_constructor_structure`
    (construtores). `finish_creation` dos protótipos só põe o `@@toStringTag`; `create` dos construtores usa `create_collection_constructor`
    (sem o `before_finish`). As entradas vêm dos novos `temporal_function_entry` e `temporal_getter_entry` (`temporal_object.rs`), que
    Duration e Instant ainda duplicam localmente (dívida de DRY: migrar os dois para os helpers). Ordem conferida contra o bun 1.4.2
    para as 13 listas (tabela, depois `constructor`/`@@toStringTag`; `ZonedDateTime` termina a tabela em `epochMilliseconds`).
    `delete` de nome da tabela reifica: `PlainDate.prototype` com `add` acessado e `delete with` dá `constructor,add,toPlainMonthDay,...`;
    `delete PlainTime.from` dá `length,name,prototype,compare`; `delete Duration.compare` dá `length,name,prototype,from`;
    `delete PlainMonthDay.length` dá `from,name,prototype`. Testes novos em `tests/temporal_proto_static_props.rs` (`remaining_*`).
    A transformação foi feita por script Python em lote (os 11 arquivos são idênticos na estrutura), exceção da regra Write/Edit.
    Na 1a compilação conferir: imports soltos que sobraram, e `GetValueFunc` de `temporal_getter!` coagível em `static`.

## 11. Conferência das coleções, iteradores e afins (nenhuma tem tabela)

Conferido no upstream (`grep "@begin"` e a linha do `ClassInfo ...::s_info` de cada `.cpp`): todas as classes abaixo têm
`staticPropHashTable = nullptr` e não contêm `@begin`; instalam tudo com `JSC_NATIVE_FUNCTION`/`JSC_BUILTIN_FUNCTION`
eager no `finishCreation`. Nada a migrar, nenhum `tests/collection_proto_static_props.rs` é necessário (não há ordem
lazy para medir no bun):

- `MapPrototype`, `SetPrototype`, `WeakMapPrototype`, `WeakSetPrototype`, `FinalizationRegistryPrototype`
- `WeakRefPrototype` (no upstream o nome é `WeakObjectRefPrototype`, `WeakObjectRefPrototype.cpp`, também `nullptr`)
- `ArrayIteratorPrototype`, `MapIteratorPrototype`, `SetIteratorPrototype`, `StringIteratorPrototype`
- `ArrayPrototype` (ClassInfo `"Array"`, pai `JSArray::s_info`, `nullptr`) e `RegExpPrototype` (`nullptr`)

Dentro desse grupo de `runtime/*.cpp`, o `@begin` só aparece em `RegExpConstructor.cpp` (outro agente),
`JSIteratorHelperPrototype.cpp` (já migrado, 10k) e `IntlSegmentIteratorPrototype.cpp` (Intl, outro agente).
Portanto o `HAS_STATIC_PROPERTY_TABLE` NÃO deve ser ligado em nenhuma das doze classes acima.

    10m. FEITA (sem cargo, não compilada): `JSWebAssembly` e `WebAssemblyModuleConstructor` migrados (`js_web_assembly.rs`).
    `WEB_ASSEMBLY_TABLE_VALUES` (13 entradas na ordem do `@begin`: dez `LazyProperty` `DontEnum` mais `compile`,
    `instantiate`, `validate` como `Function` 1, enumeráveis) em `JS_WEB_ASSEMBLY_S_INFO`, `STRUCTURE_FLAGS` com
    `HAS_STATIC_PROPERTY_TABLE`. Eager como no upstream: `@@toStringTag`, `compileStreaming`, `instantiateStreaming`, `JSTag`,
    JSPI. As dez classes continuam sendo CRIADAS em `install_web_assembly`, mas num objeto descartável (as estruturas dos
    embrulhos e dos erros precisam existir para exports e erros internos, o `LazyClassStructure` do C++); os construtores
    ficam em `CLASS_CONSTRUCTORS` e o callback só os entrega, então a criação antecipada não é observável. Divergência
    assumida: custo de criação no startup. Módulo: `WEB_ASSEMBLY_MODULE_CONSTRUCTOR_S_INFO` (`customSections` 2, `imports` 1,
    `exports` 1, `Function`, enumeráveis), via `IntlClass::install_with_constructor_info` (novo) e
    `collection_constructor_structure` agora liga `HAS_STATIC_PROPERTY_TABLE` quando o `ClassInfo` tem tabela. Medido no bun:
    antes e depois de acessar a ordem é a inicial (tabela primeiro); `delete WebAssembly.Memory` (com `Table` e `validate`
    acessados) dá `compileStreaming,...,SuspendError,Table,validate,` + resto da tabela; no Module, `imports` acessado e
    `delete Module.exports` dá `length,name,prototype,imports,customSections`. Teste `tests/wasm_static_props.rs`.
    Na 1a compilação conferir: imports duplicados em `js_web_assembly.rs` (`VM`, `Structure`), `host_function!` usáveis em
    `static` (`web_assembly_compile` etc. são definidos depois da tabela, ok em Rust), `get_direct_by_name` no scratch
    devolvendo o construtor, e se algum golden `wasm_*` lia `WebAssembly.Memory` via `install_*` na ordem antiga.
    Obs.: um `Edit` meu em `intl_support.rs` colidiu com a edição concorrente do agente de Intl (parâmetro `info` solto); corrigi
    adicionando o parâmetro `info` a `install_with_prototype_info` e `install_with_constructor_info`.
