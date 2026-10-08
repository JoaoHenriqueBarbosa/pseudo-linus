#!/usr/bin/env python3
"""Gera src/wtf/unicode/properties_tables.rs (Unicode 17.0, o do ICU 78.3 do bun 1.4.2):
- ID_START e ID_CONTINUE (`u_hasBinaryProperty(UCHAR_ID_START/ID_CONTINUE)`), de
  DerivedCoreProperties.txt;
- GENERAL_CATEGORY_RUNS (`u_charType`), de extracted/DerivedGeneralCategory.txt, com os valores
  do enum `UCharCategory` do ICU (Cn = 0, Lu = 1, ..., Pf = 29).
Uso: scripts/gen-unicode-properties.py (na raiz do crate).
"""
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
UCD = os.path.join(ROOT, "data", "unicode")

# Ordem do enum `UCharCategory` de unicode/uchar.h.
ORDER = ["Cn", "Lu", "Ll", "Lt", "Lm", "Lo", "Mn", "Me", "Mc", "Nd", "Nl", "No", "Zs", "Zl", "Zp",
         "Cc", "Cf", "Co", "Cs", "Pd", "Ps", "Pe", "Pc", "Po", "Sm", "Sc", "Sk", "So", "Pi", "Pf"]


def entries(name):
    with open(os.path.join(UCD, name), encoding="utf-8") as f:
        for line in f:
            line = line.split("#", 1)[0].strip()
            if line:
                rng, val = [x.strip() for x in line.split(";")[:2]]
                a, _, b = rng.partition("..")
                yield int(a, 16), int(b or a, 16), val


def merged(pairs):
    out = []
    for a, b in sorted(pairs):
        if out and a <= out[-1][1] + 1:
            out[-1] = (out[-1][0], max(b, out[-1][1]))
        else:
            out.append((a, b))
    return out


def ranges(name, r):
    body = ",\n".join(f"    (0x{a:04X}, 0x{b:04X})" for a, b in r)
    return f"pub static {name}: &[(u32, u32)] = &[\n{body},\n];\n"


def main():
    props = {"ID_Start": [], "ID_Continue": []}
    for a, b, v in entries("DerivedCoreProperties.txt"):
        if v in props:
            props[v].append((a, b))
    gc = [0] * 0x110000
    for a, b, v in entries("DerivedGeneralCategory.txt"):
        for c in range(a, b + 1):
            gc[c] = ORDER.index(v)
    runs = []
    for c, v in enumerate(gc):
        if not runs or runs[-1][1] != v:
            runs.append((c, v))
    body = ",\n".join(f"    (0x{c:04X}, {v})" for c, v in runs)
    out = [
        "//! Gerado por `scripts/gen-unicode-properties.py` (Unicode 17.0). Não editar à mão.\n",
        ranges("ID_START", merged(props["ID_Start"])),
        ranges("ID_CONTINUE", merged(props["ID_Continue"])),
        "/// Início de cada sequência de pontos de código com a mesma `UCharCategory`.\n"
        f"pub static GENERAL_CATEGORY_RUNS: &[(u32, u8)] = &[\n{body},\n];\n",
    ]
    with open(os.path.join(ROOT, "src", "wtf", "unicode", "properties_tables.rs"), "w",
              encoding="utf-8") as f:
        f.write("\n".join(out))


main()
