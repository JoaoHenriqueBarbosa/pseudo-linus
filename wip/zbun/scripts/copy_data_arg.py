#!/usr/bin/env python3
"""Passe do Store: `{ let arg_b = &p.store[X].data; Self::f(p, arg_b) }` empresta o Store enquanto `p` é passado como
`&mut`. O dado é `Copy`: o bloco copia `arg_b` e passa `&arg_b`.

Uso: python3 -I copy_data_arg.py RAIZ [--apply]
"""
import pathlib
import re
import sys

RX = re.compile(rb"let arg_b = &(p\.store\[[^;\n]*?\][\w.]*\.data); ([A-Za-z_]\w*::\w+|p\.\w+)\(p, arg_b\)")


def main():
    apply = "--apply" in sys.argv
    root = pathlib.Path([a for a in sys.argv[1:] if not a.startswith("--")][0])
    total = 0
    for path in sorted(root.rglob("*.rs")):
        text = path.read_bytes()
        new, n = RX.subn(rb"let arg_b = \1; \2(p, &arg_b)", text)
        if n:
            total += n
            print(f"{path}: {n}")
            if apply:
                path.write_bytes(new)
    print(f"reescritos: {total}, aplicado: {apply}")


main()
