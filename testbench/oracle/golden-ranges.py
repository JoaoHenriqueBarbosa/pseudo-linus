"""Intervalos do golden da internet: o que o próprio oráculo não repete entre rodadas.

Uso: golden-ranges.py <saída> <rodada> [<rodada> ...]

O despejo do DOM se divide em texto e números (`tokens`). Um número varia quando muda entre as rodadas que têm a mesma
quantidade de números e, em todas elas, o mesmo texto logo antes e logo depois dele; o intervalo dele é o mínimo e o
máximo observados nessas rodadas. Cada texto distinto do oráculo (fora os números) que tem algum número variável vira
um grupo: o placar aceita o despejo com esse texto, cada número exato igual ao da rodada e cada número variável dentro
do intervalo. Nada é estimado nem alargado: o intervalo é só o que o oráculo mostrou, no mesmo lugar do documento.

A regra do token tem cópia exata em `crates/ul-chromium/tests/internet.rs` (`tokens`).
"""

import json
import sys


def tokens(text):
    """Divide em (pedaços de texto, números): número começa num dígito, ou num `-` ou `.` seguidos de dígito, que não
    vem depois de letra, dígito, `_` ou `.`; segue com dígitos, uma fração e um expoente."""
    pieces, numbers = [], []
    start = 0
    i = 0
    n = len(text)

    def word(c):
        return c.isascii() and (c.isalnum() or c == "_" or c == ".")

    while i < n:
        c = text[i]
        prev_ok = i == 0 or not word(text[i - 1])
        j = i
        if prev_ok and c == "-" and i + 1 < n and (text[i + 1].isdigit() and text[i + 1].isascii()):
            j = i + 1
        if prev_ok and j < n and (text[j].isascii() and text[j].isdigit() or (text[j] == "." and j + 1 < n and text[j + 1].isascii() and text[j + 1].isdigit())):
            k = j
            while k < n and text[k].isascii() and text[k].isdigit():
                k += 1
            if k + 1 < n and text[k] == "." and text[k + 1].isascii() and text[k + 1].isdigit():
                k += 1
                while k < n and text[k].isascii() and text[k].isdigit():
                    k += 1
            if k < n and text[k] in "eE":
                e = k + 1
                if e < n and text[e] in "+-":
                    e += 1
                if e < n and text[e].isascii() and text[e].isdigit():
                    while e < n and text[e].isascii() and text[e].isdigit():
                        e += 1
                    k = e
            pieces.append(text[start:i])
            numbers.append(text[i:k])
            start = k
            i = k
            continue
        i += 1
    pieces.append(text[start:])
    return pieces, numbers


def main():
    out = sys.argv[1]
    runs = []
    for path in sys.argv[2:]:
        with open(path, encoding="utf-8", errors="surrogateescape") as handle:
            text = handle.read()
        if text:
            runs.append(tokens(text))
    # A variação de cada número se aprende em todas as rodadas com a mesma quantidade de números, e só onde o texto
    # vizinho (o pedaço antes e o depois) é idêntico em todas elas: a ordem de rede e o instante de uma animação são
    # variações independentes, e uma rodada com outra ordem continua mostrando o mesmo número no mesmo lugar.
    variable = {}
    for count in {len(numbers) for _, numbers in runs}:
        same = [run for run in runs if len(run[1]) == count]
        for i in range(count):
            values = [numbers[i] for _, numbers in same]
            if len(set(values)) == 1:
                continue
            if len({pieces[i] for pieces, _ in same}) != 1 or len({pieces[i + 1] for pieces, _ in same}) != 1:
                continue
            floats = [float(v) for v in values]
            variable[(count, i)] = {"min": min(floats), "max": max(floats)}
    groups = {}
    for pieces, numbers in runs:
        groups.setdefault(json.dumps(pieces), []).append(numbers)
    written = 0
    for key, members in groups.items():
        count = len(members[0])
        slots = [variable.get((count, i), {"exact": number}) for i, number in enumerate(members[0])]
        if all("exact" in slot for slot in slots):
            continue
        name = out if written == 0 else out.replace(".ranges.json", f".ranges.{written}.json")
        with open(name, "w", encoding="utf-8", errors="surrogateescape") as handle:
            json.dump({"runs": len(members), "pieces": json.loads(key), "numbers": slots}, handle, ensure_ascii=False)
        written += 1
    print(written)


if __name__ == "__main__":
    main()
