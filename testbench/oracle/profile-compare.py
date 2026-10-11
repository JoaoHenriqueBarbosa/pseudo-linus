#!/usr/bin/env python3
"""Compara várias medições do measure-profile.sh e diz o que é estável e o que varia entre execuções.

Uso: profile-compare.py <rótulo-da-seção> <medição.txt> <medição.txt> ...

Para cada caminho: AUSENTE se falta em alguma execução, VARIA com os campos que mudam (type, mode, size, text, link).
"""
import json
import sys


def load(path):
    sections = {}
    current = None
    for line in open(path):
        line = line.rstrip("\n")
        if line.startswith("=="):
            current = line[3:].strip()
            sections[current] = {}
        elif line.startswith("{") and current is not None:
            entry = json.loads(line)
            sections[current][entry["path"]] = entry
    return sections


def main():
    label = sys.argv[1]
    runs = [load(path)[label] for path in sys.argv[2:]]
    paths = set()
    for run in runs:
        paths |= set(run)
    print(f"## {label}: {len(paths)} caminhos, por execução {[len(run) for run in runs]}")
    for path in sorted(paths):
        entries = [run.get(path) for run in runs]
        if any(entry is None for entry in entries):
            print("AUSENTE", path, [entry is not None for entry in entries])
            continue
        varying = [key for key in ("type", "mode", "size", "text", "link") if len({json.dumps(e.get(key)) for e in entries}) > 1]
        if varying:
            print("VARIA", varying, path, [e["size"] for e in entries])


if __name__ == "__main__":
    main()
