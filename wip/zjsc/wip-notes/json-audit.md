# Auditoria de JSON (golden contra o bun 1.4.2)

## O que foi criado

- `scripts/gen-json-golden.js`: gera `tests/golden/json_bun.tsv` (2447 programas, estável entre duas execuções) rodando
  cada programa em um único processo do bun. Cada programa grava em `R` o valor descrito (`S`) ou a exceção (`T`).
  Cobre: prefixos de documentos válidos (mensagens de EOF e posição), inserção de caractere ruim, BOM, controle em
  string, números inválidos, `__proto__` e chaves duplicadas, profundidade até 100000, reviver (ordem de visita,
  `delete`/`undefined`, mutação de `this`, `context.source`, que o bun expõe), `JSON.rawJSON`/`isRawJSON` (existem no
  bun), `JSON.stringify` com replacer função e array, `space`, `toJSON`, ciclos (a mensagem do bun não inclui a
  cadeia: `JSON.stringify cannot serialize cyclic structures.`), surrogates soltos, wrappers, esparsos, typed
  arrays, Proxy, ordem de chaves inteiras, `JSON[Symbol.toStringTag]`.
- `tests/json_bun_golden.rs`: harness no padrão de `function_error_bun_golden.rs` (não rodado, sem cargo nesta tarefa).
- `json_number_bun.tsv` não foi duplicado: lá ficam os números isolados em `eval`.

## Auditoria de `src/runtime/json_object*.rs` e `literal_parser.rs` contra o upstream

Lidos: catálogo de mensagens do `literal_parser.rs` (incluindo `\u must be followed by 4 hex digits` e
`"\uXXXX" is not a valid unicode escape`, idênticas ao `LiteralParser.cpp`), mensagem de ciclo, de BigInt e a
validação de `rawJSON`. Nenhuma divergência óbvia achada, nenhuma edição feita.

## Ligação do JSON no global (2026-10-08)

Já feita por outro agente: `json_object_native::create_json_object` (chamado em
`install_json_reflect_and_collections`, junto de Reflect) e `json_host.rs`. Conferido contra o bun 1.4.2:
`Object.getOwnPropertyNames(JSON)` é `parse, stringify, isRawJSON, rawJSON` (não `rawJSON, isRawJSON`), mais
`Symbol.toStringTag` nas chaves; `length` 2, 3, 1, 1; funções `{writable, !enumerable, configurable}`; tag
`{value:"JSON", !writable, !enumerable, configurable}`. A ordem no porte estava invertida (isRawJSON, rawJSON, parse,
stringify); reordenada em `create_json_object`. Cabeçalho "LIGAÇÃO PENDENTE" de `json_object.rs` atualizado.
Sem cargo: nada compilado nem rodado.

## Pendência histórica (resolvida pela ligação acima)

`json_object.rs` declara "LIGAÇÃO PENDENTE": o `JSON` não está registrado no global (falta a `NativeFunction` final e o
`impl JsonHost for JSGlobalObject`, incluindo a célula `JSRawJSONObject`, `Symbol.toStringTag` e o `context` do
reviver). Enquanto isso o `json_bun_golden` falha inteiro; a primeira rodada deve medir só o que a ligação resolve.
`FastStringifier` não portado (sem efeito observável, segundo o cabeçalho do módulo).
