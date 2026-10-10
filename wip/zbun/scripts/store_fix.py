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
            if re.search(rb"\bp\s*:", signature):
                return "p.store"
            if re.search(rb"(&\s*(mut\s+)?|\bmut\s+)self\b|\bself\s*:", signature):
                return "self.store"
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
        if code == "E0609" and re.match(r"^no field `store` on type `&mut (?!P\b)", text):
            rule = ("self_to_p", "", "", "")
        elif code == "E0614" and re.match(r"^type `StoreRef<[^`]*>` cannot be dereferenced", text):
            rule = ("deref", "", "", "")
        elif code == "E0609" and (found := FIELD_RE.match(text)):
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
        elif code == "E0308":
            label = span.get("label") or ""
            if re.match(r"expected `StoreStr`, found `&[^`]*\[u8[^`]*\]`$", label):
                rule = ("wrap_str", "", "", "")
            elif re.match(r"expected `&\[u8\]`, found `StoreStr`$", label):
                rule = ("unwrap_str", "", "", "")
            elif re.match(r"expected `StoreSlice<[^`]*>`, found `Vec<[^`]*>`$", label):
                rule = ("wrap_slice", "", "", "")
        elif code == "E0277" and text.startswith("the trait bound `StoreStr: From<") and span.get("label"):
            rule = ("into_str", "into", "", "")
        elif code == "E0061":
            # a ajuda "provide the argument" traz a lista de argumentos inteira com o marcador do que falta
            for child in message.get("children", []):
                for help_span in child.get("spans", []):
                    placeholder = help_span.get("suggested_replacement") or ""
                    if child.get("message") == "provide the argument" and (
                        placeholder.count("/* ") == 1 and re.search(r"/\* &(mut )?Store \*/", placeholder)
                    ):
                        wanted[help_span["file_name"]][help_span["byte_start"]] = (
                            ("store_arg", placeholder, "", ""),
                            help_span["byte_end"],
                        )
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
            if kind == "self_to_p":
                # método de um auxiliar (não o P) que recebe `p`: o Store é o do `p`
                if data[byte_start - 5 : byte_start] == b"self.":
                    ops.append((byte_start - 5, 4, b"p", 1))
                    totals[kind] += 1
                else:
                    skipped += 1
                    print(f"pulado: {name}:{line_index + 1} self_to_p sem self.")
                continue
            if kind == "deref":
                # `*r` (r: StoreRef) -> `STORE[r]`, em inserções para não colidir com trocas dentro do operando
                if store is None or data[byte_start : byte_start + 1] != b"*":
                    skipped += 1
                    print(f"pulado: {name}:{line_index + 1} deref (store={store})")
                    continue
                ops.append((byte_start, 1, f"{store}[".encode(), 1))
                ops.append((byte_end, 0, b"]", 0))
                totals[kind] += 1
                continue
            if kind == "index":
                ops.append((byte_start, 0, b"*", 1))
                totals[kind] += 1
                continue
            if kind in ("wrap_str", "unwrap_str", "wrap_slice", "store_arg"):
                if store is None:
                    skipped += 1
                    print(f"pulado: {name}:{line_index + 1} {kind} (store=None)")
                    continue
                if kind == "store_arg":
                    # `(a, b)` -> `(&mut p.store, a, b)`, por inserção, e só quando o Store é o primeiro ou o último
                    # argumento da assinatura. A sugestão do rustc também troca a ordem dos outros argumentos quando
                    # os tipos coincidem (dois `Expr`), então ela só serve para achar a posição do marcador.
                    marker = re.search(rb"/\* &(mut )?Store \*/", member.encode()).group(0)
                    sugg_inner = member.encode()[1:-1].strip()
                    placeholder = (b"&mut " if b"mut" in marker else b"&") + store.encode()
                    original_inner = data[byte_start + 1 : byte_end - 1]
                    if data[byte_start : byte_start + 1] != b"(":
                        skipped += 1
                        print(f"pulado: {name}:{line_index + 1} store_arg sem parêntese")
                        continue
                    if not original_inner.strip():
                        ops.append((byte_start + 1, 0, placeholder, 1))
                    elif sugg_inner.startswith(marker):
                        ops.append((byte_start + 1, 0, placeholder + b", ", 1))
                    elif sugg_inner.endswith(marker):
                        end_at = byte_end - 1
                        while data[end_at - 1 : end_at] in (b" ", b"\t", b"\r", b"\n", b","):
                            end_at -= 1
                        ops.append((end_at, 0, b", " + placeholder, 1))
                    else:
                        skipped += 1
                        print(f"pulado: {name}:{line_index + 1} store_arg no meio da lista")
                        continue
                else:
                    before = data[:byte_start].rstrip()
                    if kind == "wrap_str" and before.endswith((b"==", b"!=")):
                        skipped += 1
                        print(f"pulado: {name}:{line_index + 1} wrap_str em comparação (quem compara é o texto, não a faixa)")
                        continue
                    head = {"wrap_str": ".alloc_str(", "unwrap_str": ".str(", "wrap_slice": ".alloc_slice("}[kind]
                    ops.append((byte_start, 0, f"{store}{head}".encode(), 1))
                    ops.append((byte_end, 0, b")", 0))
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
                if kind == "into_str":
                    ops.append((begin, 0, f"{store}.alloc_str({star}".encode(), 1))
                    ops.append((recv_end, call_end - recv_end, b")", 0))
                elif kind == "str":
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
        # o argumento do Store troca a lista de argumentos inteira: se outra troca cai dentro dela, fica para a
        # rodada seguinte (as trocas internas entram agora e a do argumento depois)
        others = [op for op in ops if op[3] != 2]
        ops = others + [
            op for op in ops if op[3] == 2 and not any(op[0] < other[0] < op[0] + op[1] for other in others)
        ]
        out = bytearray(data)
        for pos, delete, text, _ in sorted(ops, key=lambda x: (x[0], x[3]), reverse=True):
            out[pos : pos + delete] = text
        if apply:
            open(path, "wb").write(bytes(out))
    print(f"trocas: {dict(totals)}, puladas: {skipped}, aplicado: {apply}")


CTOR_RE = re.compile(
    rb"(?P<path>(?:\b[A-Za-z_][A-Za-z0-9_]*::)*(?:StoreStr|StoreSlice|StoreRef|ExprNodeList|StmtNodeList|DeclList|PropertyList"
    rb"|ArgList|PartImportRecordIndices|ClauseItemList))::(?P<name>new|new_mut|from_bump|from_bump_vec|from_arena_slice"
    rb"|from_slice|move_from_list|init_one)\("
)
BUMP_SUFFIX = re.compile(rb"\.into_bump_slice(_mut)?\(\)\s*$")
ARENA_ALLOC = re.compile(rb"^\s*(?:self\.|p\.)?(?:arena|bump)\.alloc\(")


def ctors(root):
    """Construtores de lista e de string que apontavam para a arena viram gravações no Store.

      StoreStr::new(a)                      ->  STORE.alloc_str(a)
      Lista::{new_mut,from_bump}(a)         ->  STORE.alloc_slice(a)       (sem o `.into_bump_slice[_mut]()`)
      Lista::{from_bump_vec,move_from_list} ->  STORE.alloc_slice(a)
      Lista::{from_arena_slice,from_slice}  ->  STORE.alloc_slice(a.to_vec())
      Lista::init_one(a)                    ->  STORE.alloc_slice([a])
      StoreRef::from_bump(arena.alloc(a))   ->  STORE.alloc(a)
    """
    import os

    total = skipped = 0
    for folder, _, names in os.walk(f"{root}/js_parser/src"):
        for name in names:
            if not name.endswith(".rs"):
                continue
            path = f"{folder}/{name}"
            data = open(path, "rb").read()
            lines = data.splitlines(keepends=True)
            starts = []
            offset = 0
            for line in lines:
                starts.append(offset)
                offset += len(line)
            ops = []
            for found in CTOR_RE.finditer(data):
                open_at = found.end() - 1
                close = match_close(data, open_at)
                line_index = max(i for i, st in enumerate(starts) if st <= found.start())
                store = store_name(lines, line_index)
                if close is None or store is None:
                    skipped += 1
                    print(f"pulado: {path}:{line_index + 1} {found.group('name').decode()} (store={store})")
                    continue
                kind, member = found.group("path").split(b"::")[-1], found.group("name")
                argument = data[open_at + 1 : close]
                if kind == b"StoreStr" and member == b"new":
                    head, tail = f"{store}.alloc_str(".encode(), b")"
                elif kind == b"StoreRef" and member == b"from_bump":
                    inner = ARENA_ALLOC.match(argument)
                    if not inner:
                        skipped += 1
                        print(f"pulado: {path}:{line_index + 1} StoreRef::from_bump sem arena.alloc")
                        continue
                    argument = argument[inner.end() : argument.rstrip().rfind(b")")]
                    head, tail = f"{store}.alloc(".encode(), b")"
                elif member in (b"new_mut", b"from_bump", b"from_bump_vec", b"move_from_list"):
                    argument = BUMP_SUFFIX.sub(b"", argument)
                    head, tail = f"{store}.alloc_slice(".encode(), b")"
                elif member in (b"from_arena_slice", b"from_slice"):
                    head, tail = f"{store}.alloc_slice((".encode(), b").to_vec())"
                elif member == b"init_one":
                    head, tail = f"{store}.alloc_slice([".encode(), b"])"
                else:
                    continue
                ops.append((found.start(), close + 1 - found.start(), head + argument + tail))
                total += 1
            # construtor dentro do argumento de outro: fica para a rodada seguinte
            ops.sort()
            kept = []
            for op in ops:
                if kept and op[0] < kept[-1][0] + kept[-1][1]:
                    continue
                kept.append(op)
            ops = kept
            out = bytearray(data)
            for pos, delete, text in sorted(ops, reverse=True):
                out[pos : pos + delete] = text
            if ops and "--apply" in sys.argv:
                open(path, "wb").write(bytes(out))
    print(f"construtores: {total}, pulados: {skipped}, aplicado: {'--apply' in sys.argv}")


if len(sys.argv) > 2 and sys.argv[1] == "ctors":
    ctors(sys.argv[2])
else:
    main()
