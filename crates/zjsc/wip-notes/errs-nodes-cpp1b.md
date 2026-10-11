# Erros de nodes_codegen_cpp1b.rs que dependem de outro arquivo

- `Expression::is_pure(generator)` (usado em `BracketAccessorNode::emit_bytecode`): vem do despacho
  `impl Expression` do outro agente; hoje só existe `is_pure` por struct em `nodes_codegen_cpp1.rs:341`.
- `PropertyNode::is_underscore_proto_setter(vm, &PropertyNode)`: não conferido contra `parser/nodes.rs`.
- Os erros de `Rc<RefCell<RegisterID>>` vs `RegisterRef` do arquivo de medição são anteriores ao modelo
  unificado; as chamadas já passam `Option<RegisterRef>` (a medição precisa ser refeita).
