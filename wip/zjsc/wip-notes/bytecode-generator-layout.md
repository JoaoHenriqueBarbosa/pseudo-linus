# Layout do BytecodeGenerator em bytecompiler/

Levantamento só de leitura (grep/sed), sem compilar.

## Estado atual

- Nenhum arquivo `bytecode_generator*.rs` tem `include!` de verdade (os `include!` achados são só
  comentários "Juntada por include!"). O anfitrião `bytecode_generator.rs` não inclui ninguém.
- `bytecompiler/mod.rs` registra só: `existing_variable_mode`, `label`, `label_scope`,
  `profile_type_bytecode_flag`, `register_id`, `static_property_analysis`, `static_property_analyzer`,
  `bytecode_generator_base`. Faltam `bytecode_generator` e `nodes_codegen`.
- `nodes_codegen.rs` já é anfitrião correto (13 `include!` das fatias `nodes_codegen_cpp*.rs`).
- Fragmentos (sem `use`, sem `mod`, caminhos completos; dependem dos `use` do anfitrião):
  `part2`, `part3`, `cpp1`, `cpp1c`, `cpp2`, `cpp3`, `cpp4`, `cpp5`, `cpp6`.
- Módulos próprios: `bytecode_generator.rs` (anfitrião, tem `use` e `//!`) e `bytecode_generator_base.rs`
  (módulo independente, usa `BytecodeGeneratorBase<Traits>`, não pode ser incluído).

## O que cada um define

| Arquivo | Itens |
|---|---|
| `bytecode_generator.rs` (anfitrião) | enums `ExpectedFunction`, `EmitAwait`, `DebuggableCall`, `ThisResolutionType`, `InvalidPrototypeMode`, `VariableKind`; structs `CallArguments`, `Variable`, `CompletionType`, `FinallyJump`, `CompletionRecord`, `FinallyContext`, `ControlFlowScope`, `ForInContext`, `TryData`, `TryContext`, `TryRange`, `UsingSlot`, `UsingScope`; consts `CONTROL_FLOW_SCOPE_*`; tipos `ForIn*Inst`; `impl BytecodeGeneratorTraits for JSGeneratorTraits`, `impl LabelGenerator`/`OpWriter for BytecodeGenerator`; trait `BytecodeGeneratorNode`; `impl BytecodeGenerator` (a partir da linha 639, até `emitNode`). `pub use` de `RegisterRef` e `JSGeneratorTraits`. |
| `part2` | `impl BytecodeGenerator` (.h 502 a 999); `enum IsNotTypeofUndefined`; `PROPERTY_CONFIGURABLE/WRITABLE/ENUMERABLE`. |
| `part3` | enums `ScopeType`, `TDZCheckOptimization`, `NestedScopeType`, `TDZRequirement`, `ScopeRegisterType`, `TDZNecessityLevel`, `FunctionVariableType`; tipos `TDZMap`, `TDZStackEntry`, `BigIntMapEntry`; structs `PreservedTDZStack`, `LexicalScopeStackEntry`, `AsyncFuncParametersTryCatchInfo`, `CatchEntry`, `LastDebugHook`, **`struct BytecodeGenerator`** (campos), `StrictModeScope` (+Drop); `impl BytecodeGenerator`. |
| `cpp1` | trait `VarArgsOp` + 6 impls para `OpCall...OpSuperConstruct`; `impl Variable`, `impl FinallyContext` (`new`, `dump`), `impl BytecodeGenerator` (construtores `new_program`, `new_function`). |
| `cpp1c` | `impl BytecodeGenerator` (`new_eval`). |
| `cpp2` a `cpp5` | cada um só `impl BytecodeGenerator`. |
| `cpp6` | `impl BytecodeGenerator`; `fn rewrite_op` (privada); `impl ForInContext` (`finalize`); `impl Display for VariableKind`. |

## Colisões

- Nomes repetidos em `impl BytecodeGenerator` entre arquivos: nenhum (varri todos os `fn` com 4 espaços de
  indentação em todos os arquivos; os 3 nomes repetidos, `new`, `local`, `this_register`, estão em tipos
  diferentes: `Variable`, `ForInContext`, `FinallyContext`, `BytecodeGenerator`).
- Impl de trait repetido: nenhum (`Display for VariableKind` só em cpp6; `VarArgsOp` só em cpp1).
- Métodos inerentes de `Variable`/`FinallyContext`/`ForInContext` espalhados em dois arquivos
  (host + cpp1, host + cpp6): sem sobreposição de nomes, vale desde que todos no mesmo módulo efetivo.
- Referências quebradas ao registrar: `nodes_codegen_cpp1b.rs:691` e `nodes_codegen_cpp6.rs:365` usam
  `crate::bytecompiler::bytecode_generator_part2::PROPERTY_CONFIGURABLE`. Como `part2` será incluído e não
  módulo, trocar para `crate::bytecompiler::bytecode_generator::PROPERTY_CONFIGURABLE`.
- Comentário defasado: o cabeçalho do host diz que a struct fica em `part2`; ela está em `part3`.
  Também `runtime/get_put_info.rs:7` cita "dono part3/cpp2" (divergência de `GetPutInfo`, ver outro agente).

## Proposta para bytecompiler/mod.rs

```rust
pub mod bytecode_generator_base;
pub mod bytecode_generator;
pub mod nodes_codegen;
```

No fim de `bytecode_generator.rs` (substituindo o comentário "Continua em..."), na ordem do fonte
(.h: part2, part3; .cpp: cpp1, cpp1c, cpp2..cpp6; a ordem em Rust é irrelevante para itens, mas esta
segue o C++ e o cabeçalho dos arquivos):

```rust
include!("bytecode_generator_part2.rs");
include!("bytecode_generator_part3.rs");
include!("bytecode_generator_cpp1.rs");
include!("bytecode_generator_cpp1c.rs");
include!("bytecode_generator_cpp2.rs");
include!("bytecode_generator_cpp3.rs");
include!("bytecode_generator_cpp4.rs");
include!("bytecode_generator_cpp5.rs");
include!("bytecode_generator_cpp6.rs");
```

Observação: os `use` do anfitrião valem para os fragmentos (`Rc`, `RefCell`, `Cell`, `HandlerType`, `Label`,
`LabelScope`, `Identifier`, `VarOffset`, `READ_ONLY`...); erros de nome não resolvido na primeira
compilação devem ser resolvidos acrescentando `use` no anfitrião ou caminho completo no fragmento.
Os `//!` (inner doc) só existem no topo do anfitrião e do base, então `include!` no meio do arquivo não
quebra.
