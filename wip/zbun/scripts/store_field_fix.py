#!/usr/bin/env python3
"""Passe do Store, parte mecânica (README de wip/zbun, "Regras do porte").

Lê os diagnósticos de `cargo check --message-format=json` e, para cada erro E0609 ("no field `f` on type
`StoreRef<T>`"), troca `recv.f` por `STORE[recv].f` (ou `STORE[*recv].f` quando o tipo é `&StoreRef<T>`), onde STORE
é `self.store` ou `p.store`, conforme a assinatura da função que envolve o acesso.

Uso: python3 -I store_field_fix.py DIAGNOSTICOS.json RAIZ_DO_WORKSPACE [--apply]
Sem `--apply` só relata. O resultado é conferido pelo compilador a cada rodada.
"""
import json
import re
import sys
from collections import defaultdict

FIELD_RE = re.compile(r"^no field `([^`]+)` on type `(&(?:mut )?)?StoreRef<")
FN_RE = re.compile(rb"^\s*(pub(\([a-z]+\))?\s+)?(const\s+)?(unsafe\s+)?(async\s+)?fn\s+\w+")
IDENT = re.compile(rb"[A-Za-z0-9_]")


def match_open(s, close):
    pairs = {ord(")"): ord("("), ord("]"): ord("[")}
    want = pairs[s[close]]
    depth = 0
    i = close
    while i >= 0:
        c = s[i]
        if c in pairs:
            depth += 1
        elif c in (ord("("), ord("[")):
            depth -= 1
            if depth == 0:
                return i if c == want else None
        i -= 1
    return None


def recv_start(s, end):
    """Início da expressão de acesso (cadeia pós-fixa) que termina em `end` (exclusivo)."""
    i = end
    while True:
        while i > 0 and s[i - 1] == ord("?"):
            i -= 1
        if i == 0:
            return None
        c = s[i - 1]
        if c in b")]":
            open_at = match_open(s, i - 1)
            if open_at is None:
                return None
            i = open_at
            if s[i] == ord("(") and not (i > 0 and (IDENT.match(s[i - 1 : i]) or s[i - 1] in b")]")):
                return i
            if s[i] == ord("[") and not (i > 0 and (IDENT.match(s[i - 1 : i]) or s[i - 1] in b")]")):
                return None
            continue
        if IDENT.match(s[i - 1 : i]):
            while i > 0 and IDENT.match(s[i - 1 : i]):
                i -= 1
            if i > 0 and s[i - 1] == ord("."):
                i -= 1
                continue
            if i > 1 and s[i - 2 : i] == b"::":
                i -= 2
                continue
            return i
        return None


def store_name(lines, line_index):
    """`self.store` ou `p.store`, pela assinatura da função mais próxima acima da linha."""
    for k in range(line_index, -1, -1):
        if FN_RE.match(lines[k]):
            signature = b""
            for j in range(k, min(k + 40, len(lines))):
                signature += lines[j]
                if b"{" in lines[j] or b";" in lines[j]:
                    break
            if re.search(rb"(&\s*(mut\s+)?|\bmut\s+)self\b|\bself\s*:", signature):
                return "self.store"
            if re.search(rb"\bp\s*:", signature):
                return "p.store"
            return None
    return None


def main():
    diagnostics, root = sys.argv[1], sys.argv[2]
    apply = "--apply" in sys.argv
    wanted = defaultdict(dict)
    for raw in open(diagnostics, encoding="utf-8"):
        try:
            message = json.loads(raw)
        except ValueError:
            continue
        if message.get("reason") != "compiler-message":
            continue
        message = message["message"]
        if (message.get("code") or {}).get("code") != "E0609":
            continue
        found = FIELD_RE.match(message["message"].replace("no field `", "no field `", 1)) or FIELD_RE.match(
            re.sub(r"^error\[E0609\]: ", "", message["message"])
        )
        if not found:
            continue
        span = next((sp for sp in message["spans"] if sp["is_primary"]), None)
        if span is None or span["file_name"].startswith("/"):
            continue
        wanted[span["file_name"]][span["byte_start"]] = (found.group(1), found.group(2) or "")
    total = skipped = 0
    for name, edits in sorted(wanted.items()):
        path = f"{root}/{name}"
        data = open(path, "rb").read()
        lines = data.splitlines(keepends=True)
        starts = []
        offset = 0
        for line in lines:
            starts.append(offset)
            offset += len(line)
        insertions = []
        for byte_start, (field, ref) in edits.items():
            if byte_start == 0 or data[byte_start - 1] != ord("."):
                skipped += 1
                print(f"pulado (sem ponto): {name}@{byte_start} {field}")
                continue
            dot = byte_start - 1
            begin = recv_start(data, dot)
            line_index = max(i for i, st in enumerate(starts) if st <= dot)
            store = store_name(lines, line_index)
            if begin is None or store is None:
                skipped += 1
                print(f"pulado: {name}:{line_index + 1} {field} (receptor={begin}, store={store})")
                continue
            star = "*" if ref else ""
            insertions.append((begin, 1, f"{store}[{star}".encode()))
            insertions.append((dot, 0, b"]"))
            total += 1
        out = bytearray(data)
        for pos, _, text in sorted(insertions, key=lambda x: (x[0], x[1]), reverse=True):
            out[pos:pos] = text
        if apply:
            open(path, "wb").write(bytes(out))
    print(f"trocas: {total}, puladas: {skipped}, aplicado: {apply}")


main()
