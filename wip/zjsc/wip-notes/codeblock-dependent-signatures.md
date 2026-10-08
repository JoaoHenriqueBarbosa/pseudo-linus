# Assinaturas que dependem de CodeBlock (sem porte ainda)

Não portadas porque dependem de tipos que ainda não existem (`CodeBlock`, `UnlinkedCodeBlockGenerator`,
`SymbolTable`, `JSInstruction`, `BytecodeGenerator` completo). Portar junto com `bytecode/code_block`.

- `CodeBlock::llintBaselineCalleeSaveSpaceAsVirtualRegisters()` (CodeBlock.h:656, CodeBlock.cpp:2721):
  função estática sem estado. Em Linux x86_64 sem JIT (`#else` do .h:660) devolve `1`. O porte é um
  `pub const fn llint_baseline_callee_save_space_as_virtual_registers() -> usize { 1 }` em
  `bytecode::code_block`.
- `computeDefsForBytecodeIndex<Block, Functor>(Block*, const JSInstruction*, Checkpoint, Functor)`
  (BytecodeUseDef.h:49): chama `computeDefsForBytecodeIndexImpl(codeBlock->numVars(), instruction,
  checkpoint, functor)` (BytecodeUseDef.cpp:389). Assinatura Rust:
  `fn compute_defs_for_bytecode_index<B: HasNumVars>(block: &B, instruction: &JSInstruction,
  checkpoint: Checkpoint, functor: &mut dyn FnMut(VirtualRegister))`. Depende de `JSInstruction`
  (decode de `bytecode_ops`) e `Checkpoint`.
- `performGeneratorification(BytecodeGenerator&, UnlinkedCodeBlockGenerator*, JSInstructionStreamWriter&,
  SymbolTable* generatorFrameSymbolTable, int generatorFrameSymbolTableIndex)`
  (BytecodeGeneratorification.cpp:291): `bytecode::bytecode_generatorification`. Depende de
  liveness (`BytecodeLivenessAnalysis`), `SymbolTable` e do decode de todos os opcodes.

Já portados: `speculation_from_string` em `bytecode::speculated_type`.
