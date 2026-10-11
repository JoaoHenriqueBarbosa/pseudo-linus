#!/usr/bin/env python3
"""Gera src/bytecode/bytecode_intrinsics_table.rs a partir de bytecode/BytecodeIntrinsicRegistry.h
(as listas `JSC_COMMON_BYTECODE_INTRINSIC_*_EACH_NAME`), de bytecode/LinkTimeConstant.h
(`JSC_FOREACH_LINK_TIME_CONSTANTS`) e de derived/JavaScriptCore/JSCBuiltins.h
(`JSC_FOREACH_BUILTIN_LINK_TIME_CONSTANT`). Produz o enum `BytecodeIntrinsicEmitter` (um
`emit_intrinsic_<name>` por intrínseco), o enum `LinkTimeConstant` na ordem do C++ e as tabelas de
nomes. Uso: scripts/gen-bytecode-intrinsics.py (na raiz do crate).
"""
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
UPSTREAM = os.path.join(ROOT, "upstream", "JavaScriptCore", "bytecode")


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def block(text, name):
    """Corpo de `#define NAME(macro) \\ ...`, até a primeira linha sem barra final."""
    start = text.index(f"#define {name}(")
    lines = []
    for line in text[start:].splitlines()[1:]:
        lines.append(line)
        if not line.rstrip().endswith("\\"):
            break
    return "\n".join(lines)


def names(body, call):
    return re.findall(rf"\b{call}\((\w+)", body)


def variant(name):
    """`MAX_ARRAY_INDEX` -> `MaxArrayIndex`; `getByIdDirect` -> `GetByIdDirect`."""
    if "_" in name:
        return "".join(p.capitalize() for p in name.split("_"))
    return name[0].upper() + name[1:]


def array(items):
    return ",\n".join(f'    "{n}"' for n in items)


def pairs(items, enum):
    return ",\n".join(f'    ("{n}", {enum}::{variant(n)})' for n in items)


def main():
    registry = read(os.path.join(UPSTREAM, "BytecodeIntrinsicRegistry.h"))
    functions = names(block(registry, "JSC_COMMON_BYTECODE_INTRINSIC_FUNCTIONS_EACH_NAME"), "macro")
    simple = names(block(registry, "JSC_COMMON_BYTECODE_INTRINSIC_CONSTANTS_SIMPLE_EACH_NAME"), "macro")
    custom = names(block(registry, "JSC_COMMON_BYTECODE_INTRINSIC_CONSTANTS_CUSTOM_EACH_NAME"), "macro")
    constants = simple + custom

    ltc_text = read(os.path.join(UPSTREAM, "LinkTimeConstant.h"))
    builtin_ltc = names(
        block(read(os.path.join(ROOT, "derived", "JavaScriptCore", "JSCBuiltins.h")),
              "JSC_FOREACH_BUILTIN_LINK_TIME_CONSTANT"),
        "macro",
    )
    ltc_body = block(ltc_text, "JSC_FOREACH_LINK_TIME_CONSTANTS")
    link_time = builtin_ltc + names(ltc_body, "v")

    emitters = functions + constants
    assert len(set(emitters)) == len(emitters)
    assert len(set(link_time)) == len(link_time)
    assert functions and simple and custom and link_time

    emitter_variants = "\n".join(f"    {variant(n)} = {i}," for i, n in enumerate(emitters))
    emitter_names = pairs(emitters, "BytecodeIntrinsicEmitter")
    ltc_variants = "\n".join(f"    {variant(n)} = {i}," for i, n in enumerate(link_time))
    ltc_names = pairs(link_time, "LinkTimeConstant")
    simple_names = array(simple)
    out = f"""//! Gerado por `scripts/gen-bytecode-intrinsics.py` a partir de `bytecode/BytecodeIntrinsicRegistry.h`,
//! `bytecode/LinkTimeConstant.h` e `derived/JavaScriptCore/JSCBuiltins.h`. Não editar à mão.

/// Um `emit_intrinsic_<name>` de `BytecodeIntrinsicNode`: primeiro as funções
/// (`JSC_COMMON_BYTECODE_INTRINSIC_FUNCTIONS_EACH_NAME`), depois as constantes
/// (`..._CONSTANTS_EACH_NAME`, as simples e a `orderedHashTableSentinel`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum BytecodeIntrinsicEmitter {{
{emitter_variants}
}}

/// Nome e emitter de cada intrínseco, na ordem do enum `BytecodeIntrinsicEmitter`.
pub const EMITTER_TABLE: [(&str, BytecodeIntrinsicEmitter); {len(emitters)}] = [
{emitter_names},
];

/// Quantas das entradas de `EMITTER_NAMES` são funções (as demais são constantes).
pub const FUNCTION_COUNT: usize = {len(functions)};

/// `JSC_COMMON_BYTECODE_INTRINSIC_CONSTANTS_SIMPLE_EACH_NAME`: as constantes que o registro guarda
/// num `Strong<Unknown>` (`m_<name>`).
pub const SIMPLE_CONSTANT_NAMES: [&str; {len(simple)}] = [
{simple_names},
];

/// `enum class LinkTimeConstant : int32_t`, na ordem de `JSC_FOREACH_LINK_TIME_CONSTANTS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum LinkTimeConstant {{
{ltc_variants}
}}

/// `numberOfLinkTimeConstants`.
pub const NUMBER_OF_LINK_TIME_CONSTANTS: usize = {len(link_time)};

/// Nome e valor de cada `LinkTimeConstant`, na ordem do enum.
pub const LINK_TIME_CONSTANT_TABLE: [(&str, LinkTimeConstant); {len(link_time)}] = [
{ltc_names},
];
"""
    with open(os.path.join(ROOT, "src", "bytecode", "bytecode_intrinsics_table.rs"), "w", encoding="utf-8") as f:
        f.write(out)


if __name__ == "__main__":
    main()
