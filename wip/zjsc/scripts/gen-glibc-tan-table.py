#!/usr/bin/env python3
"""Gera `src/runtime/glibc_tan_table.rs` a partir de `utan.tbl` do glibc 2.41.

Uso: gen-glibc-tan-table.py <utan.tbl> <saída.rs>

Lê a seção LITTLE_ENDI (pares {baixo, alto} de 32 bits) e emite cada `double` como `u64` de bits.
Cada linha de `xfg` tem quatro células (xi, Fi, Gi, FFi); a última não é usada por `__tan` e é omitida.
"""
import re
import sys


def main(src_path, out_path):
    text = open(src_path).read()
    little = text.split("#ifdef LITTLE_ENDI", 1)[1]
    pairs = re.findall(r"\{\{(?:\{)?0[xX]([0-9A-Fa-f]+), 0[xX]([0-9A-Fa-f]+)\}", little)
    assert len(pairs) == 186 * 4, len(pairs)
    rows = []
    for i in range(186):
        cells = []
        for j in range(3):
            low, high = pairs[i * 4 + j]
            cells.append("0x%s%s" % (high.lower().zfill(8), low.lower().zfill(8)))
        rows.append("    [" + ", ".join(cells) + "],")
    with open(out_path, "w") as out:
        out.write("//! Tabela `xfg` de `utan.tbl` do glibc 2.41, gerada por `scripts/gen-glibc-tan-table.py`.\n")
        out.write("//! Cada linha: xi, Fi, Gi (a quarta célula, FFi, não é usada); os valores são os bits do `double`.\n\n")
        out.write("pub(super) const XFG: [[u64; 3]; 186] = [\n")
        out.write("\n".join(rows))
        out.write("\n];\n")


main(sys.argv[1], sys.argv[2])
