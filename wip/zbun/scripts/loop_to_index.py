#!/usr/bin/env python3
"""Passe do Store guiado pelo rustc: nas linhas que o diagnóstico (E0502/E0499) marca com "this for loop borrows", o
laço `for X in (&R.store[...])` (com ou sem `.iter()`) empresta o Store durante o corpo, que reentra em `R`. O laço vira
índice com cópia do item, e `X` segue sendo `&T` (referência à cópia), então o corpo não muda:

    let L = R.store[A].F;
    for index in 0..L.len() { let X = &R.store[L][index].clone(); ...

Só trata `&` (leitura). Os laços `&mut`/`iter_mut` ficam para a mão (precisam de gravação de volta).

Uso: python3 -I loop_to_index.py DIAG.json RAIZ [--apply]
"""
import json
import re
import sys

SIMPLE = re.compile(rb"^(\s*)for (\w+) in \(&(\w+)\.store\[(\w+)\.store\[([^\[\]]+)\]\.(\w+)\]\)(?:\.iter\(\))? \{\s*$")
FLAT = re.compile(rb"^(\s*)for (\w+) in \(?&?(\w+)\.store\[([\w.]+)\]\)?(?:\.iter\(\))? \{\s*$")


def main():
    apply = "--apply" in sys.argv
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    diag, root = args[0], args[1]
    lines_by_file = {}
    for line in open(diag, "rb"):
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get("reason") != "compiler-message":
            continue
        d = message["message"]
        if (d.get("code") or {}).get("code") not in ("E0502", "E0499"):
            continue
        for sp in d["spans"]:
            if "for loop borrows" in (sp.get("label") or ""):
                lines_by_file.setdefault(sp["file_name"], set()).add(sp["line_start"])
    done = manual = 0
    for file_name, numbers in lines_by_file.items():
        path = root.rstrip("/") + "/" + file_name
        text = open(path, "rb").read().split(b"\n")
        for number in sorted(numbers, reverse=True):
            line = text[number - 1]
            m = SIMPLE.match(line)
            if m:
                indent, item, recv, _, inner, field = m.groups()
                lst = field + b"_range"
                head = (
                    indent + b"let " + lst + b" = " + recv + b".store[" + inner + b"]." + field + b";\n"
                    + indent + b"for index in 0.." + lst + b".len() {\n"
                    + indent + b"    let " + item + b" = &" + recv + b".store[" + lst + b"][index].clone();"
                )
            else:
                m = FLAT.match(line)
                if not m:
                    manual += 1
                    print(f"{file_name}:{number}: manual ({line.strip()[:70]!r})")
                    continue
                indent, item, recv, var = m.groups()
                head = (
                    indent + b"for index in 0.." + var + b".len() {\n"
                    + indent + b"    let " + item + b" = &" + recv + b".store[" + var + b"][index].clone();"
                )
            text[number - 1] = head
            done += 1
        if apply:
            open(path, "wb").write(b"\n".join(text))
    print(f"laços reescritos: {done}, para a mão: {manual}, aplicado: {apply}")


main()
