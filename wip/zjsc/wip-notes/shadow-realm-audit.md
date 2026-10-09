# ShadowRealm: golden e auditoria

## Golden

- Gerador: `scripts/gen-shadow-realm-golden.js` (bun 1.4.2), saída em `tests/golden/shadow_realm_bun.tsv` (1617 programas
  mantidos, 17 descartados por serem inválidos como programa). O teste é `tests/shadow_realm_bun_golden.rs`, no padrão de
  `function_error_bun_golden.rs` (arquivo `shadow_realm_case.js`, modo estrito, `R` lido depois das microtarefas).
- Cobre: construtor (sem `new`, `new.target` via `Reflect.construct`, subclasse, descritores), receptor inválido,
  `evaluate` (argumento não string, primitivos, objetos rejeitados, funções viram wrapped functions, `name`/`length`/
  `prototype`/descritores), chamada de wrapped functions (`this`, argumentos primitivos e objetos, retorno de função
  vira wrapped, funções do chamador passadas ao realm), erro lançado no realm vira `TypeError` do chamador com a
  mensagem, `SyntaxError` do chamador no `evaluate`, `eval` indireto e modo estrito no realm, isolamento de globais e
  protótipos (`Array.prototype`, `instanceof`, `Error`, `Symbol`), realm aninhado, `importValue` (promessa, módulo
  inexistente, `data:` URLs, argumentos inválidos) e `Symbol.toStringTag`.
- Não rodei cargo nem o teste. Os resultados do porte são desconhecidos; quando o teste rodar, as divergências viram
  a lista de trabalho.

## Auditoria por leitura (contra `upstream/JavaScriptCore/runtime`)

Arquivos lidos: `shadow_realm_constructor.rs`, `shadow_realm_object.rs`, `shadow_realm_prototype.rs`,
`shadow_realm_globals.rs`, `js_remote_function.rs`, contra `ShadowRealmConstructor.cpp`, `ShadowRealmObject.cpp`,
`ShadowRealmPrototype.cpp`, `JSRemoteFunction.cpp`, `Error.cpp` (`createTypeErrorCopy`) e
`builtins/ShadowRealmPrototype.js`.

Nenhuma divergência óbvia de lógica; nenhuma edição de código feita. Conferido:

- `callShadowRealm` e `constructWithShadowRealmConstructor` (o `newTarget` é ignorado em ambos; o porte lê o `prototype`
  do construtor, que é só leitura e não deletável, igual ao `m_shadowRealmPrototype`).
- `evalInRealm`: ordem SyntaxError do chamador vs `TypeError` por `createTypeErrorCopy`, `this` do realm, exceção do
  `executeEval` limpa e copiada. `createTypeErrorCopy` bate (primitivo vira string, objeto não Proxy usa a propriedade
  própria `message` de dado, senão "Error encountered during evaluation").
- `JSRemoteFunction`: `wrapValue`/`wrapArgument`/`wrapReturnValue`, realm do retorno (caminho rápido no chamador,
  genérico no alvo), desembrulho de remote em remote, `copyNameAndLength`, "wrapping returned function throws an error",
  reificação lazy de `name`/`length` (`js_function_reify.rs`).

Divergências conhecidas, já documentadas nos arquivos e não corrigidas aqui:

1. `importInRealm` não repassa a `callerSourceOrigin` ao `importModule` (o porte chama `load_and_evaluate_module(global,
   specifier)`). Afeta só a resolução de especificador relativo a partir do chamador; o golden usa `data:` e nomes
   inexistentes, então não distingue. Corrigir exige a API do loader de módulos aceitar a origem.
2. `JSGlobalObject::init` completo no `deriveShadowRealmGlobalObject`: cada `new ShadowRealm()` cria um global inteiro
   (custo, não semântica).
3. A propriedade global `ShadowRealm` depende de `Options::use_shadow_realm()`; `src/api/bun_options.rs` liga a opção,
   então o golden só passa por `evaluate_named_script_result` se esse caminho aplicar as opções do bun. Se os testes
   falharem todos com `ReferenceError: ShadowRealm`, é isso.
