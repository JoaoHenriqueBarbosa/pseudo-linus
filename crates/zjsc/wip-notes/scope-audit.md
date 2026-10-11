# Auditoria de escopo e closures

## Golden

- `scripts/gen-scope-golden.js` gera `tests/golden/scope_bun.tsv` (1920 programas, medidos no bun 1.4.2, nenhum
  descartado, sem caminho da máquina). Teste: `tests/scope_bun_golden.rs` (`scope_and_closures_match_bun`, exige
  pelo menos 800 programas, mesmo padrão de `function_error_bun_golden.rs`).
- Cada programa grava `R` dentro de `try/catch` (`Nome: mensagem` quando lança). Os SyntaxError saem por eval
  indireto (escopo global), então a mensagem é a do parser sem caminho. Timeout de 10 s por execução.
- Cobertura: TDZ (let/const/class, parâmetros default com referência cruzada, typeof, loops, switch,
  destructuring), bindings por iteração (for/for-in/for-of, mutação, closures em condição e incremento),
  shadowing de parâmetros e `arguments`, escopo separado dos parâmetros default, named function expression
  (sloppy e strict), binding interno de classe, catch e Annex B, hoisting de function em bloco, `let` em
  label/for-in head, `const` sem inicializador, redeclarações, eval var injection, `delete`, `with` (inclui
  Proxy e `Symbol.unscopables`), generators e async, `this`/`arguments`/`new.target`/`super` lexicais, private
  names, IIFE, recursão mútua, closures com 200 a 1000 variáveis, funções com 255 a 1000 parâmetros e
  identificadores unicode e com escape.
- Cerca de 105 programas terminam com `R` indefinido (promessas que não gravam string): mesmo critério dos outros
  goldens (`<undefined>`).

## Leitura do bytecompiler contra o C++

Conferidos linha a linha com `upstream/JavaScriptCore/bytecompiler/BytecodeGenerator.cpp`: `needs_tdz_check`,
`emit_tdz_check_if_necessary`, `lift_tdz_check_if_possible`, `push_lexical_scope_internal`,
`pop_lexical_scope_internal`, `initialize_block_scoped_functions` (início). Nenhuma lacuna encontrada nesses
trechos; nenhum `todo!`/`unimplemented!` em `src/bytecompiler`. Nenhuma edição de código foi feita.

## Pendente

O teste ainda não foi executado (regra desta rodada: sem cargo). Quando rodar, as falhas do golden dizem onde estão
as lacunas reais; agrupar por categoria antes de corrigir.
