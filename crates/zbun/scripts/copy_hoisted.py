#!/usr/bin/env python3
"""Passe do Store, empréstimos: o `hoist_args.py` içava `&p.store[R].campo` (ou `&mut ...`) para um `let arg_x = ...;`, e a
referência segue viva na chamada `p.f(arg_x, ...)` que pede `&mut p`. O campo é `Copy` (Expr, Stmt, faixa), então o `let`
leva a cópia e a chamada passa `&arg_x` (ou `&mut arg_x`, com a gravação de volta depois).

    { let arg_a = &self.store[*ex].right; self.f(arg_a) }
 => { let arg_a = self.store[*ex].right; self.f(&arg_a) }

    { let arg_a = &mut p.store[e_].value; p.f(arg_a, ExprIn { .. }) }
 => { let mut arg_a = p.store[e_].value; let hoisted_result = p.f(&mut arg_a, ExprIn { .. }); p.store[e_].value = arg_a; hoisted_result }

A chamada pode ter argumentos com chaves, parênteses e quebras de linha: o fim do bloco se acha por balanceamento.

Uso: python3 -I copy_hoisted.py ARQUIVO... [--apply]
"""
import re
import sys

HEAD = re.compile(
    rb"\{ let (?P<v>arg_[a-z0-9_]+) = &(?P<mut>mut )?(?P<place>(?:self|p)\.store\[[^\]\n]+\]\.\w+); "
)


def matching(text, open_at):
    """Índice do fecho que casa com o abre em `open_at` (`(`, `{` ou `[`), ignorando strings e comentários simples."""
    pairs = {ord("("): ord(")"), ord("{"): ord("}"), ord("["): ord("]")}
    stack = [pairs[text[open_at]]]
    i = open_at + 1
    while i < len(text):
        c = text[i]
        if c == ord('"'):
            i += 1
            while i < len(text) and text[i] != ord('"'):
                i += 2 if text[i] == ord("\\") else 1
        elif text[i : i + 2] == b"//":
            while i < len(text) and text[i] != ord("\n"):
                i += 1
        elif c in pairs:
            stack.append(pairs[c])
        elif c == stack[-1]:
            stack.pop()
            if not stack:
                return i
        i += 1
    return -1


def main():
    apply = "--apply" in sys.argv
    total = 0
    for path in [a for a in sys.argv[1:] if not a.startswith("--")]:
        data = open(path, "rb").read()
        out = bytearray()
        pos = 0
        for head in HEAD.finditer(data):
            if head.start() < pos:
                continue
            block_end = matching(data, head.start())
            if block_end < 0:
                continue
            body = data[head.end() : block_end]  # `CALL(args) ` sem o `}` final
            v, is_mut, place = head.group("v"), head.group("mut"), head.group("place")
            # O argumento é o nome da variável sozinho entre `(` ou `, ` e `,` ou `)`.
            use = re.compile(rb"(?<![\w&])" + v + rb"(?!\w)")
            if not use.search(body):
                continue
            new_body = use.sub((b"&mut " if is_mut else b"&") + v, body, count=1).rstrip()
            total += 1
            out += data[pos : head.start()]
            if is_mut:
                out += (
                    b"{ let mut " + v + b" = " + place + b"; let hoisted_result = " + new_body + b"; "
                    + place + b" = " + v + b"; hoisted_result }"
                )
            else:
                out += b"{ let " + v + b" = " + place + b"; " + new_body + b" }"
            pos = block_end + 1
        out += data[pos:]
        if apply and bytes(out) != data:
            open(path, "wb").write(bytes(out))
    print(f"trocas: {total}, aplicado: {apply}")


main()
