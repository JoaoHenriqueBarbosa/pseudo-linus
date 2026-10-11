#!/usr/bin/env python3
"""Passe do Store guiado pelo rustc: nas linhas que o diagnóstico (E0499/E0502) aponta com
`{ let arg_a = &mut p.store[X].F; p.CALL( arg_a, ... ) }` (a chamada reentra em `p`, e o `&mut` do Store vive até ela
voltar), o bloco vira cópia, chamada e gravação de volta:

    { let mut arg_a = p.store[X].F; let result = p.CALL( &mut arg_a, ... ); p.store[X].F = arg_a; result }

Sem `.F` (o nó inteiro), a cópia é `.clone()`. O fim do bloco é achado por balanceamento de parênteses.

Uso: python3 -I place_call.py DIAG.json RAIZ [--apply]
"""
import json
import re
import sys

HEAD = re.compile(rb"\{ let arg_a = &mut (p\.store\[[^;\n]*?\])((?:\.\w+)*); (p\.\w+(?:::<[^>]*>)?)\(\s*arg_a")


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
            if sp["is_primary"]:
                lines_by_file.setdefault(sp["file_name"], set()).add(sp["line_start"])
    done = manual = 0
    for file_name, numbers in lines_by_file.items():
        path = root.rstrip("/") + "/" + file_name
        text = open(path, "rb").read()
        offsets = [0]
        for part in text.split(b"\n"):
            offsets.append(offsets[-1] + len(part) + 1)
        edits = []
        for number in sorted(numbers):
            lo, hi = offsets[number - 1], offsets[number]
            m = HEAD.search(text, lo, hi)
            if not m:
                continue
            call_open = m.end(3)
            end = balanced_end(text, call_open + 1)
            tail = re.compile(rb"\s*\}").match(text, end + 1)
            if not tail:
                manual += 1
                print(f"{file_name}:{number}: manual (fecho)")
                continue
            place, field, call = m.group(1), m.group(2), m.group(3)
            source = place + field + (b"" if field else b".clone()")
            rest = text[m.end():end]
            new = (
                b"{ let mut arg_a = " + source + b"; let result = " + call + b"( &mut arg_a" + rest + b"); "
                + place + field + b" = arg_a; result }"
            )
            edits.append((m.start(), tail.end(), new))
            done += 1
        if apply:
            for start, stop, new in sorted(edits, reverse=True):
                text = text[:start] + new + text[stop:]
            open(path, "wb").write(text)
    print(f"blocos reescritos: {done}, para a mão: {manual}, aplicado: {apply}")


main()
