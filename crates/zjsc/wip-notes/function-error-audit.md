# Auditoria de Function e Error (golden contra o bun 1.4.2)

## O que foi criado

- `scripts/gen-function-error-golden.js`: gera `tests/golden/function_error_bun.tsv` (915 programas, sem caminho
  da máquina: o diretório temporário sai do texto e resultado que ainda tenha caminho é descartado; saída
  determinística em duas execuções). Arquivo fixo `function_error_case.js` dos dois lados.
- `tests/function_error_bun_golden.rs`: roda cada programa com `evaluate_named_script_result` (mesmo padrão de
  `stack_golden.rs`), compara `R` e lista as divergências. Exige pelo menos 400 programas.
- Cobertura: `Function.prototype.toString` (declaração, expressão, arrow, async, gerador, método, getter/setter,
  classe com e sem extends, nativas, bound, Proxy, comentários e espaços, `new Function`, GeneratorFunction,
  AsyncFunction e AsyncGeneratorFunction), `name`/`length` (atribuição, símbolo, getter/setter, computed,
  bound, campos de classe, privados), `Error` (cause, AggregateError, `Promise.any`, captureStackTrace,
  stackTraceLimit, prepareStackTrace, CallSite, `Error.prototype.toString`) e o formato de `err.stack`.

## Achados por inspeção (nada rodado, só leitura)

- `src/runtime/function_prototype.rs::function_proto_func_to_string` segue o JSC (JSFunction, InternalFunction,
  callable, senão `throw_vm_type_error` com "Type error", igual ao bun medido). Sem divergência óbvia.
- `src/runtime/error_prototype.rs::error_to_string` implementa o algoritmo da spec; a casca nativa
  (`errorProtoFuncToString`, tabela estática, `ErrorPrototype::create`) ainda está pendente de ligação, conforme
  o cabeçalho do módulo. Não há o que corrigir por Edit pequeno: é trabalho de ligação.

## Peculiaridades do bun registradas no golden (o zjsc terá de reproduzi-las)

- `Error.captureStackTrace()` sem argumento lança `TypeError: invalid_argument` (mensagem do bun).
- Frames nativos saem como `at map (native:1:11)` e o topo como `at <anonymous> (function_error_case.js:L:C)`.
- Mensagem de `Function.prototype.toString.call({})`: `TypeError: Type error`.

## Pendências

- Rodar `cargo test --test function_error_bun_golden` (não executado nesta tarefa) e triar as divergências.
