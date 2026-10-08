#!/usr/bin/env python3
"""Gera src/wtf/unicode/case_mapping_tables.rs a partir do UCD 17.0 (o Unicode do ICU 78.3 do
bun 1.4.2, conferido com `process.versions`). Uso: scripts/gen-case-mapping.py (na raiz do crate).

Tabelas:
- SIMPLE_LOWER/SIMPLE_UPPER: mapeamento simples (colunas 13 e 12 do UnicodeData), que é o que
  `u_tolower`/`u_toupper` devolvem;
- FULL_LOWER/FULL_UPPER: entradas incondicionais do SpecialCasing (sem condição de idioma nem de
  contexto), que `u_strToLower`/`u_strToUpper` da raiz usam;
- SIMPLE_FOLD/FULL_FOLD: CaseFolding status C+S (`u_foldCase`) e C+F (`u_strFoldCase`), com
  `U_FOLD_CASE_DEFAULT` (as linhas T, de turco, ficam de fora);
- CASED e CASE_IGNORABLE: faixas das propriedades derivadas, para a regra de contexto Final_Sigma.
"""
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
UCD = os.path.join(ROOT, "data", "unicode")


def read(name):
    with open(os.path.join(UCD, name), encoding="utf-8") as f:
        for line in f:
            line = line.split("#", 1)[0].strip()
            if line:
                yield [x.strip() for x in line.split(";")]


def unicode_data():
    lower, upper = {}, {}
    for f in read("UnicodeData.txt"):
        cp = int(f[0], 16)
        if f[12]:
            upper[cp] = int(f[12], 16)
        if f[13]:
            lower[cp] = int(f[13], 16)
    return lower, upper


def special_casing():
    lower, upper = {}, {}
    for f in read("SpecialCasing.txt"):
        if len(f) > 4 and f[4]:
            continue  # condicional (idioma ou contexto): Final_Sigma é tratado no código
        cp = int(f[0], 16)
        lo = [int(x, 16) for x in f[1].split()]
        up = [int(x, 16) for x in f[3].split()]
        if lo != [cp]:
            lower[cp] = lo
        if up != [cp]:
            upper[cp] = up
    return lower, upper


def case_folding():
    simple, full_ = {}, {}
    for f in read("CaseFolding.txt"):
        cp = int(f[0], 16)
        to = [int(x, 16) for x in f[2].split()]
        if f[1] in ("C", "S"):
            simple[cp] = to[0]
        if f[1] in ("C", "F"):
            full_[cp] = to
    return simple, full_


def derived(prop):
    ranges = []
    for f in read("DerivedCoreProperties.txt"):
        if f[1] != prop:
            continue
        a, _, b = f[0].partition("..")
        ranges.append((int(a, 16), int(b or a, 16)))
    ranges.sort()
    merged = []
    for a, b in ranges:
        if merged and a <= merged[-1][1] + 1:
            merged[-1] = (merged[-1][0], max(b, merged[-1][1]))
        else:
            merged.append((a, b))
    return merged


def pairs(name, m):
    body = ",\n".join(f"    (0x{k:04X}, 0x{v:04X})" for k, v in sorted(m.items()))
    return f"pub static {name}: &[(u32, u32)] = &[\n{body},\n];\n"


def full(name, m):
    rows = []
    for k, v in sorted(m.items()):
        rows.append(f"    (0x{k:04X}, &[{', '.join(f'0x{x:04X}' for x in v)}])")
    return f"pub static {name}: &[(u32, &[u32])] = &[\n" + ",\n".join(rows) + ",\n];\n"


def ranges(name, r):
    body = ",\n".join(f"    (0x{a:04X}, 0x{b:04X})" for a, b in r)
    return f"pub static {name}: &[(u32, u32)] = &[\n{body},\n];\n"


def main():
    lo, up = unicode_data()
    flo, fup = special_casing()
    out = [
        "//! Gerado por `scripts/gen-case-mapping.py` a partir do UCD 17.0. Não editar à mão.\n",
        pairs("SIMPLE_LOWER", lo),
        pairs("SIMPLE_UPPER", up),
        full("FULL_LOWER", flo),
        full("FULL_UPPER", fup),
        pairs("SIMPLE_FOLD", case_folding()[0]),
        full("FULL_FOLD", case_folding()[1]),
        ranges("CASED", derived("Cased")),
        ranges("CASE_IGNORABLE", derived("Case_Ignorable")),
    ]
    path = os.path.join(ROOT, "src", "wtf", "unicode", "case_mapping_tables.rs")
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(out))


main()
