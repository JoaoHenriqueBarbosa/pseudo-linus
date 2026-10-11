# Métodos do BytecodeGenerator chamados e não definidos (levantamento de 2026-10-08)

Escopo: `generator.X(` em `src/bytecompiler/nodes_codegen*.rs` e `self.X(`/`generator.X(` nos fragmentos
`bytecode_generator.rs`, `bytecode_generator_cpp*.rs`, `bytecode_generator_part*.rs`, `generator_parser_arena.rs`.
Os `self.X(` dos `nodes_codegen*.rs` são métodos dos nós (`impl ...Node`), não do gerador, e ficam fora.
Método: nome chamado conferido contra `fn NOME` com a regex ancorada no início da linha
(`^\s*(pub )?fn`), para que a assinatura em comentário `// pub fn ...  // .cpp` do `part3` NÃO conte como
definição (a primeira varredura, sem âncora, errava por isso). Nada foi compilado.

Nota: a conferência é por nome em qualquer `impl` dos fragmentos; um nome definido só em `impl Variable`,
`ForInContext` etc. passaria. Conferido à parte: nenhuma chamada `generator.X` cai nesse caso (`this_register`
existe em `BytecodeGenerator`, `bytecode_generator.rs:768`).

## Contagens

- 16 nomes distintos sem `fn` no `impl BytecodeGenerator`, 4 grupos (abaixo), cerca de 640 chamadas.
- `generator.move(`: só comentário (`nodes_codegen_cpp5d.rs:49`), não é chamada.
- `parser_arena`, `emit_node_in_ignore_result_position_statement`: já existem agora (`generator_parser_arena.rs:31`,
  `bytecode_generator_cpp7.rs:7`), o inventário `nodes-codegen-missing-names.md` está defasado nesses dois.
- Aridade dos 30 métodos mais usados (`move_register`, `emit_node_expression`, `emit_load_js_value`, ...,
  `emit_call`; 1.300 chamadas): 29 batem em 100% das chamadas; só `variable` diverge (11 chamadas).

## 1. Métodos da `BytecodeGeneratorBase` (campos achatados no `BytecodeGenerator`, sem repasse)

O `BytecodeGenerator` achata os membros da base (`part3.rs:119`), então `impl BytecodeGeneratorBase<Traits>`
não vale para ele. Os oito existem só em `bytecode_generator_base.rs`. Falta também o campo `labels`
(`Vec<Rc<RefCell<GenericLabel<..>>>>`) no struct; só `callee_locals` foi achatado. Não pode ser função de
repasse (regra DRY do projeto): precisa de `Deref`/campo `base`, ou as funções da base virarem livres sobre os
campos (como `record_opcode_in`/`write_opcode_in`) e o `BytecodeGenerator` as chamar.

| nome | chamadas (gerador, fora da base) | exemplos | existente na base | C++ |
|---|---|---|---|---|
| `new_temporary` | 260 | `nodes_codegen_cpp3.rs:180`, `:182`, `:283` | `bytecode_generator_base.rs:220` `new_temporary(&mut self) -> RegisterIDRef` | `BytecodeGeneratorBaseInlines.h:125` |
| `new_label` | 121 | `nodes_codegen_cpp3.rs:155`, `:156`, `:336` | `base.rs:163` `new_label(&mut self) -> GenericLabelRef<Traits>` | `BytecodeGeneratorBaseInlines.h:51` (decl `Base.h:57`) |
| `emit_label` | 130 | `nodes_codegen_cpp3.rs:234`, `:249`, `:354` | `base.rs:183` `emit_label(&mut self, &GenericLabelRef<Traits>)` | `BytecodeGeneratorBaseInlines.h:75` |
| `new_emitted_label` | 19 | `bytecode_generator_cpp1.rs:85`, `:96`; `nodes_codegen_cpp5c.rs:289` | `base.rs:173` | `BytecodeGeneratorBaseInlines.h:61` (decl `Base.h:58`) |
| `add_var` | 13 | `bytecode_generator_cpp1c.rs:44`, `:73` | `base.rs:238` `add_var(&mut self) -> RegisterIDRef` | `BytecodeGeneratorBaseInlines.h:148` (decl `Base.h:60`) |
| `new_register` | 1 | só dentro da base hoje | `base.rs:206` | `BytecodeGeneratorBaseInlines.h:114` (decl `Base.h:59`) |
| `new_temporaries` | 1 | `nodes_codegen_cpp2.rs:208` | `base.rs:228` `new_temporaries(count, FnMut(&RegisterIDRef))` | `BytecodeGeneratorBaseInlines.h:136` |
| `reclaim_free_registers` | 1 | `bytecode_generator_cpp2.rs:601` | `base.rs:179` (`pub(crate)`) | `BytecodeGeneratorBaseInlines.h:69` (decl `Base.h:83`, protected) |

Obs.: `emit_label`/`new_label` do gerador pedem `LabelRef` (alias do `GenericLabelRef<JSGeneratorTraits>`); o
`emit_label` também precisa do `set_label_location` do trait (`bytecode_generator.rs:534`), que recebe
`&mut BytecodeGeneratorBase<Self>`, incompatível com os campos achatados: a assinatura do trait precisa mudar
junto.

## 2. Sobrecargas C++ que o Rust renomeou e o chamador usa o nome sem sufixo

| nome chamado | chamadores | candidato existente | C++ |
|---|---|---|---|
| `emit_debug_hook_expression` (9) | `nodes_codegen_cpp1.rs:71`, `nodes_codegen_cpp4.rs:204`, `:256` | `emit_debug_hook_expression_data(&mut self, &Expression, Option<RegisterRef>)` em `bytecode_generator_cpp5.rs:333` | `BytecodeGenerator.cpp:4183`, decl `BytecodeGenerator.h:1057` |
| `emit_debug_hook_statement` (1) | `bytecode_generator_part2.rs:18` | `emit_debug_hook_statement_data(&mut self, &Statement, Option<RegisterRef>)` em `cpp5.rs:315` | `BytecodeGenerator.cpp:4174`, decl `h:1056` |
| `emit_debug_hook_position` (3) | `bytecode_generator_cpp4.rs:1293`, `:1745`, `:1971` | `emit_debug_hook(&mut self, DebugHookType, &JSTextPosition, Option<RegisterRef>)` em `cpp5.rs:288` | `BytecodeGenerator.cpp:4156`, decl `h:1055` |
| `emit_debug_hook_property_list` (1) | `bytecode_generator_part2.rs:143` | `PropertyListNode` é `ExpressionNode` no C++ (`Nodes.h:799`); usar `emit_debug_hook_expression_data` com o `PropertyListNode` visto como `Expression` (o `n` aqui é `&PropertyListNodeRef`, sem conversão pronta) | `BytecodeGenerator.h:588` (`emitDefineClassElements`: `emitDebugHook(n)`) |
| `emit_node_dst_statement` (1) | `bytecode_generator_part2.rs:32` | `emit_node(&mut self, Option<&RegisterRef>, &Statement)` em `bytecode_generator.rs:875`; o C++ é `emitNode(StatementNode*)` chamando `emitNode(nullptr, n)` | `BytecodeGenerator.h:524` |
| `emit_node_in_tail_position` (1) | `bytecode_generator.rs:880` | `emit_node_in_tail_position_statement(&mut self, Option<Rc<RefCell<RegisterID>>>, &Statement)` em `part2.rs:7` (o chamador passa `Option<&RegisterRef>`: precisa de `.cloned()`) | `BytecodeGenerator.h:502` |
| `emit_throw_static_error_identifier` (1) | `nodes_codegen_cpp1.rs:181` | `emit_throw_static_error(&mut self, ErrorTypeWithExtension, &Identifier)` em `cpp5.rs:638` | `BytecodeGenerator.cpp:4397`, decl `h:1037` |

Resolução mais limpa: renomear o chamador para o nome existente (os candidatos são os definidos), em vez de
criar um segundo nome. O comentário `part3.rs:409-427` lista as assinaturas com nome sem sufixo, também defasado.

## 3. Sem candidato

| nome | chamador | situação | C++ |
|---|---|---|---|
| `new_parameter_register` (1) | `bytecode_generator_cpp2.rs:503` (`initialize_next_parameter`) | O C++ não cria registrador nomeado: `m_parameters.grow(m_parameters.size() + 1)` (vetor de `RegisterID` por valor), depois `registerFor(reg)`. O Rust tem `parameters: Vec<Rc<RefCell<RegisterID>>>` (`part3.rs:159`) e chama função inexistente. Candidato de construção: `RegisterID::new()` (`register_id.rs:37`), com `self.parameters.push(...)` e depois `register_for(reg)`; o `set_index_virtual` já é feito no corpo, então a linha 503 vira `RegisterID::new` embrulhado em `Rc<RefCell<_>>` (ver como `new_register` da base cria, `base.rs:206`). | `BytecodeGenerator.cpp:1385` |

## 4. Aridade: único desvio

`variable(&mut self, &Identifier, ThisResolutionType)` (`bytecode_generator_cpp3.rs:826`) é chamado com 1 argumento
em 11 pontos: `nodes_codegen_cpp2.rs:58`, `:95`, `:107`, `:257`, `:526`, `nodes_codegen_cpp4.rs:117`,
`bytecode_generator_cpp1.rs:131`, `:144`, `bytecode_generator_cpp2.rs:315`, `:361`, `:388`. O C++ tem argumento
padrão: `Variable variable(const Identifier&, ThisResolutionType = ThisResolutionType::Local)`
(`BytecodeGenerator.h:418`). Rust não tem argumento padrão: ou os 11 chamadores passam
`ThisResolutionType::Local`, ou entra uma segunda função com nome próprio (o repasse proibido pela regra DRY
não vale aqui porque acrescenta argumento, que o `CLAUDE.md` do projeto permite).
