# Auditoria de Proxy, Reflect, classes e Symbol contra o bun

Golden: `tests/golden/proxy_class_bun.tsv` (2010 programas), gerado por `scripts/gen-proxy-class-golden.js`
no bun 1.4.2; teste em `tests/proxy_class_bun_golden.rs` (mesmo padrão de `function_error_bun_golden.rs`,
exige pelo menos 600 programas). Cada programa grava em `R` o resultado formatado ou `NomeDoErro: mensagem`.
Programas que não terminam (2) ou que dão erro de sintaxe no arquivo inteiro (46, sem `R`) foram descartados.

## Cobertura

- As 13 traps, cada uma com invariantes violadas por mais de uma forma de chamada (`Reflect.*`, `Object.*`,
  operador, `with`, herança via `Object.create(proxy)`, `for in`, `JSON.stringify`, spread).
- Proxy revogável (todas as operações), de função, array, classe, de builtins (Map, Date, Promise...).
- `Reflect.*` com argumentos inválidos (as 13 funções com 0 a 3 argumentos de tipos errados).
- Campos privados em objeto errado (`o.#x`, `#x in o`, métodos e accessors privados, estáticos, retorno
  de objeto alheio no construtor base), static blocks (escopo, `await`, `arguments`, `super`, `return`),
  accessor e getters/setters estáticos, `new.target`, `super` em classes e literais, ordem de campos.
- Herança de Array, Map, Set, Error, AggregateError, Promise, RegExp, Date, Function, ArrayBuffer, TypedArray,
  Iterator e `Symbol.species` customizado.
- Símbolos bem conhecidos (os 13), `Symbol.for`/`keyFor`/`description`, `toPrimitive`, `hasInstance`,
  `isConcatSpreadable`, `unscopables`, `asyncIterator`, protocolo de `match`/`replace`/`split`/`search`.

## Leitura do código (sem rodar nada)

`src/runtime/proxy_object.rs`, `proxy_constructor.rs`, `proxy_revoke.rs`, `reflect_object.rs`: conferidas as
mensagens de erro do golden contra o fonte. Todas as mensagens de `TypeError` de Proxy que o bun emite
existem no porte (as que o grep de literal não achou são formatadas com o nome da propriedade ou da trap:
`'{name}' property of a Proxy's handler should be callable`, `... returned falsy value for property '{}'`,
`... not in the result from the 'ownKeys' trap`). As mensagens de `Reflect.*` e `Proxy.revocable` batem com o
bun. As de `Array.isArray`/`Object.prototype.toString` em proxy revogado estão em `array_constructor.rs`.
Nenhuma divergência óbvia por leitura, portanto nenhuma edição de código nesta passada.

## Pendências para a primeira rodada do teste

- Mensagens que dependem do texto-fonte no erro de chamada (`... is not a function. (In '...', '...' is an
  instance of ProxyObject)`, `function is not a constructor (evaluating 'new ...')`) vêm do bytecode
  generator e do `ClassInfo::class_name` ("ProxyObject"), não do código de Proxy: confirmar quando rodar.
- `Function.prototype.toString` em Proxy não chamável deve lançar `TypeError: Type error`.
- Casos que usam `with` e `eval` dentro de static block dependem do porte de escopo dinâmico.
- Os casos assíncronos (`for await`, Promise) leem `R` depois das microtarefas, como o golden de função/erro.

## Auditoria das mensagens com texto-fonte (2026-10-08, por leitura)

A cadeia existe inteira no porte: `ExpressionInfo` no `UnlinkedCodeBlock`
(`has_expression_info`, `expression_info_for_bytecode_index`), emissão no gerador (`emit_expression_info`,
98 usos em `nodes_codegen*`), `append_source_to_error_message` com o trecho exato e o contexto de 20
caracteres (aproximado), `not_a_function_source_appender`, `create_not_a_function_error`,
`create_not_a_constructor_error_at` e `create_not_an_object_error_at` ligados ao `ErrorSite` em
`llint/dispatch.rs` e `llint/slow_paths_object.rs`. Medido no bun 1.4.2 (25 casos): `a is not a function.
(In 'a()', 'a' is undefined)`, `undefined is not a constructor (evaluating 'new a()')`, `function is not a
constructor (evaluating 'new a()')`, `undefined is not an object (evaluating 'a.b')` (`a.b.c()` e `a.b.c.d()`
reportam o trecho do `get_by_id` que falhou), `a[0] is not a function. (In 'a[0]()', ...)`,
`super.x is not a function. (In 'super.x()', ...)`, `a is a Symbol`, `'s' is "x"`, e a template tag cai no
caso aproximado: `undefined is not a function (near '...a.b`x`...')`.

Lacuna corrigida: `error_description_for_value` devolvia `Function` para objeto chamável, e o JSC usa
`smallStrings.functionString()` ("function", minúsculo). Agora sai `function is not a constructor`.

A conferir quando houver execução: template tag (aproximado, sem `(In ...)`), `new` de arrow/método
(`function is not a constructor`), e o caso do Proxy (`an instance of ProxyObject`).
