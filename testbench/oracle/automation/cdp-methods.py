#!/usr/bin/env python3
"""Compara os métodos CDP que o cliente (Playwright, pyppeteer) enviou ao Chromium real, nas transcrições
`golden/*.cdp.jsonl`, com os métodos que o servidor do ul-chromium trata (strings `Dominio.metodo` em
`src/devtools/*.rs` e `prelude/*.js`). Mostra os que faltam, por cena, com a contagem de chamadas.
Uso: cdp-methods.py [cena...]  (sem argumento, todas)"""
import collections, glob, json, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.join(HERE, "..", "..", "..", "crates", "ul-chromium")
PATTERN = re.compile(r"""['"]([A-Z][A-Za-z]+\.[a-z][A-Za-z]+)['"]""")


def implemented():
    names = set()
    for pattern in ("src/devtools/*.rs", "prelude/*.js"):
        for path in glob.glob(os.path.join(CRATE, pattern)):
            # `protocol.rs` só valida parâmetros em testes: um nome lá não é um método tratado.
            if path.endswith("protocol.rs"):
                continue
            names.update(PATTERN.findall(open(path, encoding="utf-8").read()))
    return names


def sent(path):
    """Conta os métodos enviados. O pyppeteer usa o protocolo antigo (sem `flatten`): o comando da página vai como
    texto JSON em `Target.sendMessageToTarget`, então o método de dentro também conta (o de fora também, porque o
    servidor precisa tratar os dois)."""
    counts = collections.Counter()
    for line in open(path, encoding="utf-8"):
        message = json.loads(line).get("out")
        if message and "method" in message:
            counts[message["method"]] += 1
            inner = (message.get("params") or {}).get("message") if message["method"] == "Target.sendMessageToTarget" else None
            if isinstance(inner, str) and not inner.startswith("<sha256:"):
                try:
                    counts[json.loads(inner)["method"]] += 1
                except (ValueError, KeyError):
                    pass
    return counts


def main():
    have = implemented()
    wanted = sys.argv[1:] or sorted(os.path.basename(p)[: -len(".cdp.jsonl")] for p in glob.glob(os.path.join(HERE, "golden", "*.cdp.jsonl")))
    total = collections.Counter()
    for name in wanted:
        counts = sent(os.path.join(HERE, "golden", name + ".cdp.jsonl"))
        # `Nao.existe` é o método inventado de propósito (pw_context) para medir o erro de método inexistente.
        missing = {m: n for m, n in counts.items() if m not in have and not m.startswith("Nao.")}
        total.update(missing)
        print(f"{name}: {len(counts)} métodos, faltam {len(missing)}: " + ", ".join(f"{m}({n})" for m, n in sorted(missing.items())))
    print(f"\nfaltam no conjunto ({len(total)}): " + ", ".join(sorted(total)))


main()
