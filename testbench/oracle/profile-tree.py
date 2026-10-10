#!/usr/bin/env python3
"""Lista a árvore de um perfil do Chromium em JSON, uma entrada por linha, para a medição e o teste diferencial.

Uso: profile-tree.py <diretório>

Cada linha: {"path", "type" (d, f, l, o para socket e afins), "mode" (octal), "size", "link" (alvo do symlink), "text" (conteúdo dos
arquivos de até 4096 bytes, em latin-1 para não perder byte)}. A ordem é a do caminho, para a diferença entre duas
execuções ser um diff de texto.
"""
import json
import os
import stat
import sys


def walk(root):
    entries = []
    for current, dirs, files in os.walk(root):
        for name in dirs + files:
            full = os.path.join(current, name)
            entries.append(full)
    for full in sorted(entries):
        rel = os.path.relpath(full, root)
        try:
            info = os.lstat(full)
        except OSError:
            continue
        kind = "l" if stat.S_ISLNK(info.st_mode) else "d" if stat.S_ISDIR(info.st_mode) else "f" if stat.S_ISREG(info.st_mode) else "o"
        entry = {"path": rel, "type": kind, "mode": oct(stat.S_IMODE(info.st_mode)), "size": info.st_size}
        if kind == "l":
            entry["link"] = os.readlink(full)
        elif kind == "f" and info.st_size <= 4096:
            with open(full, "rb") as handle:
                entry["text"] = handle.read().decode("latin-1")
        print(json.dumps(entry, ensure_ascii=True, sort_keys=True))


if __name__ == "__main__":
    walk(sys.argv[1])
