#!/usr/bin/env python3
"""Gera src/yarr/yarr_canonicalize_unicode.rs a partir de
derived/JavaScriptCore/yarr/YarrCanonicalizeUnicode.cpp (saída do generateYarrCanonicalizeUnicode
no build do Bun): os conjuntos `unicodeCharacterSetN`, `unicodeCharacterSetInfo` e
`unicodeRangeInfo`, as tabelas do `CanonicalMode::Unicode`.
Uso: scripts/gen-yarr-canonicalize-unicode.py (na raiz do crate).
"""
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "derived", "JavaScriptCore", "yarr", "YarrCanonicalizeUnicode.cpp")


def main():
    text = open(SRC, encoding="utf-8").read()
    sets = re.findall(r"unicodeCharacterSet(\d+)\[\] = \{ ([^}]*) \};", text)
    for i, (n, _) in enumerate(sets):
        assert int(n) == i
    nsets = int(re.search(r"UNICODE_CANONICALIZATION_SETS = (\d+);", text).group(1))
    nranges = int(re.search(r"UNICODE_CANONICALIZATION_RANGES = (\d+);", text).group(1))
    assert len(sets) == nsets
    block = text[text.index("unicodeRangeInfo[UNICODE_CANONICALIZATION_RANGES]"):]
    block = block[:block.index("};")]
    ranges = re.findall(r"\{ (0x[0-9a-f]+), (0x[0-9a-f]+), (0x[0-9a-f]+), (\w+) \}", block)
    assert len(ranges) == nranges, (len(ranges), nranges)
    out = [
        "//! Gerado por `scripts/gen-yarr-canonicalize-unicode.py` a partir de\n"
        "//! `derived/JavaScriptCore/yarr/YarrCanonicalizeUnicode.cpp`. Não editar à mão.\n\n"
        "use super::yarr_canonicalize::UCS2CanonicalizationType::*;\n"
        "use super::yarr_canonicalize::{r, CanonicalizationRange};\n\n",
    ]
    for n, values in sets:
        out.append(f"const UNICODE_CHARACTER_SET{n}: &[u32] = &[{values.strip()}];\n")
    out.append(f"\npub const UNICODE_CANONICALIZATION_SETS: usize = {nsets};\n")
    out.append("pub static UNICODE_CHARACTER_SET_INFO: [&[u32]; UNICODE_CANONICALIZATION_SETS] = [\n")
    out.extend(f"    UNICODE_CHARACTER_SET{n},\n" for n, _ in sets)
    out.append("];\n\n")
    out.append(f"pub const UNICODE_CANONICALIZATION_RANGES: usize = {nranges};\n")
    out.append("pub static UNICODE_RANGE_INFO: [CanonicalizationRange; UNICODE_CANONICALIZATION_RANGES] = [\n")
    out.extend(f"    r({a}, {b}, {v}, {t}),\n" for a, b, v, t in ranges)
    out.append("];\n")
    with open(os.path.join(ROOT, "src", "yarr", "yarr_canonicalize_unicode.rs"), "w", encoding="utf-8") as f:
        f.write("".join(out))


main()
