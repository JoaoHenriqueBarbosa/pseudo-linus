#!/usr/bin/env python3
"""Passe do Store, empréstimos duplos (E0499 e E0502) de argumento de chamada.

O upstream escrevia `Stmt::assign(&mut p.store, p.new_expr(..), rhs)` e `p.new_expr(E::Dot { name: p.store.alloc_str(x) }, loc)`:
com o `Store` dentro do `p`, o primeiro empréstimo mutável ainda vive quando o argumento pede o segundo. A regra de
texto: cada argumento da chamada que usa `p.` ou `self.` (e não é o próprio `&mut p.store`) sobe para um `let`, e a
chamada vira um bloco `{ let arg_a = ..; let arg_b = ..; CHAMADA(.., arg_a, arg_b) }`. A ordem de avaliação dos
argumentos se mantém (esquerda para direita); o que não usa `p` não muda de lugar.

A chamada vem do diagnóstico do rustc: o rótulo "first borrow later used by call" aponta o callee (`Stmt::assign` ou o
nome do método). Cada chamada se trata uma vez; as de dentro primeiro (do fim do arquivo para o começo), o que mantém
os deslocamentos das de fora.

Uso: python3 -I hoist_args.py DIAG.json RAIZ_DO_CRATE [--apply]   (sem --apply só lista)
O que não casa (receptor que não é cadeia de identificadores, chamada sem `(` logo depois) fica para a mão e é listado.
"""
import json
import re
import sys

USES_PARSER = re.compile(rb"\b(?:p|self)\.")
STORE_BORROW = re.compile(rb"^\s*&mut\s+(?:p|self)\.store\s*$")
RECEIVER = re.compile(rb"[A-Za-z_][A-Za-z0-9_]*\.$")


def skip_literal(text, i):
    """`i` aponta para o começo de uma string, de um char, ou de um comentário; devolve o índice depois dele, ou `i`."""
    c = text[i : i + 1]
    if c == b'"':
        j = i + 1
        while j < len(text):
            if text[j : j + 1] == b"\\":
                j += 2
                continue
            if text[j : j + 1] == b'"':
                return j + 1
            j += 1
        return len(text)
    if c == b"'":
        # char literal (`'x'`, `'\n'`, `'\u{..}'`); um apóstrofo de lifetime não fecha em 3 a 10 bytes com `'`.
        m = re.compile(rb"'(?:\\u\{[0-9a-fA-F]+\}|\\.|[^\\'])'").match(text, i)
        return m.end() if m else i
    if text[i : i + 2] == b"//":
        j = text.find(b"\n", i)
        return len(text) if j < 0 else j
    if text[i : i + 2] == b"/*":
        j = text.find(b"*/", i + 2)
        return len(text) if j < 0 else j + 2
    return i


def matching(text, open_at):
    """O índice do `)` que fecha o `(` em `open_at`."""
    depth = 0
    i = open_at
    while i < len(text):
        skipped = skip_literal(text, i)
        if skipped != i:
            i = skipped
            continue
        c = text[i : i + 1]
        if c in b"([{":
            depth += 1
        elif c in b")]}":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return -1


def split_args(text, open_at, close_at):
    """Os argumentos de topo entre `(` e `)`, como pares (início, fim) de byte."""
    args = []
    depth = 0
    start = open_at + 1
    i = open_at + 1
    while i < close_at:
        skipped = skip_literal(text, i)
        if skipped != i:
            i = skipped
            continue
        c = text[i : i + 1]
        if c in b"([{":
            depth += 1
        elif c in b")]}":
            depth -= 1
        elif c == b"," and depth == 0:
            args.append((start, i))
            start = i + 1
        i += 1
    if text[start:close_at].strip():
        args.append((start, close_at))
    return args


def calls_from_diag(diag_path):
    calls = {}
    for line in open(diag_path, "rb"):
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get("reason") != "compiler-message":
            continue
        diag = message["message"]
        if diag["level"] != "error" or (diag.get("code") or {}).get("code") not in ("E0499", "E0502"):
            continue
        for span in diag["spans"]:
            if (span.get("label") or "").startswith("first borrow later used by call"):
                calls.setdefault(span["file_name"], set()).add((span["byte_start"], span["byte_end"]))
    return calls


def hoist(text, callee_start, callee_end):
    """Devolve (novo_texto, descrição) ou (None, motivo)."""
    open_at = callee_end
    while text[open_at : open_at + 1] in b" \n\t":
        open_at += 1
    if text[open_at : open_at + 1] != b"(":
        return None, "sem `(` depois do callee"
    close_at = matching(text, open_at)
    if close_at < 0:
        return None, "parêntese sem par"
    call_start = callee_start
    # Chamada de método: o receptor (`p.`, `self.store.`) faz parte da expressão que o bloco envolve.
    while True:
        m = RECEIVER.search(text[:call_start])
        if not m or m.end() != call_start:
            break
        call_start = m.start()
    args = split_args(text, open_at, close_at)
    lets = []
    pieces = []
    last = open_at + 1
    for index, (a, b) in enumerate(args):
        arg = text[a:b]
        pieces.append(text[last:a])
        if USES_PARSER.search(arg) and not STORE_BORROW.match(arg):
            name = b"arg_" + bytes([ord("a") + index])
            lets.append(b"let " + name + b" = " + arg.strip() + b"; ")
            pieces.append(b" " + name)
        else:
            pieces.append(arg)
        last = b
    pieces.append(text[last:close_at])
    if not lets:
        return None, "nenhum argumento usa p/self"
    call = text[call_start:open_at + 1] + b"".join(pieces) + b")"
    block = b"{ " + b"".join(lets) + call + b" }"
    return text[:call_start] + block + text[close_at + 1 :], f"{len(lets)} argumento(s) içado(s)"


def main():
    apply = "--apply" in sys.argv
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    diag, root = args[0], args[1]
    total = skipped = 0
    for file_name, spans in calls_from_diag(diag).items():
        path = root.rstrip("/") + "/" + file_name
        try:
            text = open(path, "rb").read()
        except OSError:
            continue
        changed = False
        for start, end in sorted(spans, reverse=True):
            new_text, note = hoist(text, start, end)
            line = text.count(b"\n", 0, start) + 1
            if new_text is None:
                skipped += 1
                print(f"{file_name}:{line}: manual ({note})")
                continue
            total += 1
            text = new_text
            changed = True
            print(f"{file_name}:{line}: {note}")
        if apply and changed:
            open(path, "wb").write(text)
    print(f"chamadas tratadas: {total}, para a mão: {skipped}, aplicado: {apply}")


main()
