# Triagem da rodada 26 (/tmp/zjsc10-run26.txt)

Falhas até o ponto lido: lib, accessor, array_edge, array_exotic, base64_globals, bigint_symbol, builtin_own_keys,
call_spread_varargs, collections, completion_order, dataview, date_edge, date_pattern.

## G1. Artefato do transpilador do bun: `'abc'.length = 1` vira SyntaxError no oráculo

- Testes: accessor_bun_golden (2 de 2999), collections_bun_golden (1 de 2268 no grupo; o outro é G9).
- Exemplos: `T(()=>('abc'.length=1))` e `T(()=>{const s='abc';s.length=1})` em modo estrito. Esperado `<undefined>`
  (o `globalThis.R` nunca foi atribuído), obtido `T:TypeError: Attempted to assign to readonly property.`
- Medido: `bun a.js` com `const s="abc"; s.length=1` imprime `SyntaxError: Left side of assignment is not a reference.`
  (o bun dobra `'abc'.length` em constante `3`). Com `let t=String("abc"); t.length=1` o bun lança o mesmo TypeError que nós.
- Causa: o programa do golden nem executa no bun; o nosso comportamento (`put_to_primitive`,
  `src/llint/slow_paths_object.rs`) está certo. Correção é no gerador do golden (usar `String("abc")`/valor não constante),
  não no motor. Não mexido (golden proibido nesta rodada).

## G2. Atribuição a getter sem setter em objeto nativo não lança no modo estrito

- Testes: dataview_bun_golden (2 de 9861).
- Exemplos: `'use strict'; new DataView(new ArrayBuffer(1)).byteOffset = 1` e `.buffer = 1`.
  Esperado `throw TypeError: Attempted to assign to readonly property.`, obtido `undefined`.
- Verificado no bun: lança nos dois, e também em `{get x(){}}.x = 2`.
- Hipótese: o put em acessor nativo (getter de protótipo sem setter) não propaga o `should_throw` do modo estrito; o `is_strict_mode`
  do `PutPropertySlot` chega falso quando a base é a expressão `new DataView(...)` direta (sem registrador nomeado), ou o ramo
  de `GetterSetter` sem setter devolve `Ok(false)` sem converter em TypeError. Olhar `put_on_object` / `put_by_id`
  em `src/llint/slow_paths_object.rs` e o acessor de `DataView.prototype` em `src/runtime/` (dataview*). Os 2997 programas de
  acessor comuns do accessor_bun_golden passam, então é específico do getter nativo.

## G3. Mensagem de erro de `length` não gravável do Array

- Testes: array_exotic_bun_golden (2 de 9172).
- Exemplo: `Object.defineProperty(Array.prototype,'length',{value:4,writable:false}); Array.prototype.pop()`.
  Esperado `throw TypeError: Array length is not writable`, obtido `... Attempted to assign to readonly property.`
- Verificado no bun: `Array length is not writable`.
- Causa provável: `src/runtime/array_prototype.rs:327` e `:382` usam a mensagem genérica; a mensagem de `js_array.rs:863`
  (`Array length is not writable`) é a que o JSC dá no `setLength` de `pop`/`push` (ArrayPrototype.cpp `setLength` com
  `ReadonlyPropertyWriteError` só para `shift/unshift/splice` em objeto genérico). Conferir qual das duas chamadas é o `pop`.

## G4. `DOMException` não é global; `atob` lança com `line`/`column` do chamador

- Testes: base64_globals_bun_golden (10 de 183).
- Exemplo: `atob('!')` em try, `e.code === DOMException.INVALID_CHARACTER_ERR`: obtido
  `ReferenceError|DOMException is not defined`. Esperado `true`/`InvalidCharacterError`, código 5.
- Segundo tipo: `Reflect.apply(atob,null,['!'])` o `line`/`column` esperados são 4/20 (posição da chamada no script) e vieram 1/11
  (posição relativa ao fragmento, provável deslocamento do prelúdio do programa).
- Causa: falta o global `DOMException` (construtor, protótipo com getters `code/name/message`, constantes `INVALID_CHARACTER_ERR`...).
  É um objeto novo a portar; o `line/column` é cálculo de posição de erro de `atob` nativo (stack frame do chamador).

## G5. Descrição de Symbol com surrogate solitário: artefato do golden

- Testes: bigint_symbol_bun_golden (2 de 14227).
- Exemplo: `T(()=>Symbol('\ud800'))` e `...[Symbol.toPrimitive]('number')`. Esperado `Symbol(U+FFFD)`, obtido `Symbol(\ud800)`.
- Medido no bun: `Symbol("\ud800").toString().charCodeAt(7) === 55296`, o bun mantém o surrogate. O golden perdeu a unidade na
  serialização UTF-8 do gerador. Nosso motor está certo; o gerador do golden deve emitir via `JSON.stringify` (escapa
  surrogate solitário). Não mexido.

## G6. Ordem de `Object.getOwnPropertyNames(globalThis)`

- Testes: builtin_own_keys_golden.
- Exemplo: `globalThis ordem: na posição 0 esperado Infinity, obtido undefined`. Esperado: `Infinity, undefined, NaN, isNaN, isFinite,
  escape, unescape, decodeURI, ..., eval, globalThis, parseInt, parseFloat, ArrayBuffer, EvalError, ...`.
- Causa: ordem de instalação das propriedades em `JSGlobalObject::init` / `finishCreation` (Infinity primeiro, depois undefined, NaN).
  Arquivo provável: `src/runtime/js_global_object*.rs` (lista de `putDirect` do `JSGlobalObject`). Ver o diff de ordem completo
  no log (linha ~1673).

## G7. Nome de function expression sombreado por parâmetro de arrow filha

- Testes: call_spread_varargs (`named_function_expression_name_used_by_child_only`).
- Exemplo: `var f=function g(){return (g)=>g;};f()(5)`. Esperado `5`. Verificado no bun: `5`.
- Hipótese: a arrow filha tem parâmetro `g` que sombreia o nome do function expression; o scope analysis (Parser
  `ScopeRef`/capture do nome do FunctionExpression, `usedVariables`) marca `g` da arrow como captura do nome externo e o bytecompiler
  resolve a função em vez do parâmetro. Olhar `src/parser/` (declareParameter/`closedVariableCandidates`) e o `emit_resolve` do
  nome da função em `bytecompiler`.

## G8. `var globalThis = 1; typeof globalThis` no eval indireto

- Testes: completion_order_bun_golden (1 de 5912).
- Exemplo: `E("var globalThis=1;typeof globalThis")` (eval indireto). Esperado `"number"`; obtido `undefined`.
- Verificado no bun: `(0,eval)("var globalThis=1;typeof globalThis")` -> `number`.
- Causa provável: `var` no global de nome já existente (`globalThis`, propriedade configurável do objeto global) e a atribuição
  de inicializador no eval global; o valor de completion do statement final perde o resultado. Olhar `eval` global declaration
  instantiation (`GlobalVariableAccess`/`JSGlobalObject::hasProperty` da declaração `var`) e o completion value do eval.

## G9. Intl.DateTimeFormat com locales não latinos/CLDR

- Testes: date_edge_bun_golden (12 de 4704), date_pattern_bun_golden (339 de 4408).
- Exemplos:
  - `fr`, `fr-CA`, `hi`: mês longo no pattern com `month: 'long'`; esperado `novembre`/`नवंबर`, obtido abreviado `nov.`/`नव॰`
    (a largura do mês se perde quando há `weekday` junto).
  - `zh`, `zh-HK`, `ko`: mês numérico no lugar do texto (`三月` esperado, `3` obtido; `3월` vs `3`).
  - `ru`: ordem e pontuação do pattern (`вторник, 5 марта 2024 г.` esperado, `вторник, марта 5, 2024` obtido).
  - `en-AU`: `weekday=T` esperado (narrow), obtido `Tu.`; `pt-PT` `month=M` (narrow) obtido `03`.
  - `zh-TW`: dayPeriod `清晨` antes da hora e `timeZoneName=世界標準時間` esperado, obtidos `上午` depois e `Coordinated Universal Time`.
  - date_edge: `Nov` esperado, `Nov.` obtido; ar com fuso `غرينتش+١٣` esperado, `GMT+13` obtido.
- Causa: padrões CLDR por locale ausentes ou usando o fallback `en` (dados de `Intl.DateTimeFormat` em `src/runtime/intl_date_time_format/`).
  É trabalho de dados por locale, não de lógica; 159 das 339 são as linhas `latn` (numeração) do mesmo desvio.

## G10. lib: intervalo ISO sem ano

- Teste: `runtime::intl_date_time_format::range::tests::iso_calendar_without_a_month_name_is_a_single_shared_value`.
- Exemplo: campos mês longo + dia, calendário ISO, anos diferentes. Esperado `" 14 (U+2013) 13"` (com o espaço do mês invisível, sem anos),
  obtido `"2023  14 (U+2013) 2024  13"`: o ano vazava mesmo sem campo de ano.
- Causa: `iso_interval_template` (`range.rs`), no nível `Level::Year`, devolvia `y M d` sem checar `year`.
- CORRIGIDO nesta rodada: nível Year sem o campo de ano usa `M d` nos dois lados.

## Status

- Corrigido: G10.
- Artefatos do oráculo/golden (nada a mudar no motor): G1, G5.
- Bugs do motor com hipótese: G2, G3, G7, G8 (pequenos), G6, G4, G9 (maiores).
