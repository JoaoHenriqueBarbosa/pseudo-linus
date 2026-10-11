#!/usr/bin/env python3
"""Passe do Store, empréstimos duplos: dois padrões que o rustc acusa em cascata (E0499/E0502).

1. `RECV.f(ARGS..., &mut RECV.store[REF].campo, ...)` (a visita reentra em `RECV` e o campo vive em `RECV.store`):
   copia o campo para fora, chama sobre a cópia e grava de volta. Vale porque o campo é `Copy` (Expr, Stmt, faixa).
2. `for x in (&mut RECV.store[RECV.store[REF].campo]) { corpo }` / `for x in &mut RECV.store[FAIXA] { corpo }`: o corpo
   reentra em `RECV`, então o laço passa a andar por índice, copiando o elemento e gravando de volta.

O que não casa fica para a mão e o compilador confere o resto.

Uso: python3 -I borrow_fix.py ARQUIVO... [--apply]
"""
import re
import sys

# `self.f(a, &mut self.store[*e].campo, b);` numa linha própria.
CALL = re.compile(
    rb"^(?P<ind>[ \t]*)(?P<recv>self|p)\.(?P<fn>\w+)\((?P<before>[^()\n]*?)&mut (?P=recv)\.store\[(?P<ref>[^\]\n]+)\]\.(?P<field>\w+)(?P<after>(?:, [^()\n]*)?)\);\n",
    re.M,
)

# Braço de `match` sem bloco: `PAT => self.f(ARGS, &mut self.store[*e].campo, ...),`.
ARM = re.compile(
    rb"^(?P<ind>[ \t]*)(?P<pat>[^\n=]*?=>) (?P<recv>self|p)\.(?P<fn>\w+)\((?P<before>[^()\n]*?)&mut (?P=recv)\.store\[(?P<ref>[^\]\n]+)\]\.(?P<field>\w+)(?P<after>(?:, [^()\n]*)?)\),\n",
    re.M,
)


def arm_swap(found, counter):
    ind, recv, fn = found.group("ind"), found.group("recv"), found.group("fn")
    ref, field = found.group("ref"), found.group("field")
    counter[0] += 1
    return (
        ind + found.group("pat") + b" {\n"
        + ind + b"    let mut slot = " + recv + b".store[" + ref + b"]." + field + b";\n"
        + ind + b"    " + recv + b"." + fn + b"(" + found.group("before") + b"&mut slot" + found.group("after") + b");\n"
        + ind + b"    " + recv + b".store[" + ref + b"]." + field + b" = slot;\n"
        + ind + b"}\n"
    )


# `for a in (&mut self.store[SLICE]) {` ou sem parênteses; o corpo vai até a `}` na mesma indentação.
LOOP = re.compile(
    rb"^(?P<ind>[ \t]*)for (?P<var>\w+) in \(?&mut (?P<recv>self|p)\.store\[(?P<slice>(?:[^\[\]\n]|\[[^\]\n]*\])+)\]\)? \{\n"
    rb"(?P<body>.*?)^(?P=ind)\}\n",
    re.S | re.M,
)


def call_swap(found, counter):
    ind, recv, fn = found.group("ind"), found.group("recv"), found.group("fn")
    ref, field = found.group("ref"), found.group("field")
    before, after = found.group("before"), found.group("after")
    counter[0] += 1
    return (
        ind + b"let mut slot = " + recv + b".store[" + ref + b"]." + field + b";\n"
        + ind + recv + b"." + fn + b"(" + before + b"&mut slot" + after + b");\n"
        + ind + recv + b".store[" + ref + b"]." + field + b" = slot;\n"
    )


def loop_swap(found, counter):
    ind, var, recv = found.group("ind"), found.group("var"), found.group("recv")
    sl, body = found.group("slice"), found.group("body")
    counter[0] += 1
    # O corpo tratava a variável como `&mut T`; agora ela é a cópia, e o argumento vira `&mut var`.
    body = re.sub(rb"(?<=[(,] )" + var + rb"(?=[,)])|(?<=\()" + var + rb"(?=[,)])", b"&mut " + var, body)
    return (
        ind + b"let range = " + sl + b";\n"
        + ind + b"for i in 0.." + recv + b".store[range].len() {\n"
        + ind + b"    let mut " + var + b" = " + recv + b".store[range][i];\n"
        + body
        + ind + b"    " + recv + b".store[range][i] = " + var + b";\n"
        + ind + b"}\n"
    )


def main():
    apply = "--apply" in sys.argv
    counter = [0]
    for path in [a for a in sys.argv[1:] if not a.startswith("--")]:
        data = open(path, "rb").read()
        out = ARM.sub(lambda f: arm_swap(f, counter), data)
        out = CALL.sub(lambda f: call_swap(f, counter), out)
        out = LOOP.sub(lambda f: loop_swap(f, counter), out)
        if apply and out != data:
            open(path, "wb").write(out)
    print(f"trocas: {counter[0]}, aplicado: {apply}")


if __name__ == "__main__":
    main()
