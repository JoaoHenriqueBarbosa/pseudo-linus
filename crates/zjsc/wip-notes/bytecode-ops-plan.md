# bytecode::bytecode_ops: inventário e desenho

Fonte: `upstream/JavaScriptCore/bytecode/BytecodeList.rb` (o gerador Ruby produz `BytecodeStructs.h`).
Cada `op` vira uma struct `Op<Nome>` com os campos de `args:` na ordem do `.rb`, `static constexpr
OpcodeID opcodeID`, e (no C++) `emit`, `is`, `as`, `decode`. `metadata:`, `tmps:` e `checkpoints:`
são do interpretador e não entram nesta fatia.

## Op usados em `src/bytecompiler` (contagem de menções, 3 mais frequentes primeiro)

OpStricteq 30, OpNeq 12, OpEq 10, OpNstricteq 9, OpJneqPtr 9, OpBeloweq 9, OpTailCall 8, OpNot 8,
OpLesseq/OpLess/OpGreatereq/OpGreater/OpCall/OpBelow 8, OpUrshift/OpTypeof/OpCallVarargs/
OpCallIgnoreResult/OpCallDirectEval/OpAdd 7, OpTailCallVarargs 6, OpSuperConstructVarargs/OpNeqNull/
OpJmp/OpIsUndefinedOrNull/OpEqNull/OpConstructVarargs 5, OpTypeofIsUndefined/OpToNumber/OpSub/OpRshift/
OpPutToScope/OpMul/OpLshift/OpJnundefinedOrNull/OpDiv/OpBitxor/OpBitor/OpBitand 4, e uma cauda de
cerca de 80 Op com 1 a 3 menções (jumps, `OpNew*`, `OpPut*`, `OpGet*`, `OpTo*`, `OpIterator*`,
`OpSwitch*`, `OpThrow*`, etc.).

Observação: o código existente usa três caminhos para o mesmo conceito:
`crate::bytecode::bytecode_ops::OpX` (maioria), `crate::bytecode::bytecode_list::OpX` (um trecho de
`bytecode_generator_cpp4.rs`) e `crate::bytecode::opcode::OpcodeID::OpCall` / `OpcodeID::OpStricteq`
(variantes CamelCase), enquanto `opcode.rs` gera `op_call`, `op_stricteq` (snake). Falta unificar:
os chamadores devem usar `Op*::OPCODE_ID` ou `OpcodeID::op_*`.

## Assinaturas (campos de `args:`) das 45 implementadas

| Op | opcode | campos |
|---|---|---|
| OpEq, OpNeq, OpStricteq, OpNstricteq, OpLess, OpLesseq, OpGreater, OpGreatereq, OpBelow, OpBeloweq, OpMod, OpPow, OpUrshift (`BinaryOp`) | op_eq ... | dst, lhs, rhs: VirtualRegister |
| OpAdd, OpMul, OpDiv, OpSub, OpBitand, OpBitor, OpBitxor | idem | dst, lhs, rhs, profileIndex: unsigned, operandTypes: OperandTypes |
| OpLshift, OpRshift | idem | dst, lhs, rhs, profileIndex: unsigned |
| OpEqNull, OpNeqNull, OpTypeofIsUndefined, OpIsUndefinedOrNull (`UnaryOp`) | idem | dst, operand |
| OpNot | op_not | dst, operand |
| OpTypeof | op_typeof | dst, value |
| OpToNumber, OpUnsigned (`ProfiledUnaryOp`) | idem | dst, operand, profileIndex: unsigned |
| OpMov | op_mov | dst, src |
| OpEnter | op_enter | (nenhum) |
| OpJmp | op_jmp | targetLabel: BoundLabel |
| OpJneqPtr | op_jneq_ptr | value, specialPointer: VirtualRegister, targetLabel: BoundLabel |
| OpCall, OpConstruct, OpSuperConstruct | idem | dst, callee, argc: unsigned, argv: unsigned, valueProfile: unsigned |
| OpCallIgnoreResult | op_call_ignore_result | callee, argc, argv (sem dst) |
| OpTailCall | op_tail_call | dst, callee, argc, argv (sem valueProfile) |
| OpCallDirectEval | op_call_direct_eval | dst, callee, argc, argv, thisValue, scope: VirtualRegister, lexicallyScopedFeatures: unsigned, valueProfile: unsigned |
| OpCallVarargs, OpConstructVarargs, OpSuperConstructVarargs | idem | dst, callee, thisValue?, arguments?, firstFree: VirtualRegister, firstVarArg: int, valueProfile: unsigned |
| OpTailCallVarargs | op_tail_call_varargs | idem sem valueProfile |
| OpPutToScope | op_put_to_scope | scope, var: unsigned, value, getPutInfo: GetPutInfo, symbolTableOrScopeDepth: SymbolTableOrScopeDepth, offset: unsigned |

## Desenho Rust

- Módulo `crate::bytecode::bytecode_ops` (`src/bytecode/bytecode_ops.rs`): uma `pub struct` por opcode,
  campos `pub` em snake_case, gerada por macros que espelham os `op_group` do `.rb` (`binary_op!`,
  `unary_op!`, `varargs_op!`, ...), de modo que a repetição do `.rb` não vira cópia.
- `trait BytecodeOp { const OPCODE_ID: OpcodeID; }`, o `opcodeID` do C++, implementado por todas.
- Tipos: `VirtualRegister` (existente), `OperandTypes` (`parser::result_type`), `BoundLabel` =
  `GenericBoundLabel<JSGeneratorTraits>` (`bytecompiler::label`), `unsigned` = `u32`, `int` = `i32`.
  `GetPutInfo` e `SymbolTableOrScopeDepth` são `unsigned` empacotado no C++, por ora `u32`
  (trocar quando `runtime/get_put_info` for liberado para edição). `thisValue?`/`arguments?`
  continuam `VirtualRegister`: o `?` só admite o valor inválido na codificação.
- Próxima fatia, `emit`: `impl OpX { pub fn emit(gen: &mut BytecodeGenerator, campos...) }` que
  (1) aplica `gen.rewind`/peephole como o `emit` gerado (`BytecodeGeneratorBase::alignWideOpcode`,
  `m_writer.write`), (2) escolhe `Narrow`/`Wide16`/`Wide32` pela largura dos operandos e
  (3) codifica `BoundLabel` pelo offset. Mais `is_op::<T>()`/`as_op::<T>()` em `Instruction`, que o
  `bytecode_generator` já espera. A forma do `emit` hoje chamada (`OpMov::emit(self, Some(dst),
  Some(src))`) usa `Option<RegisterID>`: converter na costura (`VirtualRegister::from_register_id`).
- Conferido em 2026-10-08 contra `BytecodeList.rb` (campos, ordem, tipos) e contra os `Op*::emit(`
  do bytecompiler (nº e ordem dos argumentos): sem divergência de aridade ou ordem. A cauda inteira
  entrou em `bytecode_ops.rs` (todos os `op` e `op_group` do `.rb` da seção `Bytecode`, menos os
  `llint_*`/`wide*` de helpers), via macros por `op_group` (`binary_jmp!`, `switch_value!`,
  `new_function!`, `profiled_unary_op!`, `unary_in_place_profiled_op!`, `unary_jmp!`) e `plain_ops!`.
  Todo `OpcodeID` usado existe em `opcode.rs`.
- Tipos ainda sem porte, por isso `u8`/`u32` nos campos: `JSType` (`runtime::js_type`, o bytecompiler
  já importa esse caminho), `PutByIdFlags` (`bytecode::put_by_id_flags`), `PrivateFieldPutKind`
  (`bytecode::put_kind`), `SymbolTableOrScopeDepth` (`bytecode::bytecode_list::` no bytecompiler).
  `GetPutInfo` agora é o tipo real (`runtime::get_put_info`) em `OpPutToScope` e `OpGetFromScope`.
- Fatia `emit` (feita em 2026-10-08, sem compilar: a bancada não rodou o build). `bytecode_op!` gera,
  por Op, `emit`, `emit_with_smallest_size_requirement(size)` e `emit_at_size(size, should_assert)`
  (`Opcode#emitter` do `.rb`); o motor é único: `emit_impl` (`checkImpl` com os `Fits::check` na
  ordem, `recordOpcode`, `writeOpcode<size>`), mais `has_metadata` (49 `op` + o grupo
  `CreateInternalFieldObjectOp`, acrescenta o `__metadataID`) e `has_checkpoints` (8 `op`).
  `Fits.h` está na seção `Operand` do próprio arquivo (VirtualRegister com
  `FirstConstantRegisterIndex8/16`, GetPutInfo, OperandTypes, BoundLabel com `saveTarget` e
  `commitTarget`, inteiros, enums); deve migrar para `bytecode/fits.rs`.
- O que falta no gerador: `impl OpWriter for BytecodeGenerator` (`record_opcode` e `write_opcode` da
  base, `add_metadata_for`, `set_uses_checkpoints`). Os argumentos de registrador aceitam
  `Option<RegisterIDRef>` direto (`IntoOperand`), então `emit(self, Some(dst), Some(src))` funciona.
- Nome fiel: não existe `emit_with_profile*` no C++. Lá é `emit(gen, ..., profileIndex[, operandTypes |
  resultType])`. Os chamadores concretos devem trocar: `OpInc/OpDec/OpToNumber/OpToNumeric/OpBitnot::
  emit_with_profile` e `OpNegate::emit_with_profile_and_type` viram `emit` com os mesmos argumentos
  (`bytecode_generator_cpp2.rs` 1075 a 1216). Os genéricos `emit_unary_op`/`emit_binary_op` usam os
  traits `UnaryOpcode`/`BinaryOpcode` (o `if constexpr` do C++): `emit`, `emit_with_profile` e
  `emit_with_profile_and_types` existem só no braço que a op tem.
- Pendências: `checkWithoutMetadataID` não portado (ninguém no bytecompiler usa); `OpPutById::flags`
  e `OpPutPrivateName::put_kind` ainda `u8`: com `bytecode/put_by_id_flags.rs` e `put_kind.rs`
  agora existindo, trocar o tipo e dar `Operand` (PutByIdFlags: `isStrict << 1 | isDirect`).
- Divergências de API do bytecompiler para a fatia `emit` (anteriores): `OpInc/OpDec/OpToNumber/OpBitnot::emit_with_profile`
  e `OpNegate::emit_with_profile_and_type` não existem no C++ (lá é `emit(gen, ..., profileIndex[, resultType])`);
  `bytecode_list::OpX` e `bytecode_ops::OpX` coexistem; `OpPutById::emit` recebe `PutByIdFlags::create(..)`.
  Os campos `?` (`identifier?`, `thisValue?`) são `VirtualRegister`/`u32` e os chamadores passam `Option`.
