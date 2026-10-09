# Mapa de cobertura dos goldens por objeto global (2026-10-08)

Medido contra o bun 1.4.2. Método: para cada método próprio de cada global (estáticos e de protótipo), contagem de
programas dos `tests/golden/*.tsv` (colunas exceto o resultado) que o citam, e quantos deles têm argumento limite
(`valueOf`, `Symbol.toPrimitive`, `NaN`, `-0`, `Infinity`, `2**32`, `2**53`). Citar o nome não é o mesmo que testar a
coerção: leia o número como indício, não como prova. Atenção: o molde `gen-accessor-golden.js` não existe; o gerador
vizinho é `gen-getter-setter-golden.js` (saída `accessor_bun.tsv`).

## Arquivos

- 126 geradores `scripts/gen-*-golden.js` e cerca de 110 `tests/golden/*.tsv` (mais harnesses `*_harness.js`).
- Formato dominante: coluna 1 fonte (JSON), última coluna resultado (JSON) de `globalThis.R` (molde
  `gen-function-error-golden.js` e `tests/function_error_bun_golden.rs`). Date usa `fuso<TAB>fonte<TAB>KIND<TAB>REPR` com
  `set_time_zone_spec_override`.

## Cobertura por global (golden principal)

| Global | Goldens | Situação |
|---|---|---|
| Object, Reflect | object_model, reflection, reflect, own_keys, key_order, delete, accessor, class | boa |
| Function | function_proto, function_error | boa |
| Array | array, collections, coercion | boa |
| String | string, string_unicode, case_mapping, coercion, annexb | métodos HTML, trimLeft/Right e substr rasos: **fechado por builtins_gap** |
| Number, Math, BigInt | number_*, math, bigint, bigint_bun, parse_double, number_to_string | boa; `Number.parseInt` (1 programa) e `Math.random` (7) rasos |
| Boolean, Symbol | coercion, symbol_weak | boa |
| Date | date, date_proto (4000+ linhas, 2 fusos), date_parse, date_pattern, date_range, date_tz, datetime_* | boa; `Date.now` só 20 programas |
| RegExp | regexp, regexp_more, regexp_legacy, regexp_opt, regexp_tables, regexp_v, regexp-exec, regexp-syntax | flags getters, `flags`, `source`, `compile`, `toString` rasos: **fechado por builtins_gap** |
| JSON | json, json_number | boa |
| Map, Set, WeakMap, WeakSet, WeakRef, FinalizationRegistry | collections, symbol_weak | boa; Set methods novos cobertos por collections |
| Promise | promise, async | `withResolvers` (7) e `try` (10) rasos; combinadores sem limite de coerção |
| Proxy | proxy, proxy_class | boa |
| ArrayBuffer, SharedArrayBuffer, DataView, TypedArrays | buffer, buffers, typedarray, typedarray_more | boa |
| Atomics | atomics | boa |
| Iterator, Generator, AsyncGenerator | iterator, async, control_flow | `Iterator.zip`, `zipKeyed`, `chunks`, `windows` rasos (1 a 2 programas cada) |
| Error | error, errors, error_stack, stack, stack_more, function_error | `Error.appendStackTrace` e `prepareStackTrace` estático sem programa |
| globais (`parseInt`, `parseFloat`, `isNaN`, `isFinite`, `encodeURI*`, `decodeURI*`, `escape`, `unescape`, `eval`, `structuredClone`, `queueMicrotask`) | globals, global_semantics, eval, coercion | todas citadas; `queueMicrotask` (27 menções) e `structuredClone` (166) merecem limite de coerção |
| Intl, Temporal, Wasm, Module, ShadowRealm | intl_*, temporal_*, wasm_*, module_*, shadow_realm | fora do escopo desta tarefa |

## Maiores buracos fechados nesta rodada

`scripts/gen-builtins-gap-golden.js` gera `tests/golden/builtins_gap_bun.tsv` (988 programas), consumido por
`tests/builtins_gap_bun_golden.rs`:

1. String Annex B: 9 métodos de marcação sem atributo, `anchor`/`fontcolor`/`fontsize`/`link`, `trimLeft`/`trimRight`
   (identidade com `trimStart`/`trimEnd`, espaços Unicode), `substr` (36 conjuntos de argumentos), `search` (despacho por
   `Symbol.search`, `lastIndex`).
2. RegExp: 8 getters de flag (receptores inválidos, subclasse, proxy, descritores), `flags` (ordem de leitura dos getters,
   exceção no meio), `source` (escape de `/` e quebras de linha), `toString`, `compile` (reinício de `lastIndex`,
   TypeError de flags com RegExp, objeto congelado, ordem de coerção).

## Próximos buracos (ordem sugerida)

1. Iterator helpers novos (`zip`, `zipKeyed`, `chunks`, `windows`) com coerção e fechamento do iterador.
2. Promise: `withResolvers`, `try`, `race`/`any`/`allSettled` com `Symbol.species` e `resolve` que lança.
3. `Number.parseInt`/`parseFloat` como identidade das globais, `Math.random` (forma, não valor), `Date.now`.
4. `queueMicrotask` e `structuredClone`: argumentos limite, ordem de coerção, erros de `DataCloneError`.
5. `Error.appendStackTrace` e `Error.prepareStackTrace` estático.

## Lacunas de `src/runtime` e `src/llint` varridas (2026-10-08, sem compilar nem rodar)

Varredura de `Unported(`, `todo!`, `LACUNA` e `não portado`. A maioria dos `Unported` restantes é invariante de
registro de células (callee que não é `JSFunction`, escopo sem célula) ou código morto: `JSObject::from_value`
alcança a célula de `JSFunction`, então os ramos "JSFunction como this de Array.prototype", "protótipo/receptor
JSFunction" em `proxy_object.rs` e "toObject que devolve função" em `slow_paths_object.rs` não disparam.

Fechadas nesta rodada (escritas, NÃO compiladas, falta o golden contra o bun):

1. `interpreter/execute_eval.rs`, `ensureBindingExists` do `eval` com `var`: o `PutError` de
   `variable_object.put(.., shouldThrow = true)` deixava `Unported`; agora vira a exceção pendente
   (`throw_put_error`), como o `ThrowScope` do C++ (global não extensível ou com setter que lança).
2. `llint/handlers_enumerator.rs`, `enumerator_key`: `propertyName` do for-in que não é string fora do
   `IndexedMode` deixava `Unported`; agora aplica `toPropertyKey` (número, símbolo ou objeto com `toString`).
   Teste a escrever: `for (var k in o) { k = 1; o[k] }` e `for (k in o) { k = {toString(){throw 1}}; o[k] }`.

Abertas, por probabilidade de uso: `promise_constructor.rs` `create_internal_field_tuple` (só com contexto
assíncrono); `stack_visitor.rs` `createArguments` com callee que não é `JSFunction` (`fn.arguments` dentro de
host function); jobs de módulo assíncrono e `JSWebAssemblyStreamingContext` (`promise_constructor.rs`).

## Divergências de mensagem conferidas contra `src/runtime`

Conferidas e iguais: getters de flag (`The RegExp.prototype.X getter can only be called on a RegExp object`), `flags`
(`... getter can only be called on an object`), `Cannot supply flags when constructing one RegExp from another.`,
`Invalid flags supplied to RegExp constructor.`, `RegExp.prototype.@@search requires that |this| be an Object`. Nos
métodos HTML e `trim*` o bun responde `Type error` para `this` nulo ou indefinido em `trim*` e `substr`, e
`String.prototype.X requires that |this| not be null or undefined` nos HTML; o `src` usa o mesmo texto (macro
`html_function!` e `string_host_function!`). Nenhuma divergência óbvia achada sem rodar o teste; o primeiro `cargo test
builtins_gap_bun` do integrador decide.

## BigInt contra o bun (`bigint_bun`)

`tests/golden/bigint_bun.tsv` (11308 programas, `scripts/gen-bigint-bun-golden.js`, `tests/bigint_bun_golden.rs`) já
cobria literais com separador, `BigInt()` com strings, `asIntN`/`asUintN`, operadores com negativos e erros,
comparação mista, `toString`/`toLocaleString`, conversões para Number, `JSON.stringify`, `Math.max`,
`BigInt64Array` e `Atomics`. Faltavam os BigInt gigantes com hash dentro do programa: acrescentadas 19 linhas
(`2n ** 100000n` em radix 2, 10, 16, 36, `3n ** 50000n`, divisão, módulo, bitwise e `asUintN(100000, -1n)`), cada uma
devolvendo `comprimento:FNV-1a` do texto. O `src/runtime/js_big_int*.rs` não foi comparado por execução (sem cargo);
o primeiro `cargo test bigint_bun_golden` do integrador decide.

## Texto-fonte de função contra o bun (`function_source_bun`)

`tests/golden/function_source_bun.tsv` (1037 programas, `scripts/gen-function-source-golden.js`,
`tests/function_source_bun_golden.rs`) complementa o `function_error_bun` (que já tinha cerca de 150 expressões de
`toString`). Cobre declaradas (nomes `get`/`set`/`of`, escapes `a`, Unicode), expressões, arrows com e sem
parênteses e async, métodos em 11 chaves (computada, símbolo, numérica, string, escape, `get`/`set`/`static`/`async`)
x 6 tipos (comum, generator, async, async generator, getter, setter) x 3 contextos (objeto, protótipo de classe,
estático), privados `#m`, classes (extends, campos, static blocks, comentários, anônimas, construtor padrão), cabeçalhos
com comentários e quebras de linha, `Function`/`GeneratorFunction`/`AsyncFunction`/`AsyncGeneratorFunction` (texto
com `anonymous` e newlines), bound (incluindo `name`/`length` redefinidos), nativas (lista fixa mais `Math`, `Reflect`
e `JSON` enumerados), Proxy, eval, `toString` em objeto que não é função e `toString` após `defineProperty(f, 'name')`.
Cada programa devolve `[texto, name, length]` em JSON. Medido no bun 1.4.2: a forma nativa é a linha única
`function name() { [native code] }` (não a de quatro linhas), e bound mostra o nome do alvo sem `bound ` (`function f()`,
`function ()` para arrow). O `function_prototype.rs` e o `js_function.rs::to_string` já seguem isso; nenhuma divergência
corrigida sem rodar. O bun roda o arquivo como módulo (estrito), então `function static/await/yield/let` foi descartado
pelo gerador. O primeiro `cargo test function_source_bun_golden` do integrador decide.

## JSON de borda (`json_more_bun`)

`scripts/gen-json-more-golden.js` gera `tests/golden/json_more_bun.tsv` (3190 programas, descartando os 131 que já estão
em `json_bun.tsv`) e `tests/json_more_bun_golden.rs` o confere. Complementa o golden base com: matriz de token ruim
(19 tokens) em 17 pontos da gramática; gramática de números (como elemento, valor de propriedade, entre vírgulas);
escapes `\a`..`\z`, `\u` truncado/inválido, controles crus 0x00..0x1f em string e em chave; literais truncados e em
caixa errada; espaços Unicode e BOM; `JSON.parse` com não strings e `toString` que lança; profundidade de objeto,
array e mistos (10 a 100000); reviver (buracos, deleção, `this`/holder, `context.source` com mutação do holder,
`__proto__`, reviver bound/Proxy/async/gerador/classe); stringify de 49 valores especiais em 13 contextos (replacer
array e função, indent, toJSON, esparso, Proxy); lista de propriedades exótica (Proxy, getters, array-like); `this` do
replacer; space (59 formas, incluindo truncamento em 10 por unidade UTF-16); ciclos (mensagem exata, Proxy, via
replacer); getters que lançam e ordem de leitura; Map/Set/typed arrays/wrappers/BigInt com `toJSON` no protótipo/Date
nos limites; surrogates soltos; `rawJSON`/`isRawJSON` (bordas de espaço, valores não primitivos, congelamento) e a
meta de `JSON`. Medido no bun 1.4.2: as mensagens de posição são as da família do `literal_parser.rs` (`Unexpected EOF`,
`Unexpected token 'X'`, `Unexpected identifier "..."`, `Unrecognized token 'X'`, `Expected ']'|'}'`, `Expected ':'
before value in object property definition`, `Property name must be a string literal`, `Unexpected comma at the end of
array expression`, `Invalid number`, `Invalid digits after decimal point`, `Invalid escape character X`, `\u must be
followed by 4 hex digits`, `"\uZZZZ" is not a valid unicode escape`, `Unterminated string`, `Single quotes (') are not
allowed in JSON`, `Unable to parse JSON string`, `Unexpected content at end of JSON literal`, `Could not parse value
expression`), todas já presentes no `src/runtime/literal_parser.rs`, e as de `rawJSON` em `json_object.rs`. Nenhuma
divergência de texto achada por grep; o primeiro `cargo test json_more_bun_golden` do integrador decide (profundidade
com `RangeError: Maximum call stack size exceeded.` é a mais provável de divergir).

`scripts/gen-number-edge-golden.js` gera `tests/golden/number_edge_bun.tsv` (1025 programas, nenhum repetido dos goldens
`number_format*`, `number_compact` e `bigint_bun`; `math_bun` e `number_to_string` usam outro formato, a dedução ali é
por tema) e `tests/number_edge_bun_golden.rs` o confere (piso de 1000). Cobre: `toFixed`/`toPrecision`/`toExponential`
(valores de borda x dígitos 0..100 e erros de intervalo), `toString(radix)` (radix 2..36 e inválidos), `parseFloat`,
`parseInt` (radix x texto), `Number()` e `+` unário sobre texto difícil, objetos e BigInt, predicados de `Number`, as 28
funções unárias de `Math` com 29 valores especiais comparados em bits (`Float64Array`), binárias (`atan2`, `pow`, `imul`,
`hypot`, `max`, `min`), `**` e `%` (zeros com sinal, infinitos, subnormais), `>>>`, shifts e bit a bit (contagem
módulo 32, `ToInt32` de valores grandes), `++`/`--` em Number, string, objeto e BigInt (prefixo, sufixo, propriedade,
índice) e a matriz de operadores binários BigInt x Number/string/objeto. As matrizes entram por amostragem de passo
fixo (`STRIDE`), as listas escritas à mão inteiras. Não foi rodado no `cargo`; o primeiro `cargo test
number_edge_bun_golden` do integrador decide.

`scripts/gen-buffer-edge-golden.js` gera `tests/golden/buffer_edge_bun.tsv` (1703 programas, prelúdio próprio, distintos
de `atomics_bun`, `buffer_bun`, `buffers_bun` e `typedarray*_bun`) e `tests/buffer_edge_bun_golden.rs` o confere (piso de
1500). Cobre Atomics (as sete operações RMW, `load`/`store`, `isLockFree`, `notify`, `wait` com timeout 0, `waitAsync`
com `{async,value}`, tipos inválidos e índices fora do intervalo), `SharedArrayBuffer` (`growable`, `grow`,
`maxByteLength`, `slice`), `ArrayBuffer` redimensionável (`resize`, `transfer`, `transferToFixedLength`, `detached`),
typed arrays com length-tracking ou fora do limite após `resize`/`transfer`, iteradores e callbacks que redimensionam,
`DataView` sobre redimensionável (getters e setters por tipo) e mensagens de erro. Timeouts infinitos ficam de fora
porque travariam o bun. Não foi rodado no `cargo`; o primeiro `cargo test buffer_edge_bun_golden` do integrador decide.

`scripts/gen-module-edge-golden.js` gera `tests/golden/module_edge_bun.tsv` (629 programas, mapa de arquivos no mesmo
formato de `module_more_bun.tsv`) e `tests/module_edge_bun_golden.rs` o confere (piso de 600, mesmo runner e mesma
tolerância a erros `BuildMessage`/`ResolveMessage`/`AggregateError` do bun). Cobre: ciclos com TDZ em export (nove formas
de declaração x cinco leituras, namespace de si mesmo, reexport em ciclo), `export *` conflitante, ambíguo, em diamante e
em ciclo, `export default` de classe e função anônima com `name` e descritores, `import.meta` (extensibilidade, url,
eval, erros de sintaxe), `import()` dinâmico com rejeição (módulo ausente, throw, ambíguo, TDZ, opções inválidas, cache
de rejeição), top-level await com ordem entre irmãos, pai e filho e dependência compartilhada, namespace objects
(`Symbol.toStringTag`, extensibilidade, descritores, Reflect e Object.* mutadores, namespace de `main.mjs` em TDZ),
re-export de namespace (`export * as`, nome string, default) e import attributes (`with {type:'json'}`, `assert`,
duplicados, chaves extras, `import()` com `with`). Excluído de propósito um caso de await que nunca resolve (o bun
termina sem saída, sem oráculo determinístico). Não foi rodado no `cargo`; o primeiro `cargo test
module_edge_bun_golden` do integrador decide.

## String Unicode de borda, segunda leva (`string_unicode_more_bun`)

`scripts/gen-string-unicode-more-golden.js` gera `tests/golden/string_unicode_more_bun.tsv` (609 programas medidos no bun
1.4.2, nenhum repetido de `string_unicode_bun.tsv` nem de `string_bun.tsv`) e `tests/string_unicode_more_bun_golden.rs` o
confere. Os nomes `string_unicode_bun*` já existiam e ficaram intactos. A matriz completa tem 7301 programas e o gerador
amostra por passo fixo (`TARGET`, padrão 600; `TARGET=100000` gera tudo). Famílias: `normalize` (62 amostras x 11
argumentos de forma, Hangul, ordem canônica, ligaduras, compatibilidade), caixa (90 amostras nas quatro conversões, 10
locales em 13 amostras), `localeCompare` (51 pares x 12 opções), `isWellFormed`/`toWellFormed`, `at`/`codePointAt`/`charAt`
com 10 cadeias de surrogates x 23 índices, `padStart`/`padEnd` com preenchimentos longos e surrogates, `replaceAll`/`replace`
com 20 padrões `$`, `split` com 33 regex Unicode (flags u, v, s, i, lookaround, `\p{..}`) x 12 cadeias, `String.raw` e
templates, `matchAll` com 33 padrões x 10 cadeias, `lastIndex` com `g`/`y`/`u`, índices de `d`, sintaxe da flag `v`
(`--`, `&&`, `\q{}`, `RGI_Emoji`) e dobra de caixa com `iu`. Não foi rodado no `cargo`; o primeiro `cargo test
string_unicode_more_bun_golden` do integrador decide.

`scripts/gen-control-flow-more-golden.js` gera `tests/golden/control_flow_more_bun.tsv` (1020 programas, descartando os
que já estão em `control_flow_bun.tsv`) e `tests/control_flow_more_bun_golden.rs` o confere (mesmo harness do golden
base). Complementa com: geradores (next/return/throw em cada ponto de suspensão x sete formas de finally com yield,
return, throw, break e continue, try aninhado, laços dentro do gerador, `yield` em 18 posições de expressão); `yield*`
(15 iteradores internos, entre eles sem return, com return que lança, return não objeto, getters de done/value, x 4
operações, em dois contextos, mais não iteráveis e cadeias aninhadas); destructuring (16 padrões x 7 fontes de
iterador instrumentado, declarações let/const/var, parâmetros, for-of, catch, ordem de avaliação de alvo/chave/default,
rest com Proxy, fechamento do iterador em cada falha); spread em chamada, array, new e objeto (14 fontes x 9 contextos,
getters, Proxy, ordem); switch com fallthrough (10 corpos x 5 valores, completion value); labels (33 formas e 22 erros
de sintaxe); for-of/for-in (5 laços x 18 combinações de break/continue/throw em try/catch/finally, fechamento do
iterador por return/throw/break, mutação da coleção, for-in com getters, delete e protótipo). Resultados gravados como
o bun 1.4.2 devolveu; o primeiro `cargo test control_flow_more_bun_golden` do integrador decide. Nota de colisão: os
nomes pedidos (`gen-control-flow-golden.js`, `control_flow_bun.tsv`, `control_flow_bun_golden.rs`) já existiam
(não rastreados, 1571 programas), então este lote entrou com o sufixo `more` para não sobrescrever.

## Eval e escopo, segunda leva (`eval_scope_bun`)

`scripts/gen-eval-scope-golden.js` gera `tests/golden/eval_scope_bun.tsv` (499 programas medidos no bun 1.4.2, nenhum
repetido de `eval_bun.tsv` nem de `scope_bun.tsv`) e `tests/eval_scope_bun_golden.rs` o confere. Cada programa roda como
script num processo novo, como em `gen-eval-golden.js`. A matriz completa tem mais de 2300 programas e o gerador amostra
por passo fixo (`TARGET`, padrão 520; `TARGET=100000` gera tudo). Famílias: eval direto e indireto (vazamento de var,
strict, let/const, function, valor de conclusão, `this`/`arguments`/`new.target`/`super` em 12 contextos, conflitos de
declaração, parâmetros com default), `new Function`/GeneratorFunction/AsyncFunction/AsyncGeneratorFunction (parâmetros
com comentários e vírgulas, corpos inválidos, `toString`, `name`/`length`), `with` + `Symbol.unscopables` (30 objetos x
18 corpos, Proxy e ordem de `has`/`get`), hoisting de function em bloco (Annex B, ~85 casos), `arguments` mapeado vs não
mapeado (descritores, freeze, strict, parâmetros duplicados), closures em laços, getters e setters em literais e classes
(nomes, privados, `super`, herança), label + break/continue em blocos, e vírgula/`void`/`typeof`/`delete` de borda. Não foi
rodado no `cargo`; o primeiro `cargo test eval_scope_bun_golden` do integrador decide.

- **TypedArray e DataView de borda** (`scripts/gen-typedarray-edge-golden.js`, `tests/golden/typedarray_edge_bun.tsv`,
  `tests/typedarray_edge_bun_golden.rs`, 529 programas): `set` com overlap e entre tipos, `subarray`/`slice`/`fill`/
  `copyWithin` com índices de borda, `sort`/`toSorted` com NaN e -0, `with`/`at`/`findLast`, `includes`/`indexOf` com NaN,
  `join`, `from`/`of`, construtores, conversões entre tipos (Uint8Clamped, Float16Array, BigInt64/BigUint64), `Math.f16round`,
  `DataView` com endianness e bordas, buffers redimensionáveis e destacados, species e mensagens de erro. A matriz completa
  tem 3700 programas; o gerador amostra 1 a cada 7 (`STRIDE`) para o teste ficar rápido, e baixar o passo amplia a cobertura.
  Os nomes não colidiam com `typedarray_bun` nem `typedarray_more_bun`. Resultados gravados como o bun 1.4.2 devolveu (tem
  `Float16Array`); não foi rodado no `cargo`, o primeiro `cargo test typedarray_edge_bun_golden` do integrador decide.

- **json_bun_golden, triagem das 39 divergências** (sem cargo): (1) BOM, `é😀`, U+2028/2029 e `space` não ASCII vindos
  como mojibake: `evaluate_named_script_result` já usa `from_utf8` (editado 17:54, a saída medida é das 17:59, possível
  binário anterior) e `create_from_utf8` está correto; remedir antes de investigar mais. (3) `a.length=4294967295; a.length`
  dava `-1`: `get_length` em `llint/dispatch_ext.rs` fazia `Int32(array.length() as i32)`; corrigido para `JSValue::from_u32`.
  (4) `new JSON.parse(..)`, `new JSON()`, `JSON()`: o texto `(evaluating '...')` sai com o início errado (`;\nfunction S(v){try`,
  mesmo comprimento da expressão certa, início no fim de `"use strict"`): a busca de `ExpressionInfo` do erro de
  construct/call de callee nativo usa o código/offset errado; e o esperado de `JSON()` é `is not a function. (In 'JSON()',
  'JSON' is an instance of JSON)`, mensagem do appender de objeto, não do `near`. `Date.prototype.toJSON.call(null)` espera
  `(evaluating 'f()')`, falta o site; `.call(1)`/`.call("x")` panicam em `toObject` de primitivo (NumberObject, StringObject
  não portados). Faltam globais do host: `Buffer`, `URL`, `URLSearchParams`, `TextEncoder`. Proxy em `JSON.stringify` de array
  não lê os índices (`get:0,get:1`).

## Nota anexada: golden de RegExp de borda (`regexp_edge`)

`scripts/gen-regexp-edge-golden.js` gera `tests/golden/regexp_edge_bun.tsv` (2542 programas, bun 1.4.2) e
`tests/regexp_edge_bun_golden.rs` o consome. Cobre named groups duplicados e `\k<n>`, lookbehind com capturas e
backreferences, `\p{...}` (General_Category, Script, Script_Extensions, binárias, emoji, com e sem `i`), flag v
(`--`, `&&`, `\q{}`, propriedades de strings, negação), flag d (indices e indices.groups), lazy aninhado e
catastrophic em tamanhos seguros, sticky e `lastIndex` (incluindo propriedade não gravável e Unicode), `Symbol.replace/
split/match/matchAll/search` personalizados e Proxy de rastreio, `RegExp.escape`, modifiers inline `(?i-s:...)` e
mensagens de SyntaxError de padrão e flags. Nota: o bun tem `RegExp.escape`, modifiers e grupos duplicados. Programas
com surrogate solto ficam de fora (não cabem em `String`). Não foi rodado `cargo`; o primeiro
`cargo test regexp_edge_bun_golden` do integrador decide. Os nomes pedidos não colidiam com nada existente.

JSON.stringify de Proxy de array: a causa estava no `JsonHost::get` de `src/runtime/json_host.rs`, que lia
por `get_property_slot` (caminho que não passa pelo trap `get` do Proxy por índice). Agora, quando a base é objeto,
usa o `[[Get]]` genérico (`ObjectRef::get`), como `object->get(globalObject, index)` do JSC; `is_array` e
`length_of_array_like` já atravessavam o Proxy. Esperado no golden: `get:toJSON,get:length,get:0,get:1`. Não foi rodado
`cargo`; o integrador confere `json_bun_golden` e a ordem ownKeys/getOwnPropertyDescriptor/get do Proxy de objeto.

`scripts/gen-subclass-edge-golden.js` gera `tests/golden/subclass_edge_bun.tsv` (1260 programas, bun 1.4.2) e
`tests/subclass_edge_bun_golden.rs` o consome. Cobre `class extends` de Array/Map/Set/WeakMap/Promise/RegExp/Error e
subtipos/AggregateError/Date/Function/ArrayBuffer/DataView/typed arrays/Boolean/Number/String/Object (Symbol e Proxy
dão erro), `Reflect.construct` com newTarget diferente (protótipo do newTarget, fallback para o realm do newTarget,
newTarget Proxy), cross-realm por `vm.createContext`, `Symbol.species` (Array, Promise, ArrayBuffer, typed arrays,
RegExp), constructor sobrescrito, `toStringTag`, `instanceof`/`hasInstance`, `length`/`name`, `super` em objeto literal,
`Object.setPrototypeOf` em instâncias de builtins, `Error.captureStackTrace` e `cause`. Os programas cross-realm usam o
global `vm` (node:vm), que o runner Rust precisa expor; sem ele essas linhas falham de propósito. Não foi rodado `cargo`;
o primeiro `cargo test subclass_edge_bun_golden` do integrador decide (são esperadas muitas divergências).

- **APIs recentes** (`scripts/gen-recent-apis-golden.js`, `tests/golden/recent_apis_bun.tsv`, `tests/golden/recent_apis_prelude.js`,
  `tests/recent_apis_bun_golden.rs`, 1731 programas, bem acima dos ~500 pedidos porque as matrizes API x fonte x opção são
  baratas): `Promise.withResolvers`/`try` (ordem de microtarefas com `tick`), `Array.fromAsync` (síncronos, assíncronos,
  array-likes, mapFn assíncrona, `this`, construtores), Iterator helpers (`map`/`filter`/`take`/`drop`/`flatMap`/`reduce`/
  `some`/`every`/`find`/`forEach`/`toArray`, `Iterator.from`, `Iterator.concat`), `Object.groupBy`/`Map.groupBy`, os sete
  métodos de `Set` com set-likes exóticos, `Error.captureStackTrace`/`cause`/`AggregateError`/`Error.isError`,
  `Uint8Array.fromBase64`/`toBase64`/`setFromBase64`/hex, `Math.sumPrecise`, `RegExp.escape`. Medido antes: o bun 1.4.2 tem
  todas, menos `Array.prototype.group` (ausente, também `groupBy`/`groupToMap`, só conferidos por `typeof`); `Temporal` e
  `structuredClone` ficaram de fora. Programas que travavam o bun (iterador infinito) foram removidos. Os resultados são os
  do bun; não foi rodado no `cargo`, o primeiro `cargo test recent_apis_bun_golden` do integrador decide.

- **Objeto global e funções globais de borda** (`scripts/gen-global-edge-golden.js`, `tests/golden/global_edge_bun.tsv`,
  `tests/global_edge_bun_golden.rs`, 553 programas, via `vm.runInThisContext`): parseInt/parseFloat/isNaN/isFinite com
  entradas exóticas e radix de borda, encodeURI/decodeURI(Component)/escape/unescape (surrogates soltos, URIError, `%`
  inválido), descritores de `globalThis` (var/function/let/const/class/atribuição solta, redeclaração por eval indireto),
  delete de globais, `this` no topo e em funções sloppy/strict, call/apply/bind (length/name de bound, `new` com bound,
  bound de bound), Symbol.hasInstance, `Function.prototype.caller/arguments`, `arguments.callee`, eval como identificador,
  descritores de name/length/prototype de cada forma de função e `toString` de nativas e de fonte. A matriz completa tem
  4426 programas; o gerador amostra por passo fixo (`TARGET`, padrão 520; `TARGET=100000` gera tudo). Resultados como o
  bun 1.4.2 devolveu; não foi rodado no `cargo`, o primeiro `cargo test global_edge_bun_golden` do integrador decide.

- **Sintaxe moderna de borda** (`scripts/gen-modern-syntax-golden.js`, `tests/golden/modern_syntax_bun.tsv`,
  `tests/modern_syntax_bun_golden.rs`, 570 programas): optional chaining em todas as posições (`delete`, parênteses,
  tagged proibido, privados, `super`), `??` com `||` e `&&`, atribuição lógica (`&&=`, `||=`, `??=`) com getters, setters,
  const, Proxy, congelados, privados e curto-circuito, `**`, separadores numéricos, BigInt literais e operadores, templates
  com escapes inválidos em tagged, static blocks, `using`/`await using` (o bun 1.4.2 tem), `import.meta` e `new.target` fora
  de lugar, regex vs divisão, ASI de borda, Unicode em identificadores e escapes `\u{...}`, comentários HTML, hashbang,
  getters com nomes numéricos/string/computados, spread de objeto com getters, rest/destructuring e label + function.
  Medido com `vm.runInThisContext` (JSC puro); erro de sintaxe por trecho passa por `eval` indireto e grava
  `throw Nome: mensagem`, e script inteiro que não compila fica `<undefined>`. A matriz tem 2277 programas; o gerador
  amostra 1 a cada 4 (`STRIDE`), e `STRIDE=1` amplia a cobertura. Não foi rodado no `cargo`; o primeiro
  `cargo test modern_syntax_bun_golden` do integrador decide.

## Nota anexada: golden de operadores de borda (`operator_edge`)

`scripts/gen-operator-edge-golden.js` gera `tests/golden/operator_edge_bun.tsv` (1543 programas, bun 1.4.2, determinístico
em duas rodadas) e `tests/operator_edge_bun_golden.rs` o consome. Cada programa roda por `vm.runInThisContext` (modo
sloppy, `'use strict'` só onde o caso pede) e `R` é capturado na saída. Cobre `==`/`===` entre 17 tipos (todos os pares),
BigInt vs string/número, Symbol e wrappers; relacionais com BigInt e string numérica (matriz 8x8 e hooks com ordem);
`+` com objetos, Date e `Symbol.toPrimitive`; typeof/void/in/delete e instanceof com `Symbol.hasInstance`; `++`/`--`
em 26 valores (variável e propriedade); ordem de avaliação de `a[b()] = c()`, chamadas, desestruturação e spread;
compound assignment (15 operadores) com getters/setters e Proxy; vírgula, `**` com negativos, shifts/bitwise/módulo/
divisão com BigInt, NOT em NaN/Infinity; Object.is e SameValueZero (Map/Set/includes/indexOf); chave de propriedade
(-0, números grandes, símbolos, objetos com hooks); matriz de 27 contextos x 11 objetos de ToPrimitive com log de hooks.
`document.all` não existe no bun (só `typeof document`). Dois programas ficam `<undefined>` (não compilam no JSC). O
`typeof` de função de `T(...)` usa o `S` do prelúdio, então valores de função saem como `function`. Não foi rodado
`cargo`; o primeiro `cargo test operator_edge_bun_golden` do integrador decide. Os nomes pedidos não colidiam com nada.

- Valor de completude de statements: `scripts/gen-completion-value-golden.js` gera `tests/golden/completion_value_bun.tsv`
  (1153 programas) e `tests/completion_value_bun_golden.rs` confere. Cobre if/else, laços com break/continue (UpdateEmpty),
  switch, try/catch/finally (inclusive break/continue/throw no finally), blocos rotulados, with, `var`/`let`/função/classe
  (vazios), blocos vazios, eval e wrappers aninhados. O bun mede com `vm.runInNewContext` (contexto limpo por linha); o
  porte usa `(0, eval)(fonte)` dentro de `try` e formata com o mesmo `S` (`tipo:valor`, `throw Nome`), pois
  `evaluate_script_sequence_result` só devolve uma global. Não foi rodado `cargo`; o primeiro
  `cargo test completion_value_bun_golden` do integrador decide.

## Nota anexada: golden complementar de ShadowRealm (`shadow_realm_more`)

Os nomes pedidos (`gen-shadow-realm-golden.js`, `shadow_realm_bun.tsv`, `shadow_realm_bun_golden.rs`) já existiam, não
versionados e com 1617 programas, então nada foi sobrescrito: o complemento saiu como
`scripts/gen-shadow-realm-more-golden.js`, `tests/golden/shadow_realm_more_bun.tsv` (972 programas, bun 1.4.2, via
`vm.runInThisContext` e `globalThis.R`) e `tests/shadow_realm_more_bun_golden.rs` (thread de 256 MiB com
`VM::set_thread_stack_budget`). Cobre retorno do evaluate (primitivos, objetos, funções, 29 tipos de objeto), wrappers
(name/length com descritores patológicos, protótipo, this, new, bind, Proxy), lançamentos e SyntaxError entre realms,
globais isolados, subclasses, não-string, evaluate recursivo e a forma do importValue. Não foi rodado `cargo`; o primeiro
`cargo test shadow_realm_more_bun_golden` do integrador decide. Pode haver sobreposição parcial com o golden existente.

## Nota anexada: golden de coleções sob mutação (`collection_mutation`)

`scripts/gen-collection-mutation-golden.js` gera `tests/golden/collection_mutation_bun.tsv` (914 programas, bun 1.4.2) e
`tests/collection_mutation_bun_golden.rs` o consome. Cobre delete, re-add, clear e add durante forEach, for-of e iterador
manual (matriz de 8 mutações x 2 passos, mais laços que crescem, trocam e aninham), iteradores esgotados que voltam após
add (por tipo de iterador), ordem de inserção, chaves -0/NaN/BigInt/símbolo/objeto (matriz de 81 pares), chaves válidas e
inválidas de WeakMap/WeakSet, Map.groupBy e Object.groupBy, size, os sete métodos novos de Set contra 33 set-likes que
registram size/has/keys (size NaN, BigInt, símbolo, has e keys inválidos, next que lança) e contra não set-likes,
mutação de `this` durante os métodos, subclasses e species, construtores de Map/Set/WeakMap/WeakSet com iterável que lança
no meio (IteratorClose, return que lança, adder substituído) e getOrInsert/getOrInsertComputed de Map e WeakMap (o bun
tem os quatro). Três programas que laçam sem fim (forEach que re-adiciona a chave visitada no passo 1; isSupersetOf com
keys infinito) ficam fora por timeout. Não foi rodado `cargo`; o primeiro `cargo test collection_mutation_bun_golden` do
integrador decide. Os nomes pedidos não colidiam com nada existente.

## Nota anexada: golden de BigInt de borda (`bigint_edge`)

`scripts/gen-bigint-edge-golden.js` gera `tests/golden/bigint_edge_bun.tsv` (1043 programas, bun 1.4.2, medidos com
`vm.runInThisContext` e `globalThis.R`) e `tests/bigint_edge_bun_golden.rs` o consome (thread de 256 MiB com
`VM::set_thread_stack_budget`). Cobre `BigInt()` de 49 strings (hex/octal/binário, espaços, sinais, vazio, `1n`) e de
valores, literais no fonte, asIntN/asUintN com bits 0/64/2**53 e inválidos, toString em radix 2..36 de três números de
milhares de dígitos (comprimento:soma de dígitos) e viagens de ida e volta, operadores (`/ % & | ^ ** << >>` com
negativos, `~`, divisão por zero, `>>>`), comparação com Number e string, BigInt64Array/BigUint64Array/DataView/Atomics,
JSON, Math, mistura de tipos, atribuições compostas, toLocaleString e Intl.NumberFormat em vários locales,
parseInt/Number (arredondamento nos empates) e mensagens de erro. Os nomes não colidiam com `bigint_bun` nem `bigint`.
Não foi rodado `cargo`; o primeiro `cargo test bigint_edge_bun_golden` do integrador decide.

- **Bordas de String.prototype e wrappers** (`scripts/gen-string-method-edge-golden.js`,
  `tests/golden/string_method_edge_bun.tsv`, `tests/string_method_edge_bun_golden.rs`, 513 programas): índices
  NaN/negativos/Infinity/fracionários/-0 em substring/substr/slice/at/charAt/charCodeAt/codePointAt, indexOf/lastIndexOf
  com posição estranha, split com limite 0/2**32, repeat com contagem de borda e RangeError (sem alocar), padStart/padEnd
  com fill vazio, concat com objetos, trim com todos os whitespaces Unicode (U+180E, U+FEFF, U+2028, NBSP), localeCompare
  e normalize, fromCharCode/fromCodePoint com erros, template literal e String.raw, startsWith/endsWith/includes com
  RegExp (TypeError) e Symbol.match, métodos HTML com aspas, toString/valueOf de wrappers (com receptor errado),
  comparação com surrogates e número para string em radix, toFixed/toExponential/toPrecision. Medido com
  `vm.runInThisContext` capturando `globalThis.R`. A matriz tem 3586 programas; o gerador amostra 1 a cada 7
  (`STRIDE`), e `STRIDE=1` amplia a cobertura. Não foi rodado `cargo`; o primeiro
  `cargo test string_method_edge_bun_golden` do integrador decide.

- **Chamadas e argumentos de borda** (`scripts/gen-call-edge-golden.js`, `tests/golden/call_edge_bun.tsv`,
  `tests/call_edge_bun_golden.rs`, 1120 programas): `arguments` (mapeado vs não mapeado em cinco formas de parâmetro,
  `length`, `callee`, `Symbol.iterator`, reatribuição, arrow e herdado, congelado/selado), rest params, default params
  (TDZ entre parâmetros, escopo próprio com `var` homônimo, `eval`), `apply`/`Reflect.apply` com array-like gigante
  (limite de `RangeError`), `call`/`bind` com `this` primitivo em sloppy vs strict (boxing), `new` com retorno
  primitivo ou objeto (função, classe, classe derivada), spread e destructuring de iteráveis customizados (iterador que
  lança, `return()` chamado ou não), tail call e recursão de 100 a 1000000 níveis (estrito vs sloppy), estouro de pilha,
  `super` em getter/setter/atribuição composta, `Function.prototype.toString` e `name` de métodos computados, e `bind`
  com `new`, `length` e `name`. Medido com `vm.runInThisContext` (JSC puro). O teste roda numa thread de 256 MiB com
  `VM::set_thread_stack_budget`. Não foi rodado no `cargo`; o primeiro `cargo test call_edge_bun_golden` do integrador
  decide (as recursões de 1000000 níveis e os `apply` de 1e6 elementos são os candidatos a lentidão ou pânico).
- **Protocolo de geradores e iteradores nativos** (`scripts/gen-iterator-protocol-golden.js`,
  `tests/golden/iterator_protocol_bun.tsv`, `tests/iterator_protocol_bun_golden.rs`, 446 programas): `Generator.prototype`
  `next/return/throw` em todos os estados e mensagens de TypeError, `yield*` com iteradores sem `return`/`throw`,
  `AsyncGenerator` com fila de pedidos, helpers de `Iterator.prototype` (map/filter/take/drop/flatMap/reduce/some/every/find)
  com iteradores que lançam e `return()` observado, ArrayIterator/StringIterator/MapIterator/SetIterator/
  RegExpStringIterator (toString, toStringTag, cadeia de protótipos, `next` com this errado),
  `%IteratorPrototype%[Symbol.iterator]` e fechamento de iterador em destructuring, for-of, spread e consumidores
  nativos. Medido com `vm.runInThisContext`. A matriz tem 1781 programas; o gerador amostra 1 a cada 4 (`STRIDE`), e
  `STRIDE=1` amplia. Não foi rodado no `cargo`. Atenção: o caso `order` do AsyncGenerator usa `setTimeout`; se o runtime
  ainda não tiver timers, esse programa falha por isso e não por semântica de iterador.
- **Descritores de propriedades dos builtins** (`scripts/gen-builtin-descriptor-golden.js`,
  `tests/golden/builtin_descriptor_bun.tsv`, `tests/builtin_descriptor_bun_golden.rs`, 501 programas, ~3,2 MB porque
  cada programa leva o prelúdio): complementa `own_keys_bun.json` com uma linha por objeto e visão (`full`: descritor de
  cada chave de `Reflect.ownKeys` com data/accessor, `writable`/`enumerable`/`configurable`, `length`/`name` das funções,
  getter/setter e valores primitivos; `meta`: protótipo, `Symbol.toStringTag`, extensibilidade, contagens; `flags`:
  contagem de combinações de atributos; `plus`: `length`/`name`/`prototype`/`constructor` dos construtores) para
  construtores, protótipos, namespaces (Math, JSON, Reflect, Atomics, Intl, WebAssembly, Temporal) e intrínsecos
  (iteradores, geradores, TypedArray, ThrowTypeError). Sem APIs de host. O bun 1.4.2 tem Temporal, ShadowRealm,
  DisposableStack, Float16Array e afins, então a ausência deles no porte aparece como divergência; só `Proxy.prototype`
  é `absent` no golden. Medido com
  `vm.runInThisContext`. Não foi rodado no `cargo`; espere muitas divergências na primeira rodada, pois o teste é um
  mapa das lacunas de atributos, não de conjunto de chaves.
- **Template literals e tagged templates** (`scripts/gen-template-edge-golden.js`, `tests/golden/template_edge_bun.tsv`,
  `tests/template_edge_bun_golden.rs`, 671 programas): cache do template object por site (função chamada várias vezes,
  laço, closure, classe, `new Function`, eval direto e indireto), cooked `undefined` vs raw com escapes inválidos
  (`\u`, `\x`, octais, `\8`, `\9`) em tagged e SyntaxError em untagged, congelamento e descritores do array e do raw,
  tag como membro/chamada/`new`/`super`/optional chain (SyntaxError), `this` do método tagged, ordem de avaliação das
  substituições, `toString`/`valueOf`/`Symbol.toPrimitive`, CRLF/CR/LS/PS (normalização no raw e continuação de linha),
  aninhamento, `String.raw` com objetos raw exóticos (array-like, Proxy, getters, `length` esquisito) e as sequências
  de escape de crase e `${`. Medido com `vm.runInThisContext`. Não foi rodado no `cargo`; o primeiro
  `cargo test template_edge_bun_golden` do integrador decide. Atenção: os programas com `await`/`then` gravam `R` só
  depois das microtarefas, e o caso de `String.raw` com `length` de 2**53 pode ser lento.

## Geradores síncronos (generator_bun) e módulos ES

Módulos ES já tinham três goldens (`module`, `module_more`, `module_edge`, 2825 casos) sobre `api::module::evaluate_module_map`
(carregador em memória, o gerador monta diretório temporário e roda o bun), então não foi criado um quarto. Faltava
profundidade em geradores: `scripts/gen-generator-golden.js` gera `tests/golden/generator_bun.tsv` (1355 programas, bun 1.4.2)
e `tests/generator_bun_golden.rs` o confere (piso de 600). Famílias: 10 corpos (try/catch/finally, yield no finally, return
e throw no finally, aninhados, laços) x 155 sequências de next/return/throw; `yield*` contra 15 iteradores manuais (sem
return, return não objeto, throw ausente, getters de done/value) x 13 sequências externas; reentrância ("already running"),
estado terminal, protótipos e descritores de GeneratorFunction, construção, `this`/`arguments`/`new.target`; consumidores
(spread, destructuring com fechamento, for-of, Array.from, helpers de Iterator) sobre 4 geradores; erros de sintaxe de `yield`
em 5 contextos. Erros são impressos como `Nome: mensagem` (nunca `stack`). Não foi rodado cargo; o primeiro
`cargo test generator_bun_golden` do integrador decide.
