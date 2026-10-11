# Gerenciamento explícito de recursos: falhas em tests/explicit_resource_management.rs

Análise por leitura (sem rodar nada).

Verificado e correto:
- `install_disposable_stacks`, `install_aggregate_error` e `install_suppressed_error` são chamados em `JSGlobalObject::init`
  (`js_global_object_init.rs`), com `Options::use_explicit_resource_management()` verdadeiro por padrão.
- LinkTimeConstants `AddDisposableResource`, `CreateDisposableResource`, `GetDisposeMethod`, `GetAsyncDisposeMethod`,
  `DisposableStack`, `AsyncDisposableStack` e `SuppressedError` são ligados no init.
- `Symbol.dispose`/`Symbol.asyncDispose` existem em `symbol_constructor.rs` e nos protótipos.
- Emissores dos intrinsics (`isDisposableStack`, `get/putDisposableStackInternalField`) e constantes de campo conferem.

Bug achado e corrigido:
- `suppressed_error.rs` gravava a propriedade `error` com `vm.property_names.error`, que em `common_identifiers.rs` é
  o identificador `"Error"` (o nome do construtor). O `"error"` minúsculo é `error_dup`. Corrigido para `error_dup`.
  Isso derruba os testes de propriedades de `SuppressedError` e de cadeia (`.error`), em todos os caminhos
  (`dispose`, `using`, `disposeAsync`).

Ainda aberto (não achado por leitura): se os testes básicos de `DisposableStack` (adopt/defer/use/move) seguirem
falhando depois dessa correção, ler o nome da exceção impresso no panic. Suspeitos restantes: colisão de nomes
em `common_identifiers.rs` (campos `*_dup` que escondem o identificador certo, como `error`) e a resolução de `@@dispose`
em `lexer_part2.rs` (`look_up_well_known_symbol`).
