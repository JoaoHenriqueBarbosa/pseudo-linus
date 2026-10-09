# Auditoria de Function.prototype e JSBoundFunction (2026-10-08)

## Golden novo

- `scripts/gen-function-proto-golden.js` mede no bun 1.4.2 e gera `tests/golden/function_proto_bun.tsv` (3011 programas, 10 descartados por não terminarem no bun).
- `tests/function_proto_bun_golden.rs` segue o padrão de `function_error_bun_golden.rs` (arquivo `function_proto_case.js`, variável global `R`). Ainda NÃO foi executado: a regra da tarefa proibia cargo.
- Cobertura: toString de todas as categorias (função, método, getter/setter, classe, arrow, async, generator, bound, nativa, símbolo, nome computado, Proxy, `new Function`, e todas as builtins enumeradas no bun), bind (name, length parcial, length gigante/negativo/Infinity/NaN/BigInt, new de bound, instanceof, bind de bound, Proxy como alvo com ordem das armadilhas), call/apply com array-like, Reflect.apply/construct, `Function.prototype[Symbol.hasInstance]`, name/length (descritores, redefinição, inferência de nome), caller/arguments (strict, sloppy, bound, arrow, classe, generator, async, `arguments.callee`), `Function.prototype` chamável, `new.target` com newTarget diferente e protótipo derivado do newTarget (inclusive função bound sem `prototype`), instanceof com mensagens de erro, recursão profunda com e sem try.

## Achado no bun (relevante para o porte)

- O bun faz chamada em posição de cauda própria (proper tail calls) também em código sloppy no caso `return f.call(this)`, `return f.apply(...)`, `return Reflect.apply(...)` e `return f(a, a)`: a recursão infinita nesses casos NÃO estoura a pilha, fica em laço e não termina. Esses 10 programas foram descartados do golden. A recursão com `f(n - 1)` de profundidade 1e7 em strict passa no bun sem RangeError (tail call), e o golden registra o resultado.
- A mensagem de estouro de pilha medida é `RangeError: Maximum call stack size exceeded.` (com o ponto final). Quem comparar deve usar o texto do TSV.

## Leitura do porte contra o upstream

- `src/runtime/function_prototype.rs` contra `upstream/JavaScriptCore/runtime/FunctionPrototype.cpp`: `functionProtoFuncToString`, `functionProtoFuncSymbolHasInstance`, `functionProtoFuncBind` (incluindo o ramo `canAssumeNameAndLengthAreOriginal`, a mensagem `|this| is not a function inside Function.prototype.bind` e a ordem hasOwnProperty, get length, get name), `callFunctionPrototype`, ordem e atributos de `addFunctionProperties` conferem linha a linha. Nenhuma divergência óbvia.
- `src/runtime/js_bound_function.rs` contra `JSBoundFunction.cpp`: `nameSlow`, `lengthSlow`, `canConstructSlow`, `boundFunctionCall`, `boundFunctionConstruct` (troca de `newTarget` quando é o próprio callee), `customHasInstance` e `getBoundFunctionStructure` conferem. As divergências já estão documentadas no cabeçalho do módulo (taint, `StructureCache`, `boundThisNoArgsFunctionCall`).
- Nenhuma edição em `src/` nesta rodada: sem rodar o teste não há divergência comprovada, e a leitura não achou nenhuma. O próximo passo é rodar `function_proto_bun_golden` e atacar as falhas pela lista que o teste imprime.
