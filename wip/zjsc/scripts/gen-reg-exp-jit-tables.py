#!/usr/bin/env python3
"""Gera src/yarr/reg_exp_jit_tables.rs a partir de derived/JavaScriptCore/yarr/RegExpJitTables.h
(saída do build do Bun/WebKit).

O C++ tem duas tabelas de 65536 bytes (`_wordcharData`, `_spacesData`) e uma função
`xxxCreate()` por classe embutida (wordchar, wordUnicodeIgnoreCaseChar, nonwordchar,
nonwordUnicodeIgnoreCaseChar, newline, spaces, nonspaces, digits, nondigits). Aqui cada tabela vira
um `static` de 65536 `u8` e cada função vira `xxx_create() -> CharacterClass`, com os mesmos
`matches`/`ranges` na mesma ordem. `anycharCreate` não está neste .h (vive em YarrPattern.cpp) e
fica em yarr_pattern.rs.
Uso: scripts/gen-reg-exp-jit-tables.py (na raiz do crate).
"""
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "derived", "JavaScriptCore", "yarr", "RegExpJitTables.h")
DST = os.path.join(ROOT, "src", "yarr", "reg_exp_jit_tables.rs")


def snake(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def main():
    text = open(SRC, encoding="utf-8").read()
    out = [
        "//! Gerado por `scripts/gen-reg-exp-jit-tables.py` a partir de\n"
        "//! `derived/JavaScriptCore/yarr/RegExpJitTables.h`. Não editar à mão.\n\n"
        "use crate::yarr::yarr_pattern::{CharacterClass, CharacterClassWidths, CharacterRange};\n\n"
    ]
    tables = {}
    for m in re.finditer(
            r"static constinit const char (_\w+)\[CharacterClass::tableSize\] = \{(.*?)\};", text, re.S):
        name, body = m.groups()
        values = [int(x) for x in re.findall(r"\d+", body)]
        assert len(values) == 65536, (name, len(values))
        const = re.sub(r"(?<!^)(?=[A-Z])", "_", name.lstrip("_")).upper()
        tables[name] = const
        out.append(f"pub static {const}: [u8; 65536] = [\n")
        for i in range(0, 65536, 32):
            out.append("    " + ", ".join(str(v) for v in values[i:i + 32]) + ",\n")
        out.append("];\n\n")
    func = re.compile(
        r"std::unique_ptr<CharacterClass> (\w+)Create\(\)\n\{\n"
        r"    auto characterClass = makeUnique<CharacterClass>\((.*?)\);\n(.*?)    return characterClass;", re.S)
    for m in func.finditer(text):
        name, ctor, body = m.groups()
        fn = snake(name) + "_create"
        out.append(f"/// `{name}Create`.\n")
        out.append(f"pub fn {fn}() -> CharacterClass {{\n")
        if ctor:
            table, inverted = re.match(r"(_\w+), (true|false)", ctor).groups()
            out.append(f"    let mut class = CharacterClass::with_table(&{tables[table]}, {inverted});\n")
        else:
            out.append("    let mut class = CharacterClass::new();\n")
        for line in body.strip().splitlines():
            line = line.strip()
            r = re.match(r"characterClass->m_(ranges8|ranges32)\.append\(CharacterRange\((0x\w+), (0x\w+)\)\);", line)
            if r:
                out.append(f"    class.{r.group(1)}.push(CharacterRange::new({r.group(2)}, {r.group(3)}));\n")
                continue
            r = re.match(r"characterClass->m_(matches8|matches32)\.append\((0x\w+)\);", line)
            if r:
                out.append(f"    class.{r.group(1)}.push({r.group(2)});\n")
                continue
            r = re.match(r"characterClass->m_characterWidths = CharacterClassWidths::(\w+);", line)
            assert r, line
            out.append(f"    class.character_widths = CharacterClassWidths::{r.group(1)};\n")
        out.append("    class\n}\n\n")
    with open(DST, "w", encoding="utf-8") as f:
        f.write("".join(out).rstrip("\n") + "\n")


if __name__ == "__main__":
    main()
