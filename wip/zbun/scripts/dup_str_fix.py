#!/usr/bin/env python3
"""Passe do Store: `{ let arg_a = p.store.str(X); p.store.alloc_str( arg_a) }` (a fatia emprestada do Store prende o
próprio Store, E0502) vira `p.store.dup_str(X)`; `{ let arg_a = p.store.str(X); p.store_name_in_ref( arg_a) }` vira
`{ let arg_a = p.arena_str(X); p.store_name_in_ref(arg_a) }`. X é lido com parênteses balanceados.

Uso: python3 -I dup_str_fix.py RAIZ [--apply]
"""
import pathlib
import re
import sys

HEAD = re.compile(rb"\{ let arg_a = (\w+)\.store\.str\(")
TAILS = (
    (re.compile(rb"\s*;\s*\w+\.store\.alloc_str\(\s*arg_a\)\s*\}"), "dup"),
    (re.compile(rb"\s*;\s*(\w+)\.store_name_in_ref\(\s*arg_a\)\s*\}"), "name"),
)


def balanced_end(text, start):
    depth = 1
    i = start
    while depth:
        c = text[i]
        depth += (c == 0x28) - (c == 0x29)
        i += 1
    return i - 1


def main():
    apply = "--apply" in sys.argv
    root = pathlib.Path([a for a in sys.argv[1:] if not a.startswith("--")][0])
    total = 0
    for path in sorted(root.rglob("*.rs")):
        text = path.read_bytes()
        out, pos, changed = [], 0, 0
        for m in HEAD.finditer(text):
            if m.start() < pos:
                continue
            end = balanced_end(text, m.end())
            expr = text[m.end():end]
            tail_at = end + 1
            for rx, kind in TAILS:
                t = rx.match(text, tail_at)
                if not t:
                    continue
                recv = m.group(1)
                if kind == "dup":
                    new = recv + b".store.dup_str(" + expr + b")"
                else:
                    new = b"{ let arg_a = " + recv + b".arena_str(" + expr + b"); " + t.group(1) + b".store_name_in_ref(arg_a) }"
                out.append(text[pos:m.start()] + new)
                pos = t.end()
                changed += 1
                break
        if changed:
            total += changed
            print(f"{path}: {changed}")
            if apply:
                path.write_bytes(b"".join(out) + text[pos:])
    print(f"reescritos: {total}, aplicado: {apply}")


main()
