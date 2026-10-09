# Membros de `VM` (e tipos ligados) usados pelo bytecompiler

Levantamento por grep em `src/bytecompiler/*.rs` (acessos `vm.x`, `self.vm.x`, `generator.vm().x`),
conferido contra `src/runtime/vm.rs` e os tipos referidos. Caminhos upstream relativos a
`upstream/JavaScriptCore/`.

Formato: chamador | membro | existe? | onde o C++ define.

## Ausentes ou com assinatura diferente

- `bytecompiler/bytecode_generator.rs:676` | `VM::property_names()` (método) | NÃO: `VM.property_names` é campo `pub` (`PropertyNames`, com `Deref` para `CommonIdentifiers`), não há método | `runtime/VM.h:642` (`CommonIdentifiers* propertyNames`)
- `bytecompiler/bytecode_generator_cpp2.rs:106` | `VM::property_names()` e `.star_namespace_private_name()` | NÃO (método) e assinatura diferente: em `CommonIdentifiers` é campo (`common_identifiers.rs:29`), o chamador usa `()` | `bytecompiler/BytecodeGenerator.cpp:1126`
- `bytecompiler/bytecode_generator_cpp2.rs:113` | `VM::property_names()` (`.builtin_names().meta_private_name()` existe em `builtin_names/generated.rs:5287`) | NÃO só o método `property_names()` | `bytecompiler/BytecodeGenerator.cpp:1128`
- `bytecompiler/bytecode_generator_cpp3.rs:1382` | `VM::property_names()` e `.length()` | NÃO (método) e assinatura diferente: `length` é campo (`common_identifiers.rs:248`) | `bytecompiler/BytecodeGenerator.cpp:2845`
- `bytecompiler/bytecode_generator_cpp4.rs:1242` | `VM::property_names()` e `.name` | NÃO só o método (`name` é campo, `common_identifiers.rs:273`, ok) | `bytecompiler/BytecodeGenerator.cpp:3648`
- `bytecompiler/bytecode_generator_cpp5.rs:672` | `VM::property_names()` e `.empty_identifier` | NÃO só o método (`empty_identifier` é campo, `common_identifiers.rs:16`, ok) | `bytecompiler/BytecodeGenerator.cpp:4424`
- `bytecompiler/bytecode_generator.rs:753` | `VM::defer_gc()` (guarda `DeferGC`) | NÃO (nem o tipo `DeferGC`, não há `Heap` portado) | `heap/DeferGC.h:42` (`DeferGC(VM&)`); uso do gerador em `bytecompiler/BytecodeGenerator.h:404`
- `bytecompiler/bytecode_generator_cpp4.rs:727` | `VM::compact_variable_map()` (`.get(environment)`) | NÃO (o tipo `CompactTDZEnvironmentMap` existe em `parser/variable_environment.rs:873`, com `get` em `:884` sobre `self: &Rc<Self>`; falta o campo/acessor no `VM`) | `runtime/VM.h:962` (`m_compactVariableMap`); uso em `bytecompiler/BytecodeGenerator.cpp:3396`
- `bytecompiler/bytecode_generator_cpp4.rs:1100` | `VM::builtin_executables()` (`.create_default_constructor(...)`) | NÃO (nem `BuiltinExecutables`: não existe `src/builtins`) | `runtime/VM.h:1033`; `builtins/BuiltinExecutables.h:83` (`createDefaultConstructor`)
- `bytecompiler/nodes_codegen_cpp2.rs:696` | `BytecodeIntrinsicRegistry::constant_value(constant, generator)` | NÃO (o registro tem só `new`, `lookup` e os acessores de `Entry`; faltam os `name##Value`, inclusive `orderedHashTableSentinelValue`, que outro agente cuida no `vm.rs`) | `bytecode/BytecodeIntrinsicRegistry.h:208` (macro `name##Value`); `bytecode/BytecodeIntrinsicRegistry.cpp:125` e `:132`; chamada em `bytecompiler/NodesCodegen.cpp:2166`

## Existentes

- `bytecompiler/bytecode_generator_part2.rs:13,105,139,186` | `VM::is_safe_to_recurse()` | sim (`vm.rs:115`) | `runtime/VM.h:890`; uso em `bytecompiler/BytecodeGenerator.h:506,565,585,608`
- `bytecompiler/nodes_codegen_cpp6.rs:590` e `nodes_codegen_cpp7.rs:91` | `generator.vm().is_safe_to_recurse()` | sim | `bytecompiler/NodesCodegen.cpp:5840` e `:6034`
- `bytecompiler/nodes_codegen_cpp2.rs:515` | `vm.property_names.builtin_names().assert_private_name()` | sim (`common_identifiers.rs:812`, `builtin_names/generated.rs:3847`) | `bytecompiler/NodesCodegen.cpp:1429`
- `bytecompiler/nodes_codegen_cpp2.rs:730,904` | `vm.property_names.builtin_names().look_up_private_name(..)` | sim (`builtin_names.rs:67`) | `bytecompiler/NodesCodegen.cpp:1501`, `:1800`
- `bytecompiler/nodes_codegen_cpp2.rs:1091` | `vm.property_names.empty_identifier` | sim | `bytecompiler/NodesCodegen.cpp:1998`
- `bytecompiler/nodes_codegen_cpp1.rs:664,666` | `vm.cell_butterfly_only_atom_strings_structure()`, `vm.cell_butterfly_structure(IndexingType)` | sim (`runtime/js_cell_butterfly.rs:59,71`, `impl VM`) | `runtime/VM.h:402`, `:572`; uso em `bytecompiler/NodesCodegen.cpp:469`
- `bytecompiler/nodes_codegen_cpp1.rs:688` | `vm.atom_string_to_js_string_map().ensure_value(..)` | sim (`vm.rs:99`, acrescentado agora por outro agente; a assinatura de `ensure_value` não foi conferida) | `runtime/VM.h:723`; uso em `bytecompiler/NodesCodegen.cpp:481`
- `bytecompiler/nodes_codegen_cpp2.rs` (`generator.vm().bytecode_intrinsic_registry()`) | `VM::bytecode_intrinsic_registry()` | sim (`vm.rs:85`) | `runtime/VM.h:1098`
- `bytecompiler/bytecode_generator_cpp4.rs:770` | `DeferTermination::new(&vm)` | sim (`vm.rs:199`, `&Rc<VM>` coage para `&VM`) | `bytecompiler/BytecodeGenerator.cpp:3428`
- `bytecompiler/nodes_codegen_cpp1.rs:170,180`, `nodes_codegen_cpp1b.rs:626`, `bytecode_generator_part3.rs:798` | `VM` repassada a `RegExp::create`, `IdentifierArena::make_identifier`, `PropertyNode::is_underscore_proto_setter`, `is_arguments_length_access` | sim (`reg_exp.rs:83`, `parser_arena.rs:69`, `nodes.rs:1051`, `nodes.rs:1213,2160`) | `runtime/RegExp.h`, `parser/Nodes.h:777`, `:222`, `:902`
- `bytecompiler/nodes_codegen_cpp1.rs:661` | `vm.heap.isDeferred()` | só comentário (`ASSERT` de depuração, some) | `bytecompiler/NodesCodegen.cpp:467`

## Contagem

Ausentes: 5 membros distintos (`VM::property_names()` como método, com 6 chamadores, mais as
chamadas `length()` e `star_namespace_private_name()` com `()` sobre campos; `defer_gc`;
`compact_variable_map`; `builtin_executables`; `BytecodeIntrinsicRegistry::constant_value`).
