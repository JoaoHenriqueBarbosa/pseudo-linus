#!/usr/bin/env python3
"""Mostra entradas de uma medição do measure-profile.sh.

Uso: profile-show.py <medição.txt> <seção> [caminho ...]
Sem caminhos, lista a raiz (entradas sem barra). Com caminhos, mostra os metadados e o texto de cada um.
"""
import json
import sys


def main():
    path, label, wanted = sys.argv[1], sys.argv[2], set(sys.argv[3:])
    current = None
    for line in open(path):
        line = line.rstrip("\n")
        if line.startswith("=="):
            current = line[3:].strip()
        elif line.startswith("{") and current == label:
            entry = json.loads(line)
            if wanted:
                if entry["path"] in wanted:
                    print(entry["path"], entry["type"], entry["mode"], entry["size"], entry.get("link", ""))
                    print(entry.get("text", ""))
            elif "/" not in entry["path"]:
                print(entry["type"], entry["mode"], entry["size"], entry["path"])


if __name__ == "__main__":
    main()
