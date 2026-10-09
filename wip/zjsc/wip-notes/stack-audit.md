# Auditoria de stack traces (golden ampliado)

Golden: `tests/golden/stack_more_bun.tsv` (500 programas), gerado por `scripts/gen-stack-more-golden.js` no bun 1.4.2
(`bun scripts/gen-stack-more-golden.js > tests/golden/stack_more_bun.tsv`). Teste: `tests/stack_more_bun_golden.rs`
(mesmo padrão de `tests/stack_golden.rs`, `evaluate_named_script_result` com o nome `file.js`).

Normalização: o diretório temporário e o prefixo `file://` saem do texto, então toda URL de frame vira `file.js`
(inclusive frames de `new Function` e `eval`, que o bun imprime com URL `file://`). Programa que não define `R`
(erro de sintaxe do programa inteiro, exceção não capturada, `stack` indefinido) vale `<undefined>`; no teste, uma
exceção do `evaluate_named_script_result` também vale `<undefined>`.

## Cobertura (por família)

- Todo tipo de função com `new Error('x').stack`: nomeada, anônima, arrow, método, getter, setter, construtor, derivado
  (explícito e implícito), estático, getter estático, static block, field e static field, método privado, computado,
  símbolo, generator, async (antes e depois de `await`, aninhado, arrow, método, estático), async generator, eval
  (direto, indireto, aninhado), `new Function`, Proxy (get, set, has, apply, construct), tagged template, reviver,
  replacer, `toJSON`, comparador de `sort`, callbacks de `map`/`forEach`/`filter`/`reduce`/`find`/`some`/`flatMap`/
  `Array.from`/`Map.forEach`/`Set.forEach`, `Promise` (executor, then, catch, finally, queueMicrotask),
  `Symbol.toPrimitive`/`toString`/`valueOf`/`hasInstance`/`species`/`iterator`, `Reflect.construct`/`get`, `new.target`.
- Os mesmos tipos embrulhados em 5 formas de chamador (depth1, depth2, arrow, try, método).
- `call`/`apply`/`bind`/`Reflect.apply`/`Function.prototype.call.call`/`map`/`Array.from`/`new` sobre 4 formas de função.
- `Error.stackTraceLimit` em 0, 1, 3, 50, Infinity, `'x'`, `-1`, `2.7`, apagado, descritor.
- `Error.captureStackTrace(obj, fn)` (19 casos), `Error.prepareStackTrace` com CallSite (15 métodos em 5 contextos,
  mais 20 casos de contrato: retorno não string, lança, chamado uma vez, `this`, etc.).
- `cause` e `AggregateError` (37 casos), expressões multi-linha (47 casos, incluindo CRLF, tab, UTF-8, comentários),
  SyntaxError de `eval`/`new Function`/eval indireto/`JSON.parse`/`RegExp` (cerca de 90 casos).

## Medições que chamam atenção (o que o bun faz)

- `Error.stackTraceLimit` em 0, `'x'`, `-1` ou apagado: `err.stack` é `undefined` (o golden antigo já tem o caso 0).
- `Error.stackTraceLimit = 2.7` trunca para 2 frames.
- Frame de async depois de `await` mostra só `at a (file.js:L:C)`, sem o chamador.
- Frame de static block: `at <anonymous> (file.js:L:C)` seguido de `at <anonymous> (file.js:L)` (programa sem coluna).
- `getTypeName()` é `undefined` em função solta e em método de objeto literal em modo estrito.
- `throw` seguido de quebra de linha é erro de sintaxe do programa, então `R` não é gravado.

## Divergências por leitura (`stack_frame.rs`, `error_instance.rs`, `error_natives.rs`)

Não rodei cargo, então o golden novo não foi executado contra o zjsc. A leitura não achou divergência óbvia que
justificasse Edit: o formato (`at nome (url:linha:coluna)`, `new ` de construtor, coluna 1 omitida, `native:1:11` de
builtin, `eval (unknown)`) e o `stackTraceLimit` (espelho no global, inicial 10 em `bun_options.rs`) já seguem as
medições. Ponto a conferir quando o teste rodar: `capture_frames` devolve `Some(vec![])` com limite 0, e o bun
imprime `stack` indefinido nesse caso (o golden antigo cobre o 0; o novo cobre `-1`, `'x'` e delete).
