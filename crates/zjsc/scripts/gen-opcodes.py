#!/usr/bin/env python3
"""Gera src/bytecode/opcode.rs (o `Opcode.h` do C++) a partir de derived/JavaScriptCore/Bytecodes.h: o enum
`OpcodeID` na ordem do `FOR_EACH_BYTECODE_ID` (o número do opcode é a posição na lista) e o tamanho
de cada instrução. Uso: scripts/gen-opcodes.py (na raiz do crate).
"""
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def block(text, name):
    start = text.index(f"#define {name}(macro)")
    lines = []
    for line in text[start:].splitlines()[1:]:
        m = re.search(r"macro\((\w+), (\d+)\)", line)
        if m:
            lines.append((m.group(1), int(m.group(2))))
        if not line.rstrip().endswith("\\"):
            break
    return lines


def main():
    with open(os.path.join(ROOT, "derived", "JavaScriptCore", "Bytecodes.h"), encoding="utf-8") as f:
        text = f.read()
    ops = block(text, "FOR_EACH_BYTECODE_ID")
    declared = int(re.search(r"#define NUMBER_OF_BYTECODE_IDS (\d+)", text).group(1))
    assert len(ops) == declared, (len(ops), declared)
    variants = "\n".join(f"    {name} = {i}," for i, (name, _) in enumerate(ops))
    ids = ",\n".join(f"    OpcodeID::{name}" for name, _ in ops)
    lengths = ", ".join(str(n) for _, n in ops)
    names = ",\n".join(f'    "{name}"' for name, _ in ops)
    out = f"""//! Gerado por `scripts/gen-opcodes.py` a partir de `derived/JavaScriptCore/Bytecodes.h`.
//! Não editar à mão.

/// `OpcodeID`: o número de cada opcode é a posição dele no `FOR_EACH_BYTECODE_ID`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum OpcodeID {{
{variants}
}}

/// `NUMBER_OF_BYTECODE_IDS`.
pub const NUMBER_OF_BYTECODE_IDS: usize = {declared};

/// Todo `OpcodeID`, na ordem do número.
pub static OPCODE_IDS: [OpcodeID; NUMBER_OF_BYTECODE_IDS] = [
{ids},
];

impl OpcodeID {{
    /// O `static_cast<OpcodeID>(value)` do C++ (`enum OpcodeID : unsigned`), com o `ASSERT` de faixa.
    pub fn from_u32(value: u32) -> OpcodeID {{
        OPCODE_IDS[value as usize]
    }}
}}

/// Tamanho de cada instrução (opcode mais operandos), na ordem de `OpcodeID`.
pub static OPCODE_LENGTHS: [u8; NUMBER_OF_BYTECODE_IDS] = [{lengths}];

/// Nome de cada opcode, na ordem de `OpcodeID` (o `opcodeNames` do C++).
pub static OPCODE_NAMES: [&str; NUMBER_OF_BYTECODE_IDS] = [
{names},
];
"""
    os.makedirs(os.path.join(ROOT, "src", "bytecode"), exist_ok=True)
    with open(os.path.join(ROOT, "src", "bytecode", "opcode.rs"), "w", encoding="utf-8") as f:
        f.write(out)


main()
