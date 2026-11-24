#!/usr/bin/env python3
"""Lista os `cfg` do repositório que avaliam diferente entre host linux (glibc) e host android.

O sandbox é um Debian 13 visto de dentro, qualquer que seja o host. O código vendorizado (uutils
coreutils e findutils, stylo) escolhe ramos por `target_os`; com `--target *-linux-android` cada
`target_os = "linux"` sem o android ao lado, e cada ramo próprio de `target_os = "android"`, muda
o que o programa faz dentro do sandbox. Este script avalia todo predicado `cfg(...)`,
`cfg_attr(...)` e `cfg!(...)` dos `.rs`, e as tabelas `[target.'cfg(...)']` dos `Cargo.toml`, nos
dois hosts e imprime os que divergem. Sai com 1 se houver algum.

    scripts/cfg-android-check.py            # crates/ e vendor/
    scripts/cfg-android-check.py DIR...     # diretórios escolhidos

`tests/`, `benches/`, `staging/`, `fuzz/` e `target/` ficam de fora, como no dry-check.
"""
import os
import re
import sys

SKIP = ("/tests/", "/benches/", "/staging/", "/fuzz/", "/target/")
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def skip_ws(t, i):
    while True:
        while i < len(t) and t[i].isspace():
            i += 1
        if t.startswith("//", i):
            i = t.index("\n", i)
            continue
        return i


def parse(t, i):
    """Um predicado a partir de `i`: devolve (nó, posição depois dele)."""
    i = skip_ws(t, i)
    st = i
    while i < len(t) and (t[i].isalnum() or t[i] == "_"):
        i += 1
    name = t[st:i]
    if not name:
        raise ValueError(t[st : st + 30])
    j = skip_ws(t, i)
    if j < len(t) and t[j] == "(":
        j += 1
        args = []
        while True:
            j = skip_ws(t, j)
            if t[j] == ")":
                return (name, args), j + 1
            a, j = parse(t, j)
            args.append(a)
            j = skip_ws(t, j)
            if t[j] == ",":
                j += 1
    if j < len(t) and t[j] == "=":
        j = skip_ws(t, j + 1)
        k = t.index('"', j + 1)
        return ("atom", name + "=" + t[j : k + 1]), k + 1
    return ("atom", name), i


# O que cada host responde aos átomos de plataforma; o resto (features, arquitetura, largura do
# ponteiro) vale igual nos dois e fica simbólico.
HOSTS = {
    "linux": {"target_os": "linux", "target_env": "gnu", "target_vendor": "unknown", "target_family": "unix"},
    "android": {"target_os": "android", "target_env": "", "target_vendor": "unknown", "target_family": "unix"},
}


def ev(node, host):
    kind, val = node
    if kind == "atom":
        if val in ("unix", "true"):
            return True
        if val in ("windows", "false"):
            return False
        key, eq, lit = val.partition("=")
        if eq and key in HOSTS[host]:
            return lit == f'"{HOSTS[host][key]}"'
        return val
    args = [ev(a, host) for a in val]
    if kind == "not":
        return (not args[0]) if isinstance(args[0], bool) else ("not", args[0])
    if kind in ("any", "all"):
        absorb = kind == "any"
        if any(a is absorb for a in args):
            return absorb
        rest = sorted({repr(a) for a in args if not isinstance(a, bool)})
        if not rest:
            return not absorb
        return rest[0] if len(rest) == 1 else (kind, tuple(rest))
    raise ValueError(kind)


RS = re.compile(r"\bcfg(?:_attr)?!?\s*\(")
TOML = re.compile(r"""^\[target\.['"]cfg\(""", re.M)


def scan(path):
    text = open(path, encoding="utf-8", errors="replace").read()
    if "target_os" not in text and "target_env" not in text:
        return
    for m in (RS if path.endswith(".rs") else TOML).finditer(text):
        line = text.count("\n", 0, m.start()) + 1
        try:
            node, end = parse(text, m.end())
        except ValueError as e:
            yield line, f"não deu para ler o predicado: {e}"
            continue
        if repr(ev(node, "linux")) != repr(ev(node, "android")):
            yield line, " ".join(text[m.end() : end].split())


def main():
    roots = sys.argv[1:] or [os.path.join(ROOT, "crates"), os.path.join(ROOT, "vendor")]
    found = 0
    for root in roots:
        for dirpath, _, files in os.walk(root):
            for f in sorted(files):
                path = os.path.join(dirpath, f)
                if not (f.endswith(".rs") or f == "Cargo.toml") or any(s in path + "/" for s in SKIP):
                    continue
                for line, what in scan(path):
                    found += 1
                    print(f"{os.path.relpath(path, ROOT)}:{line}: {what}")
    if found:
        print(f"{found} cfg avaliam diferente entre host linux e host android", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
