"""`X.into()` que o upstream convertia de `&[u8]` em `StoreStr` vira `STORE.alloc_str(X)`, com STORE escolhido pela
assinatura da função que envolve o trecho (`p.store` se ela tem `p: &mut P`, ou se houve `let p = self;`; senão
`self.store`). Guiado pelo diagnóstico do rustc (E0277 `StoreStr: From<..>` no `into`)."""
import json, re, sys

FN = re.compile(rb"\bfn\s+[A-Za-z_0-9]+")


def receiver_start(text, dot_at):
    """O começo da expressão `X` em `X.into()`: anda para trás por identificadores, campos, `()`, `[]` e literais."""
    i = dot_at
    while True:
        j = i - 1
        c = text[j:j + 1]
        if c in (b")", b"]"):
            close, opener = c, {b")": b"(", b"]": b"["}[c]
            depth = 0
            while True:
                ch = text[j:j + 1]
                if ch == close:
                    depth += 1
                elif ch == opener:
                    depth -= 1
                    if depth == 0:
                        break
                j -= 1
            i = j
            continue
        if c == b'"':
            j -= 1
            while not (text[j:j + 1] == b'"' and text[j - 1:j] != b"\\"):
                j -= 1
            if text[j - 1:j] == b"b":
                j -= 1
            return j
        m = re.search(rb"[A-Za-z_0-9]+$", text[:i])
        if m:
            i = m.start()
            if text[i - 1:i] in (b".",) or text[i - 2:i] == b"::":
                i -= 1 if text[i - 1:i] == b"." else 2
                continue
            return i
        return i


def store_for(text, at):
    fns = list(FN.finditer(text, 0, at))
    if not fns:
        return None
    sig_start = fns[-1].start()
    body_open = text.index(b"{", sig_start)
    sig = text[sig_start:body_open]
    if re.search(rb"\bp:\s*&mut\b", sig):
        return b"p.store"
    if re.search(rb"let p = self;", text[body_open:at]):
        return b"p.store"
    if b"self" in sig:
        return b"self.store"
    return None


spans = {}
for line in open(sys.argv[1], "rb"):
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("reason") != "compiler-message":
        continue
    d = m["message"]
    if "StoreStr: From" not in d["message"]:
        continue
    s = [x for x in d["spans"] if x["is_primary"]][0]
    spans.setdefault(s["file_name"], set()).add(s["byte_start"])

for f, starts in spans.items():
    text = open(f, "rb").read()
    for into_at in sorted(starts, reverse=True):
        assert text[into_at:into_at + 6] == b"into()", (f, text[into_at:into_at + 10])
        dot = into_at - 1
        start = receiver_start(text, dot)
        store = store_for(text, start)
        if store is None:
            print("manual", f, text.count(b"\n", 0, start) + 1)
            continue
        expr = text[start:dot]
        text = text[:start] + store + b".alloc_str(" + expr + b")" + text[into_at + 6:]
        print(f, text.count(b"\n", 0, start) + 1, expr.decode()[:40], store.decode())
    open(f, "wb").write(text)
