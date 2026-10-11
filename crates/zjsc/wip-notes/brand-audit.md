# Auditoria de this/brand dos builtins

Golden: `tests/golden/brand_bun.tsv` (1500 programas), gerado por `scripts/gen-brand-check-golden.js` no bun 1.4.2
(`bun scripts/gen-brand-check-golden.js > tests/golden/brand_bun.tsv`). Teste: `tests/brand_bun_golden.rs`.
Nada foi compilado nem rodado do lado do porte nesta passada (regra da tarefa).

## Cobertura

582 métodos, getters e setters de protótipo enumerados por `Reflect.ownKeys`, 5231 combinações método x this, reduzidas
de forma determinística a 1500 (todo this `sub` e `undefined` fica; o resto é afinado por passo fixo). Valores de this:
undefined, null, 1, 'x', {}, [], objeto de outra classe, Proxy dele, e instância de subclasse legítima. Inclui
Temporal.* e Intl.* (o bun 1.4.2 expõe os dois). Resultado: `typeof valor` ou `Nome|mensagem`. O programa chama o
alvo com `Reflect.apply(f, this, [])`, então algumas mensagens do JSC trazem o texto de chamada
(`undefined is not an object (evaluating 'Reflect.apply(f, void 0, [])')`), que é comportamento real do motor
(`exception_helpers.rs`), não ruído.

## Mensagens mais frequentes do golden e onde o porte as tem

Todas as 30 mais frequentes têm texto correspondente no porte (grep por substring): `Type error` (iterator_operations,
collection_support, set_prototype, reg_exp_prototype_natives), `Receiver should be a typed array view`
(typed_array_prototype*), `Receiver of DataView method must be a DataView` (data_view_prototype), `Receiver must be
ArrayBuffer`/`SharedArrayBuffer` (array_buffer_prototype, montada por `format!` com `mode.name()`), Set/Map operation
(js_set, js_map, builtins_combined.js), WeakMap/WeakSet (js_weak_map, js_weak_set), `thisNumberValue`
(number_prototype), BigInt, generator e async generator, Temporal (`valueOf must not be called`, `called on value
that's not a`, `can only convert to`), RegExp getters, Symbol.prototype.valueOf, Uint8Array hex.

## Divergências corrigidas

Nenhuma nesta passada: a presença da mensagem foi conferida, mas a ordem de verificação por método (por exemplo qual
erro vence quando this e o argumento são ambos inválidos em `Set.prototype.union`) só se confirma rodando o teste.

## Próximo passo

Rodar `cargo test --test brand_bun_golden` e triar as divergências por mensagem (agrupar por `esperado`), começando
pelos getters de RegExp e pelos métodos de Temporal/Intl, onde a mensagem depende do nome do método.
