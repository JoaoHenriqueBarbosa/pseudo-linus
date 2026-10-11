#!/usr/bin/env python3
"""Gera `src/runtime/glibc_asin_table.rs` a partir de `asincos.tbl` e `root.tbl` do glibc 2.41.

Uso: gen-glibc-asin-table.py <asincos.tbl> <root.tbl> <saída.rs>

De `asincos.tbl` lê a seção LITTLE_ENDI (pares {baixo, alto} de 32 bits, 2568 doubles) e emite cada
`double` como `u64` de bits. De `root.tbl` lê `inroot` (128 doubles em decimal) e emite os bits, para que
o valor seja o mesmo que o compilador de C produz ao ler o literal.
"""
import re
import struct
import sys


def main(asincos_path, root_path, out_path):
    text = open(asincos_path).read()
    little = text.split("#ifdef LITTLE_ENDI", 1)[1]
    pairs = re.findall(r"0[xX]([0-9A-Fa-f]+), 0[xX]([0-9A-Fa-f]+)", little)
    assert len(pairs) == 2568, len(pairs)
    root = open(root_path).read()
    body = root.split("inroot[128]", 1)[1].split("};", 1)[0]
    values = re.findall(r"\d+\.\d+", body)
    assert len(values) == 128, len(values)

    def emit(name, size, words, out):
        out.write("pub(super) const %s: [u64; %d] = [\n" % (name, size))
        for i in range(0, size, 4):
            out.write("    " + ", ".join(words[i:i + 4]) + ",\n")
        out.write("];\n")

    asncs = ["0x%s%s" % (high.lower().zfill(8), low.lower().zfill(8)) for low, high in pairs]
    inroot = ["0x%016x" % struct.unpack("<Q", struct.pack("<d", float(v)))[0] for v in values]
    with open(out_path, "w") as out:
        out.write("//! Tabelas `asncs` de `asincos.tbl` e `inroot` de `root.tbl` do glibc 2.41, geradas por\n")
        out.write("//! `scripts/gen-glibc-asin-table.py`. Os valores são os bits do `double`.\n\n")
        emit("ASNCS", 2568, asncs, out)
        out.write("\n")
        emit("INROOT", 128, inroot, out)


main(sys.argv[1], sys.argv[2], sys.argv[3])
