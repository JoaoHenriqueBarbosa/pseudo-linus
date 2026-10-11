# Auditoria do `delete` de identificador

Medido no bun 1.4.2 (`tests/golden/delete_bun.tsv`): `delete x` de `var` criado por `eval` sloppy devolve true e
remove; `var` de função, parâmetro, `let`, `const`, função e `arguments` devolvem false; `with` e global devolvem true.

## O que existia

- Emissor: `DeleteResolveNode` em `src/bytecompiler/nodes_codegen_cpp3b.rs` (local vira `false`; o resto emite
  `resolve_scope` + `op_del_by_id`). Bate com o upstream.
- Slow path: `slow_path_del_by_id` em `src/llint/slow_paths_object.rs`, despachado em `src/llint/dispatch_ext.rs`.
  (O op se chama `op_del_by_id`, não `delete_by_id`.)
- `slow_path_resolve_scope` já devolve o escopo como célula.

## A lacuna

Com base de escopo, `slow_path_del_by_id` caía em `object_for_delete` e dava `Unported` ("base que é escopo").
Não havia `deleteProperty` por variante de escopo.

## O que foi feito

- `JSScopeRef::delete_property` (`src/runtime/js_scope.rs`): chave na `SymbolTable` (lexical, global lexical,
  global object, módulo) devolve false (`JSSymbolTableObject::deleteProperty`); `StrictEvalActivation` devolve
  false; `with` apaga do objeto embrulhado; o resto é `JSObject::delete_property` (propriedade comum, true).
- `slow_path_del_by_id`: se a base é célula de escopo, usa `JSScopeRef::delete_property` e `finish_delete`
  (TypeError em modo estrito quando false).

## Pendente de verificação (nada foi compilado nem testado)

- Rodar `tests/delete_bun_golden.rs`.
- Conferir se o `var` de `eval` sloppy no escopo de função cai em propriedade comum do escopo (e não na tabela).
- `jsDeleteByIdOnScope` do upstream para `with` pode divergir (aqui segue a especificação: apaga do objeto).

# Auditoria: `delete identificador` em strict mode (tests/delete_bun_golden.rs)

Sem edição de código nesta rodada: a checagem do parser existe e está fiel ao C++.

## O que foi conferido

- `src/parser/parser_cpp8.rs:803` (`parse_unary_expression`, caso `DELETETOKEN`):
  `fail_if_true_if_strict_hooked!(context.is_resolve(&expr), "Cannot delete unqualified property '", last_identifier, "' in strict mode")`.
  Igual ao C++. `strict_mode()` vem do escopo corrente, e os goldens de eval strict
  (`eval_bun.tsv:25`, `annexb_bun.tsv:2047-2049`, `sloppy_syntax_bun.tsv:620-622`) com a mesma
  mensagem passam, então a checagem dispara.
- `(x)` e `((x))`: o `ASTBuilder::is_resolve` (`ast_builder_part2.rs:159`) olha `Expression::Resolve`, e
  parênteses não criam nó próprio (como no C++), então `delete (x)` também falha. Os mesmos goldens cobrem.
- Corpo de classe, `new Function` e eval com diretiva: nenhum caminho próprio, todos passam por `parse_unary_expression`.

## Diagnóstico provável (não confirmado, não rodei nada)

O resultado `undefined: undefined` é `e.name + ": " + e.message` com `e` sendo `globalThis`
(que não tem `name` nem `message`), e não "nenhum SyntaxError". O SyntaxError é lançado; o `catch (e)`
lê o registrador errado.

O que distingue os 8 casos dos goldens que passam: todos têm a forma
`try { globalThis.R = <expressão que lança> } catch (e) { globalThis.R = e.name ... }`, ou seja,
atribuição a propriedade (`AssignDotNode`) cujo RHS lança, com o `globalThis` guardado em temporário.
Os goldens que passam usam `R = ...` simples ou `String(...)` em volta.

Hipótese: o registrador do `e` no handler (`op_catch` / alocação do binding do catch) coincide com o
temporário da base (`globalThis`) de `AssignDotNode::emit_bytecode`
(`src/bytecompiler/nodes_codegen_cpp4b.rs:438`), por liberação ou reuso de temporário diferente do C++ ao
sair da expressão por exceção, ou o `op_catch` escrevendo o valor num registrador que não é o do `e`.

## Próximo passo

Reduzir para `try { globalThis.R = (function(){ throw new Error('x') })() } catch (e) { globalThis.R = e.name }`
e comparar o dump de bytecode (registrador do `op_catch` contra o da base do `put_by_id`) com o do C++.
Não é problema do parser.
