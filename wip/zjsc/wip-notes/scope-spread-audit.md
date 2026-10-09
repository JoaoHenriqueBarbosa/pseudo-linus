# Auditoria das divergências de escopo contra o bun (53 de 1920)

Origem: `/tmp/now2_scope_bun_golden.txt`. Ignoradas as de `setTimeout`.

## Corrigido no código (não compilado, não testado)

1. `with` sobre Proxy, `x = 2` e `x++`: faltava o segundo `has` antes do `set`.
   Causa: o `slow_path_put_to_scope` vivo (despachado por `src/llint/dispatch.rs`) é o de
   `src/llint/slow_paths.rs`, que só chamava `scope.has_property` no modo estrito com
   `ThrowIfNotFound`. O C++ chama `scope->hasProperty` sempre (o `slow_paths_object.rs` já tinha
   a versão fiel, mas não está ligado ao dispatch). Agora `has_property` roda sempre depois da
   consulta à SymbolTable, com checagem de exceção pendente.

2. Nome de função (NFE) sloppy lido como `undefined` (`eval`, arrow, `with`, `function*`, `async`):
   Causa: `emitPushFunctionNameScope` grava o callee com `put_to_scope` `ClosureVar` /
   `NotInitialization`. No LLInt isso é escrita direta no `offset` (`.pClosureVar`), mas o slow path
   do porte só tratava `ResolvedClosureVar` de forma direta; `ClosureVar` caía em `symbol_table_put`,
   que respeita o `ReadOnly` da entrada (variável sem escrita em sloppy) e descartava a escrita
   inicial, deixando o slot vazio. Agora `ClosureVar` e `ClosureVarWithVarInjectionChecks` em
   `LexicalEnvironment` gravam direto em `slow_paths.rs`.

## Não resolvido (hipóteses)

3. `delete x` dentro de `with` sobre Proxy não chama `deleteProperty`. O caminho
   `slow_path_del_by_id` -> `JSScopeRef::delete_property` -> `JSObject::delete_property` ->
   `delete_from_proxy` parece correto na leitura; falta rodar com traço para ver se o `op_del_by_id`
   chega com base escopo ou com o Proxy, e se o handler vivo é mesmo o de `dispatch_ext.rs`.

4. `globalThis.hasOwnProperty is not a function` (todos os `var` com identificador unicode/escape,
   mesmo `var ñ`): `js_global_object_init.rs` faz `set_prototype_direct(object_prototype)`, então o
   global do teste (`evaluate_script_sequence_result`) provavelmente não passa por esse init, ou o
   protótipo é trocado depois (global proxy). Conferir `globalThis.__proto__ === Object.prototype`
   no harness antes de mexer em qualquer coisa. Essa família sozinha é cerca de 30 dos 53 casos.

5. `var NaN = 1` / `var Infinity = 1` dão 1: as globais são `ReadOnly | DontDelete`, então a
   escrita deveria ser ignorada em sloppy. Suspeita: o `var` entra na SymbolTable do global
   (`addVar`) mesmo com a propriedade já existindo, virando `GlobalVar` gravável. Conferir se
   `initialize_global_properties` pula `add_var` quando `has_own_property` é verdadeiro.

### Releitura linha a linha dos itens 3 e 5 (2026-10-08, sem compilar, nenhuma correção provada)

Item 3 (`delete x` em `with` sobre Proxy). Conferido contra o C++ e igual, sem divergência achada:
- `DeleteResolveNode::emit_bytecode` (`nodes_codegen_cpp3b.rs`) é o de `NodesCodegen.cpp:2752`
  (`variable`, `emitLoad(false)` se local, `emitResolveScope(dst)`, `emitDeleteById(finalDestination(dst, base), base)`).
- `make_delete_node` do ASTBuilder gera `DeleteResolve` para `Resolve`; `emit_delete_by_id` emite `OpDelById`
  com `ecma_mode`; o único handler vivo é `dispatch_ext.rs:252` -> `slow_path_del_by_id`.
- Divergência de desenho (não de comportamento): o C++ devolve o objeto do `with` em `resolve_scope` e o
  `del_by_id` faz `JSCell::deleteProperty(baseObject)`; o porte devolve o `JSWithScope` e
  `slow_path_del_by_id` chama `JSScopeRef::delete_property`, que faz `with_scope.object().delete_property`
  e este vai para `delete_from_proxy` -> `ProxyObject::perform_delete` (trap `deleteProperty`).
  A cadeia inteira confere na leitura e o `has:x,get:sym` do esperado aparece (resolve roda), então a
  perda está entre `slow_path_del_by_id` e o trap, ou no valor do registrador `base`.
- Próximo passo obrigatório (precisa rodar): `eprintln!` temporário no topo de `slow_path_del_by_id` com
  `f.get(op.base)` e o resultado de `JSScope::from_cell_id`, e a mesma sonda em `JSScopeRef::delete_property`.
  Se `from_cell_id` der `None`, a base não é o escopo (caindo em `object_for_delete` sobre a célula do
  `with`, que apaga do próprio escopo sem trap), e o defeito está em `emit_resolve_scope`/registrador `dst`.

Item 5 (`var NaN = 1`). Conferido contra o C++ e igual:
- `create_global_var_binding` (`js_global_object.rs:1036`) é o `createGlobalVarBinding` do inline: sai
  cedo se `own_property_attributes` acha a propriedade. `NaN`/`Infinity`/`undefined` entram em
  `js_global_object_init.rs` com `DONT_ENUM | DONT_DELETE | READ_ONLY` e `reorder_standard_globals`
  preserva os atributos, então `add_symbol_table_entry` não roda para elas.
- `initialization_mode_for_assignment_context` (`DeclarationStatement` -> `Initialization`) e o laço de
  `ProgramExecutable::initialize_global_properties` (`program_executable.rs:296-362`) conferem.
- O put de `var NaN = 1` chega em `slow_paths.rs` `slow_path_put_to_scope` -> `symbol_table_put` (NotFound)
  -> `JSScopeRef::put` -> `JSObject::put` -> `put_direct_internal`, que devolve `READONLY_PROPERTY_CHANGE_ERROR`
  para atributo `READ_ONLY` nos dois ramos (dicionário e estrutura). Nada na leitura grava o valor.
- Atenção ao medir no bun: arquivo `.js` rodado por `bun arq.js` embrulha o CJS numa função, então
  `var NaN = 1` lê 1 lá também. Para o oráculo usar `bun -e` não serve pelo mesmo motivo; usar
  `(0, eval)("var NaN = 1; NaN")` (eval indireto, escopo global) e comparar com o porte pelo mesmo caminho.
- Próximo passo (precisa rodar): imprimir `own_property_attributes(NaN)` dentro de `create_global_var_binding`
  e os atributos da entrada da `SymbolTable` do global depois do `var`. Se a entrada existir, o defeito é
  `own_property_attributes` não ver `NaN` (ex.: `get_own_property_slot` com `target_of`/`GlobalProxy`).

6. TDZ com nome vazio (`class C { static s = D }` e `class K { a = b }`): esperado
   `Cannot access '' before initialization.`. A mensagem vem do nome do identificador; em campo de
   classe o bun usa o trecho de fonte do ExpressionInfo da função sintética de inicialização, que
   não tem identificador. Ver `create_tdz_error_from_source_range` e o `emit_throw_tdz` dos
   inicializadores de campo (nome vazio quando o escopo é o `fieldInitializer`).

7. Outras isoladas (um caso cada): `static name() {}` com `typeof K.name` 'function';
   mensagens de erro de campo privado (`near '...(function () { })...'`, `evaluating 'super(...args)'`);
   `({ __proto__: 1, __proto__ })` deve dar SyntaxError (duplicado) em `eval` indireto.

## Segunda passada (não compilado, não testado)

8. CORRIGIDO (item 4): `globalThis.hasOwnProperty is not a function`. Causa: `globalThis` é o `JSGlobalProxy`,
   criado em `JSGlobalObject::finish_creation` com `create_structure(.., this.get_prototype_direct())`, quando o
   global ainda tinha o `Function.prototype` do `create`. O `js_global_object_init.rs` depois trocava só o
   protótipo do global para `Object.prototype`; o proxy ficava com o protótipo velho (Function.prototype não
   tem `hasOwnProperty`). Os casos `globalThis.R = ...` passavam porque só escrevem, não herdam. Agora o init
   também faz `global_this.set_prototype_direct(object_prototype)` logo após o do global (o que
   `JSGlobalProxy::setTarget` faria no C++). Conferir depois de compilar: `globalThis.__proto__ ===
   Object.prototype` e `Object.getPrototypeOf(globalThis) === Object.getPrototypeOf(Object.getPrototypeOf(globalThis))`
   não deve ser lido; basta o primeiro. Ponto aberto: a `Structure` do proxy foi criada com o protótipo velho
   (`create_structure`); `set_prototype_direct` em objeto mono-proto deve atualizar o campo da estrutura, confirmar.

9. NÃO ACHADO (item 5, `var NaN = 1`): lido o caminho inteiro e está fiel ao C++:
   `ProgramExecutable::initializeGlobalProperties` -> `create_global_var_binding` retorna cedo se
   `own_property_attributes` existe; `slow_path_put_to_scope` (`slow_paths.rs`) -> `scope.symbol_table_put`
   (NotFound) -> `JSScopeRef::put` -> `JSObject::put`. A suspeita restante é `JSObject::put` ->
   `can_perform_fast_put_inline_excluding_proto`: se a `Structure` viva do global perdeu o bit
   `has_read_only_or_getter_setter_properties_excluding_proto` (setado em `put_direct_without_transition`
   só na estrutura do momento; as transições copiam de `previous` na linha 320 de `structure.rs`, mas a
   conversão para dicionário, `BatchedTransitionOptimizer` ou `change_global_proxy_target_transition` podem
   não copiar), o put cai em `put_inline_fast` e grava por cima do `ReadOnly`. Teste mínimo para decidir:
   `Object.getOwnPropertyDescriptor(globalThis,'NaN').writable` e `globalThis.NaN = 1; globalThis.NaN`.
   Se o segundo der 1, é o bit; corrigir onde o bit se perde (copiar na transição para dicionário).

10. NÃO INVESTIGADO por falta de tempo: item 3 (`delete` em `with` sobre Proxy) e item 6 (TDZ com nome vazio em
    campo de classe). Ponto de partida do item 3: confirmar qual `slow_path_del_by_id` o `dispatch_ext.rs`
    chama (há versões em `slow_paths*.rs`) e se o `op_del_by_id` recebe o escopo resolvido ou o `JSWithScope`.

11. Terceira passada (só leitura, nada corrigido, nada compilado):

    Item 2 (`var NaN = 1` / `globalThis.NaN = 1`): a hipótese do bit está DESCARTADA por leitura.
    `Structure::new_from_previous` copia o bit; `to_dictionary_transition`, `change_prototype_transition`,
    `change_global_proxy_target_transition` e `remove_property_transition` passam por ele; `add` e
    `put_direct_without_transition` o ligam; o `BatchedTransitionOptimizer` só converte para dicionário (sem
    flatten). Além disso, mesmo sem o bit, `put_direct_internal` (ramos dicionário e não dicionário) e
    `put_inline_slow` devolvem erro de somente leitura quando a propriedade existe com `READ_ONLY`. O único
    caminho que produz `1` é o `var` achar que NaN não existe: `create_global_var_binding` chama
    `own_property_attributes` (`js_global_object.rs:949`, `get_own_property_slot` do global) e, se der falso,
    `add_symbol_table_entry` cria uma entrada gravável na `SymbolTable` do global, e `slow_path_put_to_scope`
    grava nela via `symbol_table_put` (leitura devolve 1). Sonda para decidir (rodar com cargo): depois da
    `init`, `own_property_attributes(NaN)` e `Object.getOwnPropertyDescriptor(globalThis,'NaN')`. O passo
    suspeito é o reordenamento de `js_global_object_init.rs` (~linha 477: remove e `put_direct` de cada nome da
    lista ORDER, com `Structure::remove_property_transition`/`remove_property_without_transition` seguido de
    `put_direct`): conferir se o NaN sobrevive nele com os atributos (`get_direct_offset_with_attributes`
    devolve `attributes` com `READ_ONLY`?) e se `copy_property_table_for_pinning` (structure.rs ~536, devolve
    tabela VAZIA se a estrutura não materializou a sua) não perde propriedades quando a estrutura anterior tem
    a tabela só na cadeia de `previous`.

    Item 3 (`delete x` em `with(proxy)`): cadeia lida de ponta a ponta e está fiel: `DeleteResolveNode` ->
    `emit_resolve_scope` + `emit_delete_by_id` -> `slow_path_del_by_id` (único, `slow_paths_object.rs:776`,
    dispatch_ext.rs:252) -> `JSScope::from_cell_id` -> `JSScopeRef::delete_property` (ramo `WithScope`) ->
    `JSObject::delete_property` -> `delete_from_proxy` -> `perform_delete` (chama o trap). Nenhum ponto óbvio
    de desvio; precisa de execução: logar em `slow_path_del_by_id` se `f.get(op.base)` casa `JSValue::Cell` e
    se `from_cell_id` devolve `WithScope` (o registrador de escopo é lido por `.scope()` em `scope_operand`,
    mas aqui por `f.get`; se `f.get` não devolver `JSValue::Cell` para esse registrador, cai no ramo
    `object_for_delete`, que apaga do próprio `JSWithScope` sem chamar trap, exatamente o sintoma
    `has:x,get:sym`). Primeiro candidato: trocar a leitura em `slow_path_del_by_id` por `scope_operand(f,
    op.base)` (`.scope()` do registrador), igual a `resolve_scope`/`get_from_scope`/`put_to_scope`.

    Item 4 (TDZ `''` em inicializador de campo): `create_tdz_error_from_source_range` está fiel ao C++
    (`exception_helpers.rs:118`); o `''` do bun é um trecho `[divot - startOffset, divot + endOffset)` vazio,
    e o `ResolveNode::emit_bytecode` (`nodes_codegen_cpp1.rs:366`) emite info com intervalo do identificador
    (`'D'`). Logo, no bun a info que precede o `check_tdz` do inicializador sintetizado de campo tem
    intervalo zero (provável: o `check_tdz` cai numa info emitida em outro ponto do `FunctionNode` sintético,
    ou o `get_range` usa offsets relativos ao início da função sintética). Para o global `let b` lido do campo
    (`class K { a = b }`), o caminho é o `slow_path_get_from_scope` (`slow_paths_object.rs:889`,
    `throw_tdz(&ident)`), que o C++ lança com `createTDZError(globalObject)` sem nome em versões antigas, mas o
    bun mostra `''`: ali a mudança é lançar `Cannot access '' before initialization.` quando o código é de
    inicializador de campo (decidir comparando `op_check_tdz` vs get_from_scope do bun no oráculo).
    Não corrigido por falta de medida no bun.

12. Quarta passada (só leitura, sem cargo, nada alterado no código):

    Item 3 (`delete x` em `with(proxy)`): a hipótese do registrador está DESCARTADA. `slow_path_resolve_scope`
    grava o resultado com `f.set(op.dst, resolved.into_js_value())` (`JSValue::from_cell(cell_id)`), então
    `f.get(op.base)` devolve `JSValue::Cell` e `scope()` (`unboxed_cell`) lê o mesmo id; as duas leituras são
    equivalentes. Diferença estrutural para o C++: lá `objectAtScope` do `with` devolve o objeto do with (o
    `ProxyObject`) e o `del_by_id` chama `deleteProperty` dele direto; aqui `object_at_scope` devolve o próprio
    `JSWithScope` (`js_scope.rs:136`) e o despacho para o proxy depende do ramo `WithScope` de
    `JSScopeRef::delete_property` (`js_scope.rs:366`), que chama `with_scope.object().delete_property`, e este
    cai em `delete_from_proxy` se `type_() == ProxyObjectType`. `DeleteResolveNode` (`nodes_codegen_cpp3b.rs:591`)
    é fiel ao `NodesCodegen.cpp:2752`. Sem desvio visível: precisa de execução. Sonda: `eprintln!` no início de
    `slow_path_del_by_id` com `f.get(op.base)` e o resultado de `JSScope::from_cell_id`, e em
    `JSScopeRef::delete_property` com `with_scope.object().map(|o| o.type_())`. Suspeito remanescente: o
    `object` do `JSWithScope` ser o alvo do proxy (ou um wrapper) e não o `Proxy`, o que faria `type_()` não
    ser `ProxyObjectType` (conferir `JSWithScope::create` e o `toObject` do `with`).

    Item 4 (TDZ `''`): o C++ lança por duas rotas. `slow_path_check_tdz` (`CommonSlowPaths.cpp:326`) usa o
    TEXTO do intervalo `[divot - startOffset, divot + endOffset)` e `createTDZError(globalObject, StringView)`
    (só `'nome'` se o texto não for vazio, senão o mesmo formato com texto vazio); `get_from_scope` global
    lexical (`LLIntSlowPaths.cpp:2373`) usa o `Identifier` (nome real, ou "Cannot access uninitialized
    variable." se vazio). Os casos do golden (`static s = D`, `a = b`) com `D`/`b` declarados DEPOIS caem na rota
    do `check_tdz` (o gerador emite `emit_tdz_check` após o `get_from_scope`, `nodes_codegen_cpp1.rs:395`).
    O `ResolveNode::emitBytecode` do porte é idêntico ao upstream (`NodesCodegen.cpp:282`: info `[start,
    start+len)` do identificador), e o `create_tdz_error_from_source_range` também; logo o porte devolve `'D'`
    corretamente a partir do mesmo fonte. O `''` do bun vem de a info/`getRange` resolver para intervalo vazio
    dentro do inicializador de campo sintetizado (divots dessas funções são relativos/rebaseados no JSC real).
    Nenhum ajuste de range foi feito: sem medir o oráculo (qual `divot`/`startOffset` o bun grava), forçar vazio
    só para estes casos seria atalho. Próximo passo: dumpar `expression_info_for_bytecode_index` do `check_tdz`
    no inicializador de campo e comparar com `bun --print` do bytecode (`BUN_JSC_dumpBytecode`); se o bun
    achar divot relativo ao início da função sintética, o ajuste é em como o `FieldInitializer` codegen
    (`emit_expression_info` dentro de `UnlinkedFunctionExecutable` de campo) soma `m_startOffset`.

## Achado (golden now3, 50 divergências)

Grupo A, 36 casos `var <nome unicode/_/$> = 1; globalThis.hasOwnProperty(...)` dando false (todos os 36 testes de
`hasOwnProperty` do golden, não é questão de unicode): `JSObject::get_own_property_slot` (js_object.rs) não
consultava a `SymbolTable` do global. No C++, `JSGlobalObject::getOwnPropertySlot` faz `Base::getOwnPropertySlot` e depois
`symbolTableGet`; `var`/função do script global só existem na tabela. Só `JSGlobalObject::own_property_attributes` e
`get_own_property_descriptor` faziam isso à mão, e o despacho `own_descriptor`/`has_own_property` usa o `JSObject`.
Correção: ramo `GlobalObjectType` no fim de `get_own_property_slot` (via `JSScope::from_cell_id` e
`JSScopeRef::symbol_table_get`, `slot.set_value(self, attributes, value)`). NÃO compilada nem medida (sem cargo).
Efeito colateral a conferir: `Object.keys(globalThis)`/`getOwnPropertyNames` não passam por aqui.

Grupo B, `var NaN/Infinity = 1` lendo 1: NÃO resolvido. Com a correção A nada muda aqui (NaN está na Structure, então
`create_global_var_binding` não cria entrada). Falta ver `resolve_scope_type`/`ProgramExecutable` para `NaN`: o
compilador provavelmente emite `GlobalVar` (símbolo) ou `GlobalProperty` com put que ignora ReadOnly; no C++
(`JSScope::resolveScopeType`/`BytecodeGenerator::variable`) um global somente leitura do `Structure` vira
`GlobalProperty`, e o `put_to_scope` GlobalProperty respeita READ_ONLY. Conferir `slow_path_put_to_scope` e
`ProgramExecutable::initialize_global_properties`.

Grupo C, NFE (`g = 1; typeof g` dentro de eval, arrow, with, generator, async): nome da função não é regravável em
sloppy (atribuição silenciosamente ignorada); o resolve dentro de eval/arrow/with/generator está gravando na variável
do escopo da NFE. Não investigado (tempo).

Grupo D (não investigados): mensagens de erro (`evaluating '...'` espúrio em `super.x` no eval, `near '...'` em campo
privado, TDZ `''` em class static/campo), `__proto__` duplicado em eval, `with(proxy)` com `R` lido no trap.

## Auditoria do read-only da NFE no gerador (após `slow_path_put_to_scope` gravar direto em `ResolvedClosureVar`)

Conferido contra `upstream/JavaScriptCore/bytecompiler/NodesCodegen.cpp`, sem rodar cargo.

- `emit_read_only_exception_if_needed` (bytecode_generator_cpp5.rs) é igual ao C++ (`isStrict() || isConst()`).
- `Variable` do nome da NFE: `variable_for_local_entry` + `set_is_read_only()` quando `result_is_callee`
  (bytecode_generator_cpp3.rs), igual ao C++; em sloppy com eval a busca vira dinâmica (`from_ident`).
- Os 21 chamadores têm paridade 1 a 1 com os 20+ do C++: os que usam o retorno (AssignResolve, ReadModifyResolve,
  ShortCircuitReadModifyResolve, Postfix/Prefix resolve, bind de resolve do DeconstructionPattern) pulam o put
  (`if (!isReadOnly)` / `threw_exception => return`). `g = 1`, `g++`, `++g`, `g += 1`, `g &&= 1` e `[g] = [1]` /
  `({g} = {g:1})` não gravam.
- Os que o C++ também NÃO pulam (descartam o retorno): `for (g in ...)` e `for (g of ...)` (ForIn/ForOf
  `emitLoopHeader`, NodesCodegen.cpp:4463 e 4625) e o `emitReadModifyAssignment` de `+=` com strcat. No C++ o put
  acontece mesmo assim, é paridade; o `slow_path_put_to_scope` gravando direto faz `for (g of [1])` sobrescrever
  `g` no porte, igual ao JSC real (conferir contra o bun se algum golden divergir).
- Arrow/eval fora do escopo do gerador da função: `variable()` devolve `from_ident`, `resolve_type()` dá
  `Dynamic` (sloppy eval) ou `GlobalProperty*`, nunca `ClosureVar`; vai pelo `symbol_table_put`, que respeita o
  `ReadOnly` e o `should_throw`. Não passa pelo atalho.
- Correção feita: o atalho do slow path cobria também `ClosureVar` e `ClosureVarWithVarInjectionChecks`, mas o
  gerador nunca os emite (o offset só existe no cache do slow path do C++) e `op.offset` seria 0. Agora só
  `ResolvedClosureVar` (src/llint/slow_paths.rs).
- Nenhuma falta de chamador encontrada; nenhuma outra edição no gerador.
