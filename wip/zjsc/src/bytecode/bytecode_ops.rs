//! Porte das structs `Op*` que o gerador Ruby (`bytecode/generator/`) produz a partir de
//! `bytecode/BytecodeList.rb` em `BytecodeStructs.h`.
//!
//! Nesta fatia só existem os campos (`args:` do `.rb`, na mesma ordem e com os mesmos tipos).
//! Metadata (`metadata:`), `tmps:` e `checkpoints:` pertencem ao interpretador e não entram no
//! que o `BytecodeGenerator` escreve. A lógica de emissão (`emit`, `is`, `as`) vem numa fatia
//! seguinte.
//!
//! Tipos do `.rb` sem porte próprio ainda:
//! - `GetPutInfo` e `SymbolTableOrScopeDepth` são `unsigned` empacotado no C++ (`m_operand`), por
//!   isso viram `u32` aqui.
//! - `BoundLabel` é `GenericBoundLabel<JSGeneratorTraits>`.
//! - Operando marcado com `?` no `.rb` (`thisValue?`) continua `VirtualRegister`; o `?` só permite
//!   o valor inválido na codificação.

use crate::bytecode::opcode::OpcodeID;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::bytecompiler::label::{GenericBoundLabel, JSGeneratorTraits};
use crate::parser::result_type::OperandTypes;

/// `BoundLabel` do `.rb`.
pub type BoundLabel = GenericBoundLabel<JSGeneratorTraits>;

/// O que toda struct `Op*` gerada expõe: `static constexpr OpcodeID opcodeID`.
pub trait BytecodeOp {
    const OPCODE_ID: OpcodeID;
}

/// Gera uma struct `Op*` com os campos do `args:` e o `opcodeID`.
macro_rules! bytecode_op {
    ($(#[$meta:meta])* $name:ident, $id:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        pub struct $name {
            $(pub $field: $ty,)*
        }

        impl BytecodeOp for $name {
            const OPCODE_ID: OpcodeID = OpcodeID::$id;
        }
    };
}

/// `op_group :BinaryOp`: `dst`, `lhs`, `rhs`.
macro_rules! binary_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id { dst: VirtualRegister, lhs: VirtualRegister, rhs: VirtualRegister }
        );)*
    };
}

/// `op_group :ProfiledBinaryOpWithOperandTypes`: `BinaryOp` mais `profileIndex` e `operandTypes`.
macro_rules! profiled_binary_op_with_operand_types {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id {
                dst: VirtualRegister,
                lhs: VirtualRegister,
                rhs: VirtualRegister,
                profile_index: u32,
                operand_types: OperandTypes,
            }
        );)*
    };
}

/// `op_group :ProfiledBinaryOp`: `BinaryOp` mais `profileIndex`.
macro_rules! profiled_binary_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id {
                dst: VirtualRegister,
                lhs: VirtualRegister,
                rhs: VirtualRegister,
                profile_index: u32,
            }
        );)*
    };
}

/// `op_group :UnaryOp`: `dst`, `operand`.
macro_rules! unary_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id { dst: VirtualRegister, operand: VirtualRegister }
        );)*
    };
}

/// `op_call_varargs` e variações: o `.rb` repete os mesmos `args:` (`valueProfile` em todas,
/// menos em `tail_call_varargs`).
macro_rules! varargs_op {
    ($($name:ident => $id:ident),* $(,)?) => {
        $(bytecode_op!(
            #[derive(Clone, Debug, PartialEq, Eq)]
            $name, $id {
                dst: VirtualRegister,
                callee: VirtualRegister,
                this_value: VirtualRegister,
                arguments: VirtualRegister,
                first_free: VirtualRegister,
                first_var_arg: i32,
                value_profile: u32,
            }
        );)*
    };
}

binary_op! {
    OpEq => op_eq,
    OpNeq => op_neq,
    OpStricteq => op_stricteq,
    OpNstricteq => op_nstricteq,
    OpLess => op_less,
    OpLesseq => op_lesseq,
    OpGreater => op_greater,
    OpGreatereq => op_greatereq,
    OpBelow => op_below,
    OpBeloweq => op_beloweq,
    OpMod => op_mod,
    OpPow => op_pow,
    OpUrshift => op_urshift,
}

profiled_binary_op_with_operand_types! {
    OpAdd => op_add,
    OpMul => op_mul,
    OpDiv => op_div,
    OpSub => op_sub,
    OpBitand => op_bitand,
    OpBitor => op_bitor,
    OpBitxor => op_bitxor,
}

profiled_binary_op! {
    OpLshift => op_lshift,
    OpRshift => op_rshift,
}

unary_op! {
    OpEqNull => op_eq_null,
    OpNeqNull => op_neq_null,
    OpTypeofIsUndefined => op_typeof_is_undefined,
    OpIsUndefinedOrNull => op_is_undefined_or_null,
}

varargs_op! {
    OpCallVarargs => op_call_varargs,
    OpConstructVarargs => op_construct_varargs,
    OpSuperConstructVarargs => op_super_construct_varargs,
}

// `op :tail_call_varargs` não tem `valueProfile`.
bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpTailCallVarargs, op_tail_call_varargs {
        dst: VirtualRegister,
        callee: VirtualRegister,
        this_value: VirtualRegister,
        arguments: VirtualRegister,
        first_free: VirtualRegister,
        first_var_arg: i32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpCall, op_call {
        dst: VirtualRegister,
        callee: VirtualRegister,
        argc: u32,
        argv: u32,
        value_profile: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpConstruct, op_construct {
        dst: VirtualRegister,
        callee: VirtualRegister,
        argc: u32,
        argv: u32,
        value_profile: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpSuperConstruct, op_super_construct {
        dst: VirtualRegister,
        callee: VirtualRegister,
        argc: u32,
        argv: u32,
        value_profile: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpCallIgnoreResult, op_call_ignore_result {
        callee: VirtualRegister,
        argc: u32,
        argv: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpTailCall, op_tail_call {
        dst: VirtualRegister,
        callee: VirtualRegister,
        argc: u32,
        argv: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpCallDirectEval, op_call_direct_eval {
        dst: VirtualRegister,
        callee: VirtualRegister,
        argc: u32,
        argv: u32,
        this_value: VirtualRegister,
        scope: VirtualRegister,
        lexically_scoped_features: u32,
        value_profile: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpNot, op_not { dst: VirtualRegister, operand: VirtualRegister }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpTypeof, op_typeof { dst: VirtualRegister, value: VirtualRegister }
);

// `op_group :ProfiledUnaryOp` (`to_number`, `to_numeric`, `bitnot`, `unsigned`).
bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpToNumber, op_to_number {
        dst: VirtualRegister,
        operand: VirtualRegister,
        profile_index: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpUnsigned, op_unsigned {
        dst: VirtualRegister,
        operand: VirtualRegister,
        profile_index: u32,
    }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpMov, op_mov { dst: VirtualRegister, src: VirtualRegister }
);

bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpPutToScope, op_put_to_scope {
        scope: VirtualRegister,
        var: u32,
        value: VirtualRegister,
        get_put_info: u32,
        symbol_table_or_scope_depth: u32,
        offset: u32,
    }
);

// Ops com `BoundLabel` (sem `Clone`/`PartialEq`, o `GenericBoundLabel` não os tem).
bytecode_op!(
    OpJmp, op_jmp { target_label: BoundLabel }
);

bytecode_op!(
    OpJneqPtr, op_jneq_ptr {
        value: VirtualRegister,
        special_pointer: VirtualRegister,
        target_label: BoundLabel,
    }
);

// `op :enter` não tem `args:`.
bytecode_op!(
    #[derive(Clone, Debug, PartialEq, Eq)]
    OpEnter, op_enter {}
);
