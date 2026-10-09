# Auditoria de instruções e sintaxe contra o bun

Golden: `tests/golden/statements_bun.tsv` (3757 programas, 229 deles com `TOP:`, ou seja, o script inteiro lança no bun),
gerado por `scripts/gen-statements-golden.js` no bun 1.4.2 (cerca de 2 minutos). Teste:
`tests/statements_bun_golden.rs` (`statements_match_bun`), mesmo padrão de `function_error_bun_golden.rs`.

## Famílias

labels aninhados (loop externo x salto x posição), switch (posição do default x discriminante x break), try/catch/finally
(5 ações x 5 ações x 5 ações dentro de função com laço), for-in com mutação, for-of com iterator.return (5 formas de
`return` x 6 saídas), closures por iteração, vírgula e ternário, optional chaining (25 formas x 13 bases), `??=`/`||=`/`&&=`
(6 alvos x 10 valores x 3 operadores), `**`, templates (tagged, cache do strings array, raw, escapes inválidos), destructuring,
spread, getters/setters/`__proto__`/shorthand em literais, números (separadores, octais legados, sloppy e strict), ASI,
escapes Unicode em identificadores, `let`/`async`/`await`/`yield` como identificador, classes (campos, privados, static
blocks), HTML-like comments, hashbang e 200+ erros de sintaxe antecipados com mensagem exata (via `(0, eval)`).

Não repete bigint_bun.tsv nem coercion_bun.tsv (BigInt e coerção ficam lá; aqui só aparecem `**` com BigInt e `0n`).

## Convenções do golden

- O programa roda no bun como Script (`vm.runInThisContext`), sloppy. `R` é lido depois de 20 ms (microtarefas e timers).
- Resultado `TOP:` no TSV: o script lançou no topo (SyntaxError antecipado do corpo inteiro, por exemplo). O teste Rust
  exige apenas que o programa lance; a mensagem desses casos fica no TSV para quem quiser compará-la depois.
- 40 programas sem `R` foram descartados (ex.: `import(...)` dinâmico no vm do bun lança de outro modo).

## Estado

Não rodei cargo (regra desta tarefa), então ainda não há lista de falhas nem correção no parser/bytecompiler. Próximo passo:
rodar `cargo test --test statements_bun_golden`, agrupar as divergências por família e corrigir, começando por
SyntaxError antecipado (o parser já bate 211 de 211 mensagens no golden dedicado, então as divergências prováveis estão
em campo de classe/ASI/`let` contextual) e depois por try/finally sobreposto e iterator.return no bytecompiler.
