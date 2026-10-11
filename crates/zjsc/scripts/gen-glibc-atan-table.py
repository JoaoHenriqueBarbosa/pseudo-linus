#!/usr/bin/env python3
"""Gera `src/runtime/glibc_atan_table.rs` a partir de `uatan.tbl` do glibc 2.41.

Uso: gen-glibc-atan-table.py <uatan.tbl> <saída.rs>

Lê a seção LITTLE_ENDI (pares {baixo, alto} de 32 bits) e emite cada `double` como `u64` de bits.
"""
import re
import sys


def main(src_path, out_path):
    text = open(src_path).read()
    little = text.split("#ifdef LITTLE_ENDI", 1)[1]
    pairs = re.findall(r"\{\{(?:\{)?0[xX]([0-9A-Fa-f]+), 0[xX]([0-9A-Fa-f]+)\}", little)
    assert len(pairs) == 241 * 7, len(pairs)
    rows = []
    for i in range(241):
        cells = []
        for j in range(7):
            low, high = pairs[i * 7 + j]
            cells.append("0x%s%s" % (high.lower().zfill(8), low.lower().zfill(8)))
        rows.append("    [" + ", ".join(cells) + "],")
    with open(out_path, "w") as out:
        out.write("//! Tabela `cij` de `uatan.tbl` do glibc 2.41, gerada por `scripts/gen-glibc-atan-table.py`.\n")
        out.write("//! Cada linha: x0, atan(x0) (alto), c2..c6; os valores são os bits do `double`.\n\n")
        out.write("pub(super) const CIJ: [[u64; 7]; 241] = [\n")
        out.write("\n".join(rows))
        out.write("\n];\n")


main(sys.argv[1], sys.argv[2])
