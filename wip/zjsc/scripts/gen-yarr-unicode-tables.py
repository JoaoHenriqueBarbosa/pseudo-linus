#!/usr/bin/env python3
"""Gera src/yarr/unicode_pattern_tables.rs a partir de derived/JavaScriptCore/yarr/UnicodePatternTables.h
(saída do generateYarrUnicodePropertyTables.py do WebKit no build do Bun).

O C++ tem uma função `createCharacterClassN()` por propriedade; aqui cada uma vira uma linha de
`CLASSES` com as mesmas quatro listas, a largura e, nas propriedades de sequência, as strings
(`kStringsNData` fatiado por `kStringsNLengths`). As tabelas de hash (`HashIndex`, `HashValue`) e
`unicodeCharacterClassMayContainStrings` saem iguais.
Uso: scripts/gen-yarr-unicode-tables.py (na raiz do crate).
"""
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "derived", "JavaScriptCore", "yarr", "UnicodePatternTables.h")


def nums(text):
    return [int(x, 16) for x in re.findall(r"0x[0-9a-fA-F]+", text)]


def pairs(text):
    return [(int(a, 16), int(b, 16)) for a, b in re.findall(r"\{(0x[0-9a-f]+), (0x[0-9a-f]+)\}", text)]


def u32s(name, xs):
    return f"static {name}: &[u32] = &[{', '.join(hex(x) for x in xs)}];\n"


def ranges(name, xs):
    body = ", ".join(f"CharacterRange::new({hex(a)}, {hex(b)})" for a, b in xs)
    return f"static {name}: &[CharacterRange] = &[{body}];\n"


def main():
    text = open(SRC, encoding="utf-8").read()
    out = [
        "//! Gerado por `scripts/gen-yarr-unicode-tables.py` a partir de\n"
        "//! `derived/JavaScriptCore/yarr/UnicodePatternTables.h`. Não editar à mão.\n\n"
        "use crate::yarr::yarr_pattern::{CharacterClassWidths, CharacterRange};\n"
        "use crate::yarr::yarr_unicode_properties::{ClassData, HashIndex, HashTable, HashValue};\n\n"
    ]
    func = re.compile(
        r"createCharacterClass(\d+)\(\)\n\{\n    // Name = (\S+),.*?\n"
        r"    auto characterClass = makeUnique<CharacterClass>\(\n"
        r"        std::initializer_list<char32_t>\(\{(.*?)\}\),\n"
        r"        std::initializer_list<CharacterRange>\(\{(.*?)\}\),\n"
        r"        std::initializer_list<char32_t>\(\{(.*?)\}\),\n"
        r"        std::initializer_list<CharacterRange>\(\{(.*?)\}\),\n"
        r"        CharacterClassWidths::(\w+)\);\n(.*?)    return characterClass;", re.S)
    rows = []
    for m in func.finditer(text):
        n, name, m8, r8, m32, r32, widths, tail = m.groups()
        n = int(n)
        assert n == len(rows), n
        out.append(f"// {name}\n")
        out.append(u32s(f"M8_{n}", nums(m8)))
        out.append(ranges(f"R8_{n}", pairs(r8)))
        out.append(u32s(f"M32_{n}", nums(m32)))
        out.append(ranges(f"R32_{n}", pairs(r32)))
        strings = "None"
        if "m_strings" in tail:
            data = re.search(rf"kStrings{n}Data\[\] = \{{(.*?)\}};", text, re.S).group(1)
            lengths = re.search(rf"kStrings{n}Lengths\[\] = \{{(.*?)\}};", text, re.S).group(1)
            lens = [int(x) for x in re.findall(r"\d+", lengths)]
            out.append(u32s(f"SD_{n}", nums(data)))
            out.append(f"static SL_{n}: &[u8] = &[{', '.join(map(str, lens))}];\n")
            assert "m_inCanonicalForm = true" in tail
            strings = f"Some((SD_{n}, SL_{n}))"
        rows.append(f"    ClassData {{ matches8: M8_{n}, ranges8: R8_{n}, matches32: M32_{n}, ranges32: R32_{n}, "
                    f"widths: CharacterClassWidths::{widths}, strings: {strings} }}")
    declared = len(re.findall(r"makeUnique<CharacterClass>", text))
    assert len(rows) == declared, (len(rows), declared)
    out.append(f"\n/// `createCharacterClassFunctions`, na mesma ordem.\npub static CLASSES: [ClassData; {len(rows)}] = [\n")
    out.append(",\n".join(rows) + ",\n];\n")

    for table in ["generalCategory", "binaryProperty", "script", "scriptExtension", "sequenceProperty"]:
        idx = re.search(rf"{table}TableIndex\[(\d+)\] = \{{(.*?)\}};", text, re.S)
        val = re.search(rf"{table}TableValue\[(\d+)\] = \{{(.*?)\}};", text, re.S)
        ht = re.search(rf"{table}HashTable = \s*\{{ (\d+), (\d+),", text)
        ix = re.findall(r"\{ (-?\d+), (-?\d+) \}", idx.group(2))
        vs = re.findall(r'\{ "([^"]+)", (\d+) \}', val.group(2))
        assert len(ix) == int(idx.group(1)) and len(vs) == int(val.group(1)) == int(ht.group(1))
        up = re.sub(r"([A-Z])", r"_\1", table).upper()
        out.append(f"\nstatic {up}_INDEX: [HashIndex; {len(ix)}] = [\n"
                   + "".join(f"    HashIndex {{ value: {a}, next: {b} }},\n" for a, b in ix) + "];\n")
        out.append(f"static {up}_VALUES: [HashValue; {len(vs)}] = [\n"
                   + "".join(f'    HashValue {{ key: "{k}", index: {v} }},\n' for k, v in vs) + "];\n")
        out.append(f"pub static {up}_HASH_TABLE: HashTable = HashTable {{ number_of_values: {ht.group(1)}, "
                   f"index_mask: {ht.group(2)}, values: &{up}_VALUES, index: &{up}_INDEX }};\n")

    may = re.search(r"unicodeCharacterClassMayContainStrings\(unsigned unicodeClassId\)\n\{\n"
                    r"    if \(unicodeClassId >= (\d+)\)\n        return false;\n\n"
                    r"    if \(unicodeClassId >= (\d+) && unicodeClassId <= (\d+)\)\n        return true;", text)
    total, lo, hi = may.groups()
    out.append(f"""
/// `unicodeCharacterClassMayContainStrings`.
pub fn unicode_character_class_may_contain_strings(unicode_class_id: u32) -> bool {{
    if unicode_class_id >= {total} {{
        return false;
    }}
    ({lo}..={hi}).contains(&unicode_class_id)
}}
""")
    with open(os.path.join(ROOT, "src", "yarr", "unicode_pattern_tables.rs"), "w", encoding="utf-8") as f:
        f.write("".join(out))


main()
