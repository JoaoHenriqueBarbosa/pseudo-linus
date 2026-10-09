# Auditoria de DisposableStack e using: falhas de tests/explicit_resource_management.rs

Por leitura, sem rodar o build. O `bun` do host serviu de oráculo para mensagens e ordem.

## Causa achada e corrigida

- `use_rejects_values_without_a_dispose_method`: o teste esperava, para `stack.use(1)`, a mensagem de
  `getDisposeMethod` ("Disposable value must be an object, null, or undefined"). No JSC (e no bun) `use` passa por
  `addDisposableResource` e `createDisposableResource`, que checa `isObject` antes e lança
  "Disposable value must be an object". Já `using x = 1` passa direto por `getDisposeMethod` e lança a mensagem longa
  (o teste `using_a_value_without_dispose_throws_type_error` está certo). Expectativa do teste corrigida, o runtime
  já estava fiel.

## Conferido e fiel ao upstream (não é a causa)

- Fontes de `createDisposableResource`, `getDisposeMethod`, `getAsyncDisposeMethod`, `adopt`, `defer`, `dispose`,
  `move`, `use` em `builtins_combined.js` (offsets conferidos).
- `emit_using_body_scope` e `emit_prepare_disposable` (bytecode_generator_cpp5.rs) linha a linha contra
  `BytecodeGenerator.cpp:4722-4960`: sem divergência.
- Intrinsics get/put de campo interno do DisposableStack, capability inicial (array vazio no construtor),
  ligação dos link-time constants, tabela de well-known symbols do lexer (`@@dispose`, `@@asyncDispose`).
- `suppressed_error.rs` usa `error_dup` (`"error"`), correção anterior mantida (nota explicit-resource-management.md).

## Ainda aberto (sem causa por leitura)

Com o oráculo, o comportamento esperado de `move`, de `using` com valor inválido e de `await using` é o dos testes.
Sobram falhas que dependem de execução: `move_transfers...`, `using_a_value_without_dispose...`, `two/three_throwing...`,
bloco/laço/switch, inicializador que lança, e o hang de `await using` (laço de drenagem ou promessa que nunca
resolve). O padrão comum: todos passam pelo caminho de exceção capturada (`SynthesizedCatch` / out-of-line handler)
ou pelo `await` dentro do finally sintetizado. Suspeitos em ordem: `emit_out_of_line_exception_handler` com
`try_slot_data` repetido, `emit_await` dentro de handler `SynthesizedFinally` (retomada do gerador async), e
`emit_construct` de `SuppressedError` com `CallArguments` de 2 argumentos. Próximo passo: rodar um teste por vez
com `ZJSC` dump de bytecode e comparar com o bun.
