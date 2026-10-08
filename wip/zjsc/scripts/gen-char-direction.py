#!/usr/bin/env python3
"""Gera src/wtf/unicode/char_direction_table.rs (a `u_charDirection` do ICU 78.3, Unicode 17.0) a
partir de data/unicode/DerivedBidiClass.txt, inclusive os valores padrão das linhas `@missing`,
que o ICU aplica aos pontos de código sem atribuição. Uso: scripts/gen-char-direction.py.
"""
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Ordem do enum `UCharDirection` de unicode/uchar.h.
ORDER = ["L", "R", "EN", "ES", "ET", "AN", "CS", "B", "S", "WS", "ON", "LRE", "LRO", "AL", "RLE",
         "RLO", "PDF", "NSM", "BN", "FSI", "LRI", "RLI", "PDI"]
LONG = {"Left_To_Right": "L", "Right_To_Left": "R", "Arabic_Letter": "AL", "European_Number": "EN",
        "Boundary_Neutral": "BN", "European_Terminator": "ET"}


def main():
    values = [None] * 0x110000
    missing, explicit = [], []
    with open(os.path.join(ROOT, "data", "unicode", "DerivedBidiClass.txt"), encoding="utf-8") as f:
        for line in f:
            if line.startswith("# @missing:"):
                rng, cls = [x.strip() for x in line[len("# @missing:"):].split(";")]
                missing.append((rng, LONG.get(cls, cls)))
                continue
            line = line.split("#", 1)[0].strip()
            if line:
                rng, cls = [x.strip() for x in line.split(";")]
                explicit.append((rng, cls))
    for rng, cls in missing + explicit:  # padrões primeiro, valores explícitos por cima
        a, _, b = rng.partition("..")
        for c in range(int(a, 16), int(b or a, 16) + 1):
            values[c] = ORDER.index(cls)
    runs = []
    for c, v in enumerate(values):
        v = v if v is not None else 0
        if runs and runs[-1][1] == v:
            continue
        runs.append((c, v))
    body = ",\n".join(f"    (0x{c:04X}, {v})" for c, v in runs)
    out = ("//! Gerado por `scripts/gen-char-direction.py` (Unicode 17.0). Não editar à mão.\n\n"
           "/// Início de cada sequência de pontos de código com a mesma `UCharDirection`.\n"
           f"pub static CHAR_DIRECTION_RUNS: &[(u32, u8)] = &[\n{body},\n];\n")
    with open(os.path.join(ROOT, "src", "wtf", "unicode", "char_direction_table.rs"), "w",
              encoding="utf-8") as f:
        f.write(out)


main()
