//! Tabela de opcodes do CPython 3.13 (`Include/opcode_ids.h` e `Include/internal/pycore_opcode_metadata.h`):
//! número, marcas (`has_arg`, `has_const`, ...), entradas de CACHE em linha e efeito na pilha.
//!
//! É a fonte única do `_opcode` nativo e do emissor de bytecode (`cpybc`). Os nomes e números batem com o
//! `_opcode_metadata.py` do Debian, que o `opcode.py` real importa.

pub const ARG: u16 = 1;
pub const CONST: u16 = 2;
pub const NAME: u16 = 4;
pub const JUMP: u16 = 8;
pub const FREE: u16 = 16;
pub const LOCAL: u16 = 32;
pub const EXC: u16 = 64;

/// Primeiro opcode pseudo (só existem dentro do compilador; o `opmap` os expõe mesmo assim).
pub const MIN_PSEUDO: u16 = 256;
pub const EXTENDED_ARG: u8 = 71;

/// Uma linha da tabela de opcodes.
#[derive(Debug, Clone, Copy)]
pub struct OpInfo {
    pub name: &'static str,
    pub code: u16,
    pub flags: u16,
    /// Entradas de CACHE que seguem a instrução (unidades de 2 bytes).
    pub caches: u8,
}

const fn op(name: &'static str, code: u16, flags: u16, caches: u8) -> OpInfo {
    OpInfo { name, code, flags, caches }
}

/// Todos os opcodes não especializados do 3.13, na ordem do `opmap` do Debian.
pub const OPS: &[OpInfo] = &[
    op("CACHE", 0, 0, 0),
    op("BEFORE_ASYNC_WITH", 1, 0, 0),
    op("BEFORE_WITH", 2, 0, 0),
    op("BINARY_SLICE", 4, 0, 0),
    op("BINARY_SUBSCR", 5, 0, 1),
    op("CHECK_EG_MATCH", 6, 0, 0),
    op("CHECK_EXC_MATCH", 7, 0, 0),
    op("CLEANUP_THROW", 8, 0, 0),
    op("DELETE_SUBSCR", 9, 0, 0),
    op("END_ASYNC_FOR", 10, 0, 0),
    op("END_FOR", 11, 0, 0),
    op("END_SEND", 12, 0, 0),
    op("EXIT_INIT_CHECK", 13, 0, 0),
    op("FORMAT_SIMPLE", 14, 0, 0),
    op("FORMAT_WITH_SPEC", 15, 0, 0),
    op("GET_AITER", 16, 0, 0),
    op("RESERVED", 17, 0, 0),
    op("GET_ANEXT", 18, 0, 0),
    op("GET_ITER", 19, 0, 0),
    op("GET_LEN", 20, 0, 0),
    op("GET_YIELD_FROM_ITER", 21, 0, 0),
    op("INTERPRETER_EXIT", 22, 0, 0),
    op("LOAD_ASSERTION_ERROR", 23, 0, 0),
    op("LOAD_BUILD_CLASS", 24, 0, 0),
    op("LOAD_LOCALS", 25, 0, 0),
    op("MAKE_FUNCTION", 26, 0, 0),
    op("MATCH_KEYS", 27, 0, 0),
    op("MATCH_MAPPING", 28, 0, 0),
    op("MATCH_SEQUENCE", 29, 0, 0),
    op("NOP", 30, 0, 0),
    op("POP_EXCEPT", 31, 0, 0),
    op("POP_TOP", 32, 0, 0),
    op("PUSH_EXC_INFO", 33, 0, 0),
    op("PUSH_NULL", 34, 0, 0),
    op("RETURN_GENERATOR", 35, 0, 0),
    op("RETURN_VALUE", 36, 0, 0),
    op("SETUP_ANNOTATIONS", 37, 0, 0),
    op("STORE_SLICE", 38, 0, 0),
    op("STORE_SUBSCR", 39, 0, 1),
    op("TO_BOOL", 40, 0, 3),
    op("UNARY_INVERT", 41, 0, 0),
    op("UNARY_NEGATIVE", 42, 0, 0),
    op("UNARY_NOT", 43, 0, 0),
    op("WITH_EXCEPT_START", 44, 0, 0),
    op("BINARY_OP", 45, ARG, 1),
    op("BUILD_CONST_KEY_MAP", 46, ARG, 0),
    op("BUILD_LIST", 47, ARG, 0),
    op("BUILD_MAP", 48, ARG, 0),
    op("BUILD_SET", 49, ARG, 0),
    op("BUILD_SLICE", 50, ARG, 0),
    op("BUILD_STRING", 51, ARG, 0),
    op("BUILD_TUPLE", 52, ARG, 0),
    op("CALL", 53, ARG, 3),
    op("CALL_FUNCTION_EX", 54, ARG, 0),
    op("CALL_INTRINSIC_1", 55, ARG, 0),
    op("CALL_INTRINSIC_2", 56, ARG, 0),
    op("CALL_KW", 57, ARG, 0),
    op("COMPARE_OP", 58, ARG, 1),
    op("CONTAINS_OP", 59, ARG, 1),
    op("CONVERT_VALUE", 60, ARG, 0),
    op("COPY", 61, ARG, 0),
    op("COPY_FREE_VARS", 62, ARG, 0),
    op("DELETE_ATTR", 63, ARG | NAME, 0),
    op("DELETE_DEREF", 64, ARG | FREE, 0),
    op("DELETE_FAST", 65, ARG | LOCAL, 0),
    op("DELETE_GLOBAL", 66, ARG | NAME, 0),
    op("DELETE_NAME", 67, ARG | NAME, 0),
    op("DICT_MERGE", 68, ARG, 0),
    op("DICT_UPDATE", 69, ARG, 0),
    op("ENTER_EXECUTOR", 70, ARG, 0),
    op("EXTENDED_ARG", 71, ARG, 0),
    op("FOR_ITER", 72, ARG | JUMP, 1),
    op("GET_AWAITABLE", 73, ARG, 0),
    op("IMPORT_FROM", 74, ARG | NAME, 0),
    op("IMPORT_NAME", 75, ARG | NAME, 0),
    op("IS_OP", 76, ARG, 0),
    op("JUMP_BACKWARD", 77, ARG | JUMP, 1),
    op("JUMP_BACKWARD_NO_INTERRUPT", 78, ARG | JUMP, 0),
    op("JUMP_FORWARD", 79, ARG | JUMP, 0),
    op("LIST_APPEND", 80, ARG, 0),
    op("LIST_EXTEND", 81, ARG, 0),
    op("LOAD_ATTR", 82, ARG | NAME, 9),
    op("LOAD_CONST", 83, ARG | CONST, 0),
    op("LOAD_DEREF", 84, ARG | FREE, 0),
    op("LOAD_FAST", 85, ARG | LOCAL, 0),
    op("LOAD_FAST_AND_CLEAR", 86, ARG | LOCAL, 0),
    op("LOAD_FAST_CHECK", 87, ARG | LOCAL, 0),
    op("LOAD_FAST_LOAD_FAST", 88, ARG | LOCAL, 0),
    op("LOAD_FROM_DICT_OR_DEREF", 89, ARG | FREE, 0),
    op("LOAD_FROM_DICT_OR_GLOBALS", 90, ARG | NAME, 0),
    op("LOAD_GLOBAL", 91, ARG | NAME, 4),
    op("LOAD_NAME", 92, ARG | NAME, 0),
    op("LOAD_SUPER_ATTR", 93, ARG | NAME, 1),
    op("MAKE_CELL", 94, ARG | FREE, 0),
    op("MAP_ADD", 95, ARG, 0),
    op("MATCH_CLASS", 96, ARG, 0),
    op("POP_JUMP_IF_FALSE", 97, ARG | JUMP, 1),
    op("POP_JUMP_IF_NONE", 98, ARG | JUMP, 1),
    op("POP_JUMP_IF_NOT_NONE", 99, ARG | JUMP, 1),
    op("POP_JUMP_IF_TRUE", 100, ARG | JUMP, 1),
    op("RAISE_VARARGS", 101, ARG, 0),
    op("RERAISE", 102, ARG, 0),
    op("RETURN_CONST", 103, ARG | CONST, 0),
    op("SEND", 104, ARG | JUMP, 1),
    op("SET_ADD", 105, ARG, 0),
    op("SET_FUNCTION_ATTRIBUTE", 106, ARG, 0),
    op("SET_UPDATE", 107, ARG, 0),
    op("STORE_ATTR", 108, ARG | NAME, 4),
    op("STORE_DEREF", 109, ARG | FREE, 0),
    op("STORE_FAST", 110, ARG | LOCAL, 0),
    op("STORE_FAST_LOAD_FAST", 111, ARG | LOCAL, 0),
    op("STORE_FAST_STORE_FAST", 112, ARG | LOCAL, 0),
    op("STORE_GLOBAL", 113, ARG | NAME, 0),
    op("STORE_NAME", 114, ARG | NAME, 0),
    op("SWAP", 115, ARG, 0),
    op("UNPACK_EX", 116, ARG, 0),
    op("UNPACK_SEQUENCE", 117, ARG, 1),
    op("YIELD_VALUE", 118, ARG, 0),
    op("RESUME", 149, ARG, 0),
    op("INSTRUMENTED_RESUME", 236, ARG, 0),
    op("INSTRUMENTED_END_FOR", 237, 0, 0),
    op("INSTRUMENTED_END_SEND", 238, 0, 0),
    op("INSTRUMENTED_RETURN_VALUE", 239, 0, 0),
    op("INSTRUMENTED_RETURN_CONST", 240, ARG | CONST, 0),
    op("INSTRUMENTED_YIELD_VALUE", 241, ARG, 0),
    op("INSTRUMENTED_LOAD_SUPER_ATTR", 242, ARG, 1),
    op("INSTRUMENTED_FOR_ITER", 243, ARG, 1),
    op("INSTRUMENTED_CALL", 244, ARG, 3),
    op("INSTRUMENTED_CALL_KW", 245, ARG, 0),
    op("INSTRUMENTED_CALL_FUNCTION_EX", 246, 0, 0),
    op("INSTRUMENTED_INSTRUCTION", 247, 0, 0),
    op("INSTRUMENTED_JUMP_FORWARD", 248, ARG, 0),
    op("INSTRUMENTED_JUMP_BACKWARD", 249, ARG, 1),
    op("INSTRUMENTED_POP_JUMP_IF_TRUE", 250, ARG, 1),
    op("INSTRUMENTED_POP_JUMP_IF_FALSE", 251, ARG, 1),
    op("INSTRUMENTED_POP_JUMP_IF_NONE", 252, ARG, 1),
    op("INSTRUMENTED_POP_JUMP_IF_NOT_NONE", 253, ARG, 1),
    op("INSTRUMENTED_LINE", 254, 0, 0),
    op("JUMP", 256, ARG | JUMP, 0),
    op("JUMP_NO_INTERRUPT", 257, ARG | JUMP, 0),
    op("LOAD_CLOSURE", 258, ARG | LOCAL, 0),
    op("LOAD_METHOD", 259, ARG | NAME, 0),
    op("LOAD_SUPER_METHOD", 260, ARG | NAME, 0),
    op("LOAD_ZERO_SUPER_ATTR", 261, ARG | NAME, 0),
    op("LOAD_ZERO_SUPER_METHOD", 262, ARG | NAME, 0),
    op("POP_BLOCK", 263, 0, 0),
    op("SETUP_CLEANUP", 264, ARG | EXC, 0),
    op("SETUP_FINALLY", 265, ARG | EXC, 0),
    op("SETUP_WITH", 266, ARG | EXC, 0),
    op("STORE_FAST_MAYBE_NULL", 267, ARG | LOCAL, 0),
];

/// A linha da tabela de um opcode, ou `None` se o número não é um opcode do 3.13.
pub fn info(code: i64) -> Option<&'static OpInfo> {
    let code = u16::try_from(code).ok()?;
    OPS.iter().find(|o| o.code == code)
}

/// Entradas de CACHE em linha de um opcode real.
pub fn caches_of(code: u8) -> usize {
    info(i64::from(code)).map_or(0, |o| usize::from(o.caches))
}

/// Efeito na pilha (`stack_effect` do `_opcode`). `None`: opcode sem efeito definido.
pub fn stack_effect(name: &str, oparg: i64, jump: bool) -> Option<i64> {
    let bit = oparg & 1;
    Some(match name {
        "CACHE" | "NOP" | "RESERVED" | "FORMAT_SIMPLE" | "GET_AITER" | "GET_ITER" | "GET_YIELD_FROM_ITER"
        | "MAKE_FUNCTION" | "SETUP_ANNOTATIONS" | "TO_BOOL" | "UNARY_INVERT" | "UNARY_NEGATIVE" | "UNARY_NOT"
        | "CALL_INTRINSIC_1" | "CONVERT_VALUE" | "COPY_FREE_VARS" | "DELETE_DEREF" | "DELETE_FAST"
        | "DELETE_GLOBAL" | "DELETE_NAME" | "ENTER_EXECUTOR" | "EXTENDED_ARG" | "GET_AWAITABLE" | "JUMP_BACKWARD"
        | "JUMP_BACKWARD_NO_INTERRUPT" | "JUMP_FORWARD" | "LOAD_FROM_DICT_OR_DEREF" | "LOAD_FROM_DICT_OR_GLOBALS"
        | "MAKE_CELL" | "SEND" | "STORE_FAST_LOAD_FAST" | "SWAP" | "YIELD_VALUE" | "RESUME"
        | "INSTRUMENTED_RESUME" | "INSTRUMENTED_YIELD_VALUE"
        | "INSTRUMENTED_INSTRUCTION" | "INSTRUMENTED_LINE" | "INSTRUMENTED_JUMP_FORWARD"
        | "INSTRUMENTED_JUMP_BACKWARD" | "JUMP" | "JUMP_NO_INTERRUPT" | "POP_BLOCK" | "CHECK_EG_MATCH"
        | "CHECK_EXC_MATCH" => 0,
        // `RETURN_GENERATOR` medido no oráculo (1 com `jump` ausente, falso e verdadeiro); `RETURN_CONST` não foi
        // medido, mas o `co_stacksize` de `def h(): pass` e do módulo vazio é 1, o que só fecha com efeito +1.
        "RETURN_GENERATOR" | "RETURN_CONST" | "INSTRUMENTED_RETURN_CONST" | "BEFORE_ASYNC_WITH" | "BEFORE_WITH"
        | "GET_ANEXT" | "GET_LEN" | "LOAD_ASSERTION_ERROR" | "LOAD_BUILD_CLASS"
        | "LOAD_LOCALS" | "MATCH_KEYS" | "MATCH_MAPPING" | "MATCH_SEQUENCE" | "PUSH_EXC_INFO" | "PUSH_NULL"
        | "WITH_EXCEPT_START" | "COPY" | "IMPORT_FROM" | "LOAD_CONST" | "LOAD_DEREF" | "LOAD_FAST"
        | "LOAD_FAST_AND_CLEAR" | "LOAD_FAST_CHECK" | "LOAD_NAME" | "LOAD_CLOSURE" | "LOAD_METHOD" => 1,
        "LOAD_FAST_LOAD_FAST" => 2,
        "BINARY_SUBSCR" | "BINARY_OP" | "CALL_INTRINSIC_2" | "COMPARE_OP" | "CONTAINS_OP" | "DELETE_ATTR"
        | "DICT_MERGE" | "DICT_UPDATE" | "FORMAT_WITH_SPEC" | "IMPORT_NAME" | "IS_OP" | "LIST_APPEND"
        | "LIST_EXTEND" | "POP_JUMP_IF_FALSE" | "POP_JUMP_IF_NONE" | "POP_JUMP_IF_NOT_NONE"
        | "POP_JUMP_IF_TRUE" | "RERAISE" | "SET_ADD" | "SET_FUNCTION_ATTRIBUTE" | "SET_UPDATE" | "STORE_DEREF"
        | "STORE_FAST" | "STORE_GLOBAL" | "STORE_NAME" | "STORE_FAST_MAYBE_NULL" | "CLEANUP_THROW" | "END_FOR"
        | "END_SEND" | "EXIT_INIT_CHECK" | "POP_EXCEPT" | "POP_TOP" | "RETURN_VALUE" | "INTERPRETER_EXIT"
        | "INSTRUMENTED_END_FOR" | "INSTRUMENTED_END_SEND" | "INSTRUMENTED_RETURN_VALUE"
        | "INSTRUMENTED_POP_JUMP_IF_TRUE" | "INSTRUMENTED_POP_JUMP_IF_FALSE" | "INSTRUMENTED_POP_JUMP_IF_NONE"
        | "INSTRUMENTED_POP_JUMP_IF_NOT_NONE" | "LOAD_SUPER_METHOD" | "LOAD_ZERO_SUPER_METHOD" => -1,
        "BINARY_SLICE" | "MAP_ADD" | "MATCH_CLASS" | "STORE_ATTR" | "STORE_FAST_STORE_FAST" | "DELETE_SUBSCR"
        | "END_ASYNC_FOR" | "LOAD_ZERO_SUPER_ATTR" => -2,
        "STORE_SLICE" => -4,
        "STORE_SUBSCR" => -3,
        "BUILD_CONST_KEY_MAP" => -oparg,
        "BUILD_LIST" | "BUILD_SET" | "BUILD_STRING" | "BUILD_TUPLE" => 1 - oparg,
        "BUILD_MAP" => 1 - 2 * oparg,
        "BUILD_SLICE" => {
            if oparg == 3 {
                -2
            } else {
                -1
            }
        }
        "CALL" | "INSTRUMENTED_CALL" => -1 - oparg,
        "CALL_KW" | "INSTRUMENTED_CALL_KW" => -2 - oparg,
        "CALL_FUNCTION_EX" | "INSTRUMENTED_CALL_FUNCTION_EX" => -2 - bit,
        "FOR_ITER" | "INSTRUMENTED_FOR_ITER" => 1,
        "LOAD_ATTR" => bit,
        "LOAD_GLOBAL" => 1 + bit,
        "LOAD_SUPER_ATTR" | "INSTRUMENTED_LOAD_SUPER_ATTR" => -2 + bit,
        "RAISE_VARARGS" => -oparg,
        "UNPACK_EX" => (oparg & 0xFF) + (oparg >> 8),
        "UNPACK_SEQUENCE" => oparg - 1,
        "SETUP_FINALLY" | "SETUP_WITH" => i64::from(jump),
        "SETUP_CLEANUP" => 2 * i64::from(jump),
        _ => return None,
    })
}

/// `stack_effect(opcode, oparg, jump)` do `_opcode`: com `jump` ausente, o maior dos dois casos.
pub fn stack_effect_any(name: &str, oparg: i64, jump: Option<bool>) -> Option<i64> {
    match jump {
        Some(j) => stack_effect(name, oparg, j),
        None => Some(stack_effect(name, oparg, false)?.max(stack_effect(name, oparg, true)?)),
    }
}

/// Nomes de `INTRINSIC_1_*`, na ordem dos números.
pub const INTRINSIC_1: &[&str] = &[
    "INTRINSIC_1_INVALID",
    "INTRINSIC_PRINT",
    "INTRINSIC_IMPORT_STAR",
    "INTRINSIC_STOPITERATION_ERROR",
    "INTRINSIC_ASYNC_GEN_WRAP",
    "INTRINSIC_UNARY_POSITIVE",
    "INTRINSIC_LIST_TO_TUPLE",
    "INTRINSIC_TYPEVAR",
    "INTRINSIC_PARAMSPEC",
    "INTRINSIC_TYPEVARTUPLE",
    "INTRINSIC_SUBSCRIPT_GENERIC",
    "INTRINSIC_TYPEALIAS",
];

/// Nomes de `INTRINSIC_2_*`, na ordem dos números.
pub const INTRINSIC_2: &[&str] = &[
    "INTRINSIC_2_INVALID",
    "INTRINSIC_PREP_RERAISE_STAR",
    "INTRINSIC_TYPEVAR_WITH_BOUND",
    "INTRINSIC_TYPEVAR_WITH_CONSTRAINTS",
    "INTRINSIC_SET_FUNCTION_TYPE_PARAMS",
    "INTRINSIC_SET_TYPEPARAM_DEFAULT",
];

/// `(nome, símbolo)` de cada operador de `BINARY_OP`, na ordem do `oparg` (`NB_*`).
pub const NB_OPS: &[(&str, &str)] = &[
    ("NB_ADD", "+"),
    ("NB_AND", "&"),
    ("NB_FLOOR_DIVIDE", "//"),
    ("NB_LSHIFT", "<<"),
    ("NB_MATRIX_MULTIPLY", "@"),
    ("NB_MULTIPLY", "*"),
    ("NB_REMAINDER", "%"),
    ("NB_OR", "|"),
    ("NB_POWER", "**"),
    ("NB_RSHIFT", ">>"),
    ("NB_SUBTRACT", "-"),
    ("NB_TRUE_DIVIDE", "/"),
    ("NB_XOR", "^"),
    ("NB_INPLACE_ADD", "+="),
    ("NB_INPLACE_AND", "&="),
    ("NB_INPLACE_FLOOR_DIVIDE", "//="),
    ("NB_INPLACE_LSHIFT", "<<="),
    ("NB_INPLACE_MATRIX_MULTIPLY", "@="),
    ("NB_INPLACE_MULTIPLY", "*="),
    ("NB_INPLACE_REMAINDER", "%="),
    ("NB_INPLACE_OR", "|="),
    ("NB_INPLACE_POWER", "**="),
    ("NB_INPLACE_RSHIFT", ">>="),
    ("NB_INPLACE_SUBTRACT", "-="),
    ("NB_INPLACE_TRUE_DIVIDE", "/="),
    ("NB_INPLACE_XOR", "^="),
];
