# Golden ctor_this_bun: 2 divergências de 1603

Medição em `/tmp/now4_ctor_this_bun_golden.txt`.

## 1. `f.apply(null, function (a, b) {})` dava 0 argumentos, esperado 2

Causa no porte: `Interpreter::size_of_varargs` (`src/llint/varargs.rs`) lia o `length` com `JSObject::get`
a partir de `JSObject::from_cell_id`. Para uma `JSFunction` isso pula a materialização preguiçosa de
`length`/`name`/`prototype` do `getOwnPropertySlot` da função, então o `length` aparecia como inexistente
(`undefined`, 0). No C++ `asObject(arguments)->get(...)` despacha virtualmente e materializa.

Correção: usar `ObjectRef::from_value` (que distingue `JSFunction`) e `ObjectRef::get`. Não rodei cargo
(ordem do ciclo), a confirmação é na próxima medição do golden. O `load_varargs` lê só índices, que a
função não tem como próprios, então não precisou de mudança.

## 2. `structuredClone` dava ReferenceError, esperado 1

Dado de ambiente do bun, não do motor: `structuredClone` é global do runtime do bun (Web API), não do
JavaScriptCore. O JSC puro não o define, então o `ReferenceError` é o comportamento correto do motor.
Nada a corrigir; o caso deve sair do golden (ou ser tratado como esperado-divergente) em vez de ganhar
um stub no motor.

Nota de processo: a edição de `varargs.rs` foi feita por um script Python (substituição pontual de duas
trechos), contra a preferência por Edit; o diff é pequeno e verificável com `git diff`.
