#!/usr/bin/env python3
"""Compara dois JSON de perfil (Local State, Preferences, Secure Preferences) e lista os caminhos que diferem.

Uso: profile-jsondiff.py <a.json> <b.json> [c.json ...]
Imprime, para cada caminho de chave cujo valor muda entre os arquivos, os valores de cada um.
"""
import json
import sys


def flatten(value, prefix=""):
    if isinstance(value, dict):
        for key, item in value.items():
            yield from flatten(item, f"{prefix}/{key}")
    elif isinstance(value, list) and value and not all(isinstance(i, (int, float, str)) for i in value):
        for index, item in enumerate(value):
            yield from flatten(item, f"{prefix}[{index}]")
    else:
        yield prefix, value


def main():
    flats = [dict(flatten(json.load(open(path)))) for path in sys.argv[1:]]
    keys = set()
    for flat in flats:
        keys |= set(flat)
    for key in sorted(keys):
        values = [flat.get(key, "<ausente>") for flat in flats]
        if len({json.dumps(v) for v in values}) > 1:
            print(key, values)
    print(f"# {len(keys)} chaves, {sum(1 for k in keys if len({json.dumps(f.get(k, '<a>')) for f in flats}) > 1)} variam")


if __name__ == "__main__":
    main()
