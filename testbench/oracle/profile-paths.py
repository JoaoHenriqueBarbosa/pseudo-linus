#!/usr/bin/env python3
"""Compara os caminhos de duas listagens de perfil (profile-tree.py ou saída do measure-profile.sh).

Uso: profile-paths.py <A> [seção-de-A] <B> [seção-de-B]   (a seção só vale para arquivos de medição com "== ")
Imprime os caminhos só em A, só em B, e as contagens.
"""
import json
import sys


def paths(file, section=None):
    result = {}
    current = None
    for line in open(file):
        line = line.rstrip("\n")
        if line.startswith("=="):
            current = line[3:].strip()
        elif line.startswith("{") and (section is None or current == section):
            entry = json.loads(line)
            result[entry["path"]] = entry
    return result


def main():
    args = sys.argv[1:]
    a_file, args = args[0], args[1:]
    a_section = None
    if args and not args[0].endswith((".txt", ".jsonl")):
        a_section, args = args[0], args[1:]
    b_file, args = args[0], args[1:]
    b_section = args[0] if args else None
    a, b = paths(a_file, a_section), paths(b_file, b_section)
    only_a, only_b = sorted(set(a) - set(b)), sorted(set(b) - set(a))
    print(f"A={len(a)} B={len(b)} comuns={len(set(a) & set(b))}")
    for p in only_a:
        print("só em A:", p, a[p]["type"], a[p]["size"])
    for p in only_b:
        print("só em B:", p, b[p]["type"], b[p]["size"])


if __name__ == "__main__":
    main()
