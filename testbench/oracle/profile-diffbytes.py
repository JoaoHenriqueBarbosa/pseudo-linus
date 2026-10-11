#!/usr/bin/env python3
"""Mostra onde dois arquivos (ou duas árvores) de perfil diferem, em faixas de bytes.

Uso: profile-diffbytes.py <árvoreA> <árvoreB>
Para cada arquivo regular presente nas duas e diferente: tamanhos e as faixas [início, fim) que mudam, com os
bytes de A e de B (repr) quando a faixa tem até 40 bytes. Serve para decidir o que no molde vira campo variável.
"""
import os
import sys


def ranges(a, b):
    out = []
    start = None
    limit = min(len(a), len(b))
    for i in range(limit):
        if a[i] != b[i]:
            if start is None:
                start = i
        elif start is not None:
            out.append((start, i))
            start = None
    if start is not None:
        out.append((start, limit))
    return out


def main():
    left, right = sys.argv[1], sys.argv[2]
    for current, _, files in os.walk(left):
        for name in sorted(files):
            a_path = os.path.join(current, name)
            b_path = os.path.join(right, os.path.relpath(a_path, left))
            if not os.path.isfile(a_path) or not os.path.isfile(b_path):
                continue
            a = open(a_path, "rb").read()
            b = open(b_path, "rb").read()
            if a == b:
                continue
            found = ranges(a, b)
            print(f"{os.path.relpath(a_path, left)}: {len(a)} vs {len(b)}, {len(found)} faixas, {sum(e - s for s, e in found)} bytes")
            for start, end in found[:12]:
                if end - start <= 40:
                    print(f"    [{start},{end}) {a[start:end]!r} -> {b[start:end]!r}")
                else:
                    print(f"    [{start},{end}) {end - start} bytes")
            if len(found) > 12:
                print(f"    ... mais {len(found) - 12} faixas")


if __name__ == "__main__":
    main()
