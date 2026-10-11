#!/usr/bin/env python3
"""Passe do Store: o rustc aponta `expected `&[u8]`, found `Vec<u8>`` (E0308) onde a variável virou cópia. O span do erro
é a expressão inteira que está no lugar errado; a correção é prefixar `&`. Só mexe quando a expressão é um identificador
simples (o span tem só `\\w+`), o que deixa o resto para a mão.

Uso: python3 -I ref_fix.py DIAG.json RAIZ [--apply]
"""
import json
import re
import sys

WANT = ("expected `&[u8]`, found `Vec<u8>`", "expected `&_`, found `Vec<u8>`")


def main():
    apply = "--apply" in sys.argv
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    diag, root = args[0], args[1]
    spans = {}
    for line in open(diag, "rb"):
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get("reason") != "compiler-message":
            continue
        d = message["message"]
        if (d.get("code") or {}).get("code") != "E0308":
            continue
        for sp in d["spans"]:
            if sp.get("is_primary") and any(w in (sp.get("label") or "") for w in WANT):
                spans.setdefault(sp["file_name"], set()).add((sp["byte_start"], sp["byte_end"]))
    total = skipped = 0
    for file_name, items in spans.items():
        path = root.rstrip("/") + "/" + file_name
        text = open(path, "rb").read()
        for start, end in sorted(items, reverse=True):
            piece = text[start:end]
            if not re.fullmatch(rb"[A-Za-z_]\w*", piece):
                skipped += 1
                print(f"{file_name}: manual ({piece[:50]!r})")
                continue
            text = text[:start] + b"&" + text[start:]
            total += 1
        if apply:
            open(path, "wb").write(text)
    print(f"prefixados: {total}, para a mão: {skipped}, aplicado: {apply}")


main()
