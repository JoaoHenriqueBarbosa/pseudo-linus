#!/usr/bin/env python3
"""Passe do Store, parte mecânica (README de wip/zbun, "Regras do porte").

Lê os diagnósticos de `cargo check --message-format=json` e aplica trocas que o compilador já apontou. Em cada uma,
STORE é `self.store` ou `p.store`, conforme a assinatura da função que envolve o trecho.

  E0609  campo em `StoreRef<T>`        `r.campo`           ->  `STORE[r].campo`
  E0277  `Store` indexado por `&Ref`   `STORE[r]`          ->  `STORE[*r]`
  E0599  `StoreStr::slice()`           `r.slice()`         ->  `STORE.str(r)`
  E0599  `StoreSlice::slice()/_mut()`  `r.slice()`         ->  `(&STORE[r])` / `(&mut STORE[r])`
  E0599  `StoreSlice::iter()` e afins  `r.iter()`          ->  `STORE[r].iter()`
  E0599  `StoreRef::get()/get_mut()`   `r.get()`           ->  `(&STORE[r])` / `(&mut STORE[r])`

Uso: python3 -I store_fix.py DIAGNOSTICOS.json RAIZ_DO_WORKSPACE [--apply]
Sem `--apply` só relata. O resultado é conferido pelo compilador a cada rodada.
"""
import json
import re
import sys
from collections import defaultdict

FIELD_RE = re.compile(r"^no field `([^`]+)` on type `(&(?:mut )?)?StoreRef<")
INDEX_RE = re.compile(r"^the type `Store` cannot be indexed by `&(?:mut )?(?:StoreRef|StoreSlice)<")
METHOD_RE = re.compile(
    r"^no method named `(\w+)` found for (?:struct|reference|mutable reference) "
    r"`(&(?:mut )?)?(StoreStr|StoreSlice<[^`]*>|StoreRef<[^`]*>)`"
)
FN_RE = re.compile(rb"^\s*(pub(\([a-z]+\))?\s+)?(const\s+)?(unsafe\s+)?(async\s+)?fn\s+\w+")
IDENT = re.compile(rb"[A-Za-z0-9_]")


def match_open(s, close):
    pairs = {ord(")"): ord("("), ord("]"): ord("[")}
    depth = 0
    i = close
    while i >= 0:
        c = s[i]
        if c in pairs:
            depth += 1
        elif c in (ord("("), ord("[")):
            depth -= 1
            if depth == 0:
                return i
        i -= 1
    return None


def match_close(s, open_at):
    depth = 0
    i = open_at
    while i < len(s):
        c = s[i]
        if c in b"([":
            depth += 1
        elif c in b")]":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return None


def recv_start(s, end):
    """Início da expressão de acesso (cadeia pós-fixa) que termina em `end` (exclusivo)."""
    i = end
    while True:
        # cadeia quebrada em linhas pelo rustfmt: `x\n    .metodo()`
        while i > 0 and s[i - 1] in b" \t\r\n":
            i -= 1
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
            called = i > 0 and (IDENT.match(s[i - 1 : i]) or s[i - 1] in b")]")
            if s[i] == ord("(") and not called:
                return i
            if s[i] == ord("[") and not called:
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


def collect(diagnostics):
    wanted = defaultdict(dict)
    for raw in open(diagnostics, encoding="utf-8"):
        try:
            message = json.loads(raw)
        except ValueError:
            continue
        if message.get("reason") != "compiler-message":
            continue
        message = message["message"]
        code = (message.get("code") or {}).get("code")
        text = message["message"]
        span = next((sp for sp in message["spans"] if sp["is_primary"]), None)
        if span is None or span["file_name"].startswith("/"):
            continue
        rule = None
        if code == "E0609" and (found := FIELD_RE.match(text)):
            rule = ("field", found.group(1), found.group(2) or "", "")
        elif code == "E0277" and INDEX_RE.match(text):
            rule = ("index", "", "", "")
        elif code == "E0599" and (found := METHOD_RE.match(text)):
            kind = found.group(3).split("<")[0]
            method = found.group(1)
            table = {
                ("StoreStr", "slice"): "str",
                ("StoreSlice", "slice"): "slice",
                ("StoreSlice", "slice_mut"): "slice",
                ("StoreSlice", "iter"): "iter",
                ("StoreSlice", "iter_mut"): "iter",
                ("StoreRef", "get"): "get",
                ("StoreRef", "get_mut"): "get_mut",
            }
            if (kind, method) in table:
                rule = (table[(kind, method)], method, found.group(2) or "", "")
        if rule:
            wanted[span["file_name"]][span["byte_start"]] = (rule, span["byte_end"])
    return wanted


def main():
    diagnostics, root = sys.argv[1], sys.argv[2]
    apply = "--apply" in sys.argv
    totals = defaultdict(int)
    skipped = 0
    for name, edits in sorted(collect(diagnostics).items()):
        path = f"{root}/{name}"
        data = open(path, "rb").read()
        lines = data.splitlines(keepends=True)
        starts = []
        offset = 0
        for line in lines:
            starts.append(offset)
            offset += len(line)
        # (posição, bytes a apagar, texto, ordem entre inserções na mesma posição)
        ops = []
        for byte_start, (rule, byte_end) in edits.items():
            kind, member, ref, _ = rule
            line_index = max(i for i, st in enumerate(starts) if st <= byte_start)
            store = store_name(lines, line_index)
            if kind == "index":
                if store is None and False:
                    pass
                ops.append((byte_start, 0, b"*", 1))
                totals[kind] += 1
                continue
            if byte_start == 0 or data[byte_start - 1] != ord("."):
                skipped += 1
                print(f"pulado (sem ponto): {name}:{line_index + 1} {member}")
                continue
            dot = byte_start - 1
            begin = recv_start(data, dot)
            recv_end = dot
            while recv_end > 0 and data[recv_end - 1] in b" \t\r\n":
                recv_end -= 1
            if begin is None or store is None:
                skipped += 1
                print(f"pulado: {name}:{line_index + 1} {member} (receptor={begin}, store={store})")
                continue
            star = "*" if ref else ""
            if kind == "field":
                ops.append((begin, 0, f"{store}[{star}".encode(), 1))
                ops.append((recv_end, 0, b"]", 0))
            else:
                # métodos: apaga `.nome()` (ou só o ponto, no `iter`) e reescreve em volta do receptor
                call_end = byte_end
                if data[call_end : call_end + 1] == b"(":
                    close = match_close(data, call_end)
                    if close is None:
                        skipped += 1
                        continue
                    call_end = close + 1
                if kind == "str":
                    ops.append((begin, 0, f"{store}.str({star}".encode(), 1))
                    ops.append((recv_end, call_end - recv_end, b")", 0))
                elif kind == "slice":
                    borrow = "&mut " if member == "slice_mut" else "&"
                    ops.append((begin, 0, f"({borrow}{store}[{star}".encode(), 1))
                    ops.append((recv_end, call_end - recv_end, b"])", 0))
                elif kind == "iter":
                    ops.append((begin, 0, f"{store}[{star}".encode(), 1))
                    ops.append((recv_end, dot + 1 - recv_end, b"].", 0))
                elif kind == "get":
                    ops.append((begin, 0, f"(&{store}[{star}".encode(), 1))
                    ops.append((recv_end, call_end - recv_end, b"])", 0))
                elif kind == "get_mut":
                    ops.append((begin, 0, f"(&mut {store}[{star}".encode(), 1))
                    ops.append((recv_end, call_end - recv_end, b"])", 0))
            totals[kind] += 1
        out = bytearray(data)
        for pos, delete, text, _ in sorted(ops, key=lambda x: (x[0], x[3]), reverse=True):
            out[pos : pos + delete] = text
        if apply:
            open(path, "wb").write(bytes(out))
    print(f"trocas: {dict(totals)}, puladas: {skipped}, aplicado: {apply}")


main()
