# Auditoria de pânicos dos goldens (2026-10-08)

Leitura estática, sem executar cargo. Cada item diz o que está corrigido e o que ainda depende de medição.

## js_object.rs:1390 (scope_bun_golden), CORRIGIDO

`put_inline_slow` assumia que nenhum `obj` da cadeia sobrescreve `getPrototype`, porque o `Proxy` sai
antes pelo `overridesPut`. A premissa falha para o primeiro elemento da cadeia (`obj == this`), que o
teste `obj != this && overrides_put` não filtra: um `JSGlobalProxy` (o `this` do escopo global) liga
`OverridesGetPrototype`. O C++ (`JSObject.cpp`, `putInlineSlow`) chama `obj->getPrototype(globalObject)`,
que despacha pelo método da tabela. Agora o laço usa `get_prototype(&global_object)` quando o tipo
sobrescreve, convertendo o `Thrown` com `put_error_from_thrown` (o `RETURN_IF_EXCEPTION`).

## js_value_conversions.rs:507 (promise_bun_golden), ENDURECIDO, causa raiz não provada

`cell_kind` devolve `Object` para qualquer célula que não seja String, Symbol ou BigInt, inclusive
entradas do registro que não são `JSObject` (`Exception`, `GetterSetter`, reações de promessa, contextos
de combinadores). No C++ isso seria `static_cast<const JSObject*>` em célula errada (comportamento
indefinido), então a causa real é um valor com célula interna vazando para o JS. `to_primitive_preferred`
agora devolve o próprio valor quando `ObjectRef::from_value` falha. Pendente: achar quem deixa a célula
vazar (rodar promise_bun_golden com backtrace e ver o `cell_id` com `cell_registry::get`).

## js_promise_host.rs:209 (promise_bun_golden), NÃO RESOLVIDO

`caught_exception` exige exceção pendente no `VM`; o C++ também assume (`catchScope.exception()`).
Candidatos nos chamadores: `promise_constructor.rs:924` (`throw_thrown` não deixa exceção para
`Thrown::Pending`/`Termination` já consumidas), `js_promise_combinators_context.rs:110` (`throw_put_error`
com `PutError::Pending`) e `get_property_named` (`HostThrown::Pending` com exceção já limpa por um
`ThrowScope` interno). Falta o backtrace para escolher; não alterei por palpite.

## host_call.rs:202 (json_bun_golden), NÃO RESOLVIDO

É o `panic!` de `Thrown::Unported(what)`: o pânico só identifica o ponto de repasse, a causa está na
mensagem `what`, que não consta no relato. Candidatos do caminho do JSON: `PutError::Unported` de
`js_array.rs:909` (put de `length` com valor não primitivo) e `array_prototype.rs:147` (JSFunction como
`this`), e `date_prototype_natives.rs:253` (toJSON com primitivo). Pendente: rodar o golden e ler `what`.

## property_descriptor.rs:178 (builtin_own_keys_golden), objeto ainda não identificado

O pânico é de uma `Structure` sem `realm` no `slotBase` de um `CustomAccessor`. Conferi estaticamente os
candidatos (Function.prototype `caller`/`arguments`, Iterator.prototype, Symbol.prototype, RegExp
constructor, protótipos Temporal, JSGlobalObject): todos nascem com realm ou recebem `set_realm`
(`js_global_object_init.rs:103`, `js_global_object.rs:659`) e as transições copiam o realm. Não achei a
origem sem rodar. A mensagem do pânico agora traz a classe do `slotBase`
(`property_descriptor.rs`), então a próxima execução do golden aponta o objeto direto.

## math_bun_golden: atanh (87) era o teste, não o porte

`tests/math_bun_golden.rs` chamava `x.atanh()`, `x.sinh()`, `x.ln_1p()` etc. da std do Rust, enquanto
`math_object.rs` usa `glibc_hyper`. Medido no bun (`scripts/atanh-probe.js`): a fórmula de `e_atanh.c` com
o `log1p` do glibc reproduz as 505 linhas de atanh sem divergência; e o ramo `k == 0` do `log1p` do porte
(`scripts/log1p-probe.js`, FMA emulado com BigInt) bate com `Math.log1p` em 20000 amostras. O teste agora
chama `glibc_hyper::{acosh,asinh,atanh,cbrt,cosh,expm1,log10,log1p,sinh,tanh}`. Pendente reconferir
rodando o golden (o ramo `k != 0` do `log1p`, `|x| >= 0.414`, não foi sondado).
`Math.sumPrecise` (4 linhas): não investigado, falta tempo.

## Date.prototype.toJSON em primitivo (json_bun_golden)

`this_to_object` (`date_prototype_natives.rs`) devolvia `Thrown::Unported` para número, booleano e string.
`JSValue::to_object` (`host_function_support.rs`) já cria `NumberObject`, `BooleanObject`, `StringObject`,
`SymbolObject` e `BigIntObject`, então o guarda foi removido e `this_to_object` só chama `to_object`.
Sem cargo (não rodado): falta confirmar `Date.prototype.toJSON.call(1/true/'x')` esperando
`TypeError: toISOString is not a function`. Outro `Unported` do mesmo tipo: `slow_paths_object.rs:569`
(`toObject de primitivo que devolve função`, no `delete`), que é caso diferente e não foi tocado.

## Math.sumPrecise (4 divergências no math_bun_golden)

As quatro linhas (golden 21220, 21257, 21260, 21281) são somas negativas exatas: o bun 1.4.2 devolve um ulp
a mais em módulo (`Math.sumPrecise([-1])` dá `-1.0000000000000002`), enquanto o positivo é exato. O
acumulador do porte já era correto (soma exata, round-to-nearest-even); o bun tem um quirk de arredondamento
no sinal negativo, medido em grade de 1/64 de ulp: sobe se o bit de arredondamento está ligado (empate vai
para longe do zero, sem par) ou se não há nenhum bit ligado abaixo do segundo bit depois da mantissa
(soma exata e fração 0,25 ulp exata sobem; 0,125 ou 0,25 mais qualquer coisa não). Subnormais não são
afetados. Corrigido em `src/wtf/precise_sum.rs` (`compute`, ramo `negative`), com teste
`negative_rounding_quirk_of_bun`. Cargo não rodado.

## log1p, 3 divergências de 1 ulp no math_bun_golden (log1p 17071, atanh 12660, acosh 13028)

Causa: o fim dos ramos `k == 0` e `k != 0` de `log1p` (`src/runtime/glibc_hyper.rs`) estava contraído em FMA
(`f - fma(-s, hfsq + R, hfsq)` e `fma(k, ln2_hi, ...)`). O bun medido não contrai essas expressões: o glibc
devolve `f - (hfsq - s*(hfsq + R))` e `k*ln2_hi - ((hfsq - (s*(hfsq + R) + (k*ln2_lo + c))) - f)` em contas
simples; só o polinômio `R` segue com FMA (variante `r = 0`). Medido com `scripts/log1p-probe-k.js` (16 variantes
contra o `Math.log1p` do bun, 24000 pontos, incluindo k grande e perto de -1): só a variante sem FMA no fim dá
0 divergências, e ela reproduz o alvo `bfcb511c9e247c30` do caso 17071. Os casos de atanh e acosh passam pelo mesmo
ramo `k == 0` de log1p (argumentos 0,33 e 0,137). Corrigido; cargo não rodado.

## builtin_own_keys_golden: `slotBase de CustomAccessor sem realm na Structure: classe Function`

Auditoria das Structures de função: todas as `create_structure` de JSFunction, InternalFunction, construtores, BoundFunction e as `Structure::create` dos construtores nativos já passam `Some(global_object)` (o realm entra em `Structure::create_with_indexing_type`) e `new_from_previous` copia o realm das transições. A única que nasce sem realm é a do `Function.prototype` (`FunctionPrototype::create_structure(vm, None, ...)` em `JSGlobalObject::init`, porque o global ainda não existe), e o `set_realm` posterior só tocava na raiz `function_prototype_structure`, não na `Structure` viva do objeto (que pode ter transitado desde a criação). Corrigido em `src/runtime/js_global_object_init.rs`: também `function_prototype.structure().set_realm(...)`. Cargo não rodado; se o pânico persistir, o próximo suspeito é a Structure de outra função criada com `None` fora de `init`.
