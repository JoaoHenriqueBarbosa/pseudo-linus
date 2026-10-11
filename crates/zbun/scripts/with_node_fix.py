#!/usr/bin/env python3
"""Passe do Store, empréstimos: `Self::f(p, ..., &mut p.store[r], ...)` vira `with_node!(p, r, data => Self::f(p, ..., data, ...))`.

O despachante de `visit_stmt.rs` passava a `p` e uma referência para dentro do `p.store` na mesma chamada (E0499). O
macro `with_node!` (js_parser/src/lib.rs) copia o nó para fora, roda a chamada sobre a cópia e grava de volta. O
padrão é só a chamada de função associada com `p` como primeiro argumento e o nó como `&mut p.store[IDENT]`; o que
não casa fica para a mão e o compilador confere o resto.

Uso: python3 -I with_node_fix.py ARQUIVO... [--apply]
"""
import re
import sys

CALL = re.compile(
    rb"(?P<head>Self::\w+)\((?P<before>p(?:, [^()&]*?)?, )&mut p\.store\[(?P<ref>\w+)\](?P<after>(?:, [^()]*)?)\)"
)

# `if let Some(v) = &mut STORE[x].campo { corpo }`: o campo é `Option` de valor `Copy` (Expr, Stmt, faixa). Copia para
# fora, roda o corpo sobre a cópia e grava de volta depois do bloco. O fim do bloco é a `}` na mesma indentação.
OPT = re.compile(
    rb"^(?P<ind>[ \t]*)if let Some\((?P<var>\w+)\) = &mut (?P<store>(?:self|p)\.store\[[^\]\n]+\])\.(?P<field>\w+) \{\n"
    rb"(?P<body>.*?)^(?P=ind)\}\n",
    re.S | re.M,
)


def opt_swap(found):
    ind, store, field = found.group("ind"), found.group("store"), found.group("field")
    return (
        ind + b"let mut slot = " + store + b"." + field + b";\n"
        + ind + b"if let Some(" + found.group("var") + b") = &mut slot {\n"
        + found.group("body")
        + ind + b"}\n"
        + ind + store + b"." + field + b" = slot;\n"
    )



def main():
    apply = "--apply" in sys.argv
    total = 0
    for path in [a for a in sys.argv[1:] if not a.startswith("--")]:
        data = open(path, "rb").read()

        def swap(found):
            nonlocal total
            total += 1
            call = found.group("head") + b"(" + found.group("before") + b"data" + found.group("after") + b")"
            return b"with_node!(p, " + found.group("ref") + b", data => " + call + b")"

        out = CALL.sub(swap, data)
        if "--opt" in sys.argv:
            out, count = OPT.subn(opt_swap, out)
            total += count
        # `(mut sr)` no padrão da variante não precisa do `mut`: a cópia é que é mutável.
        out = re.sub(rb"\(mut (sr)\) => with_node!", rb"(\1) => with_node!", out)
        if apply and out != data:
            open(path, "wb").write(out)
    print(f"chamadas: {total}, aplicado: {apply}")


main()
