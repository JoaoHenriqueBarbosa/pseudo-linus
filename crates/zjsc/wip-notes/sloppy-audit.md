# Auditoria de semântica sloppy e sintaxe de borda

Golden: `tests/golden/sloppy_syntax_bun.tsv` (3228 programas, medidos no bun 1.4.2), gerado por
`scripts/gen-sloppy-syntax-golden.js`; teste em `tests/sloppy_syntax_bun_golden.rs` (`sloppy_syntax_matches_bun`,
mínimo de 3000 programas). Nenhum teste foi rodado nesta passada (regra da tarefa: sem cargo).

## Como o gerador roda

- Mesmo molde do `gen-annexb-golden.js`: preload com `vm.runInThisContext` (script sloppy, nunca módulo estrito) e o
  resultado na global `R`. Quase todo programa roda o trecho por eval indireto ou `new Function` dentro de try/catch e
  grava o valor ou `Nome: mensagem`, então SyntaxError com mensagem exata entra no golden. Programas de comentário
  HTML também rodam como texto do script.
- Muitos trechos entram em dobro (sloppy e com prefixo `"use strict"`), por isso o total passa de 600.
- Descartado (1): `while (1) { switch (1) { case 1: continue } }`, laço infinito de propósito (estoura o tempo).
  Seis programas ficam com `R` indefinido (erro de sintaxe no próprio script); o zjsc deve deixar `R` indefinido também.

## Áreas cobertas

Funções em blocos, switch, if sem bloco e labels (B.3.3, com parâmetros, `arguments`, catch, let); `with` e
`Symbol.unscopables` (inclusive Proxy com log de `has`/`get`/`set`); octais, `08`/`09`, separadores numéricos,
escapes em string, template e tagged (cooked indefinido, raw), regex legado; comentários HTML; `arguments` mapeado e
não mapeado (default, rest, destructuring, defineProperty, freeze, callee); `this` sloppy vs strict; eval (var,
function, let, bloco, conflito com let, `new.target`, `super`, campos de classe); `new.target`; `__defineGetter__`
e irmãos, `__proto__` em literais (duplicata, shorthand, computado, destructuring); `Function(...)` com comentários,
newlines, injeção de `}`, parâmetros duplicados, geradoras e async; labels, break e continue com try/finally; ASI
(`return\nx`, `a\n++b`, `let`, `yield`, `await`, `async`, classes, `for (let of`); optional chaining com delete, calls,
templates, `?.5:1`, privados; `??=` `||=` `&&=` com getters, setters, with, Proxy, super e privados; exponente,
BigInt (aritmética, conversões, TypedArray); campos e métodos privados, `#x in o`, static blocks, `accessor`
(decorators/accessor aparecem como o bun responde), `super` em getters, setters, campos e estáticos.

## Mensagens conferidas contra src/parser (leitura, sem rodar)

Presentes: `Numeric literals may not begin with 0_`, `Non-number found after exponent indicator`, `Cannot use tagged
templates in an optional chain`, `Cannot call constructor in an optional chain`, `Bare private name can only be used
as the left-hand side of an \`in\` expression`, `Cannot use 'await' within static block`, `'break' cannot cross static
block boundary`, `Attempted to redefine __proto__ property`, `Cannot reference undeclared private names`,
`Parameters should match arguments offered as parameters in Function constructor`, `Can't create duplicate variable
in eval` (em `interpreter/execute_eval.rs`).

## Varredura estática das mensagens do golden (2026-10-08)

Das 217 mensagens únicas de SyntaxError do golden, o grep literal acusa 132 "ausentes", mas quase todas são falso
positivo: o porte monta a mensagem por fragmentos (`semantic_fail_if_true!(..., "a", x, "b")`, a macro
`semantic_failure_due_to_keyword`, linhas quebradas). Conferidas à mão contra `Parser.cpp`: todas as de texto fixo
existem no porte; `Failed to parse String to BigInt` está em `runtime/js_big_int_part6.rs`; as de setter/getter
privado estão em `parser_cpp5.rs:526-531`. Nenhuma divergência de texto achada por leitura.

- `for (let in {})` (bun: `Cannot use the keyword 'in' as a lexical variable name.`): NÃO é divergência. O caminho é
  `parse_for_statement` (`is_let_declaration`) -> `parse_variable_declaration_list` -> `in` não é
  `match_spec_identifier` -> `parse_destructuring_pattern` (caso `_`, `parser_cpp2.rs:924`) ->
  `semantic_failure_due_to_keyword!(destructuring_kind_to_variable_kind_name(kind))`, que gera `Cannot use the keyword
  'in' as a lexical variable name` (`INTOKEN` tem `KEYWORD_TOKEN_FLAG` e não é contextual). Igual ao upstream; só
  confirmar quando o teste rodar.
- Pendente (precisa de execução): triar os `esperado/veio` reais, que é o único jeito de achar divergência de texto
  em mensagens montadas.

## Próximo passo

Rodar `cargo test --test sloppy_syntax_bun_golden` (alguém com permissão de build) e triar os `esperado/veio`.
