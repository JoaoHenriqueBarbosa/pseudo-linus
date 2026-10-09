# Auditoria de Symbol, WeakRef, FinalizationRegistry, WeakMap/WeakSet (2026-10-08)

## Golden

- `scripts/gen-symbol-weak-golden.js` gera `tests/golden/symbol_weak_bun.tsv` no bun 1.4.2 (3614 programas, nenhum
  descartado, sem caminho da máquina, cada execução com timeout de 10 s, nada chama `gc()`).
- `tests/symbol_weak_bun_golden.rs` é o teste, no padrão de `function_error_bun_golden.rs` (arquivo `symbol_weak_case.js`).
- Cada programa grava em `R` o resultado ou `Nome: mensagem` do erro. Ainda NÃO foi rodado contra o zjsc (sem cargo
  nesta tarefa): a primeira rodada vai listar as divergências reais.

## Mensagens medidas no bun (valem mais que o texto do pedido)

- Conversão de símbolo: `Cannot convert a symbol to a string` e `Cannot convert a symbol to a number`
  (`symbol` em minúsculas).
- WeakMap/WeakSet: `WeakMap keys must be objects or non-registered symbols`, `WeakSet values must be objects or
  non-registered symbols` (não existe `Invalid value used...` neste bun).
- WeakRef: `First argument to WeakRef should be an object or a non-registered symbol`.
- FinalizationRegistry: `register requires an object or a non-registered symbol as the target`, `... as the
  unregistration token`, `unregister requires ...`, e `register expects the target object and the holdings parameter
  are not the same...`. `cleanupSome` não existe.
- `Symbol.keyFor`: `Symbol.keyFor requires that the first argument be a symbol`.

## Leitura do runtime contra upstream

`symbol_constructor.rs`, `symbol_prototype.rs`, `weak_object_ref_constructor.rs`, `finalization_registry_prototype.rs`,
`js_weak_map.rs` (`can_be_held_weakly`): mensagens, ordem dos símbolos conhecidos (a do macro
`JSC_COMMON_PRIVATE_IDENTIFIERS_EACH_WELL_KNOWN_SYMBOL`), atributos e `[Symbol.toPrimitive]` (nome, comprimento 1,
`DontEnum|ReadOnly`) conferem com `SymbolConstructor.cpp`, `SymbolPrototype.cpp` e os `.cpp` de Weak*. Nenhuma
divergência óbvia que justificasse Edit.

## Lacunas conhecidas (documentadas nos cabeçalhos dos arquivos)

- `Symbol(desc)` e `Symbol.for(key)` usam `JSValue::to_string` sem chamar `@@toPrimitive`/`toString` de objeto nem
  propagar exceção: os programas com descrição objeto (`{ toString() ... }`) e `Symbol.for({ toString() { throw ... } })`
  devem falhar até `to_primitive` existir.
- `for` e `keyFor` entram por `put_direct` depois dos símbolos conhecidos (sem tabela `.lut.h`): a ordem de
  `Object.getOwnPropertyNames(Symbol)` pode divergir do bun; o golden mede isso.

## Correções de 2026-10-08 (sem compilar nem rodar)

- `Symbol(desc)` e `Symbol.for(key)`: o `JSValue::to_string` do porte já chama `to_primitive` de objeto; faltava
  propagar a exceção. Agora `call_symbol_body` e `symbol_constructor_for_body` devolvem `Err(Thrown::Pending)` quando
  `has_pending_exception()`.
- Ordem medida no bun 1.4.2: `Reflect.ownKeys(Symbol)` = `for, keyFor, length, name, prototype, hasInstance,
  isConcatSpreadable, asyncIterator, iterator, match, matchAll, replace, search, species, split, toPrimitive,
  toStringTag, unscopables, dispose, asyncDispose`. `Symbol.prototype` = `description, toString, valueOf, constructor,
  @@toPrimitive, @@toStringTag`. O porte instalava `for`/`keyFor` no fim e `@@toPrimitive`/`@@toStringTag` no começo.
  Corrigido em `symbol_constructor.rs` (`for`/`keyFor` antes de `finish_creation`) e em `symbol_prototype.rs`
  (novo `install_symbol_keyed_properties`, chamado em `js_global_object_init.rs` logo após gravar `constructor`).
- As demais ordens (Object, Array, Math, JSON, Reflect, Number, String, Promise, Date, RegExp, Map, Set e
  protótipos) já têm golden: `tests/golden/own_keys_bun.json` (`scripts/gen-own-keys-golden.js`) e
  `tests/builtin_own_keys_golden.rs`. Não foram comparadas por leitura nesta tarefa: a primeira rodada do teste
  lista as divergências. Nota do bun: a posição de `length`/`name` varia por classe (`Number` e `Promise` têm
  `length,name` primeiro; `Object` tem as estáticas antes), então a ordem é por classe, não regra geral.
