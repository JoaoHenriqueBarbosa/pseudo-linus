"""Lê as saídas do bundle do oráculo em OUT e grava os `.expected` do teste de mapa em DIR (ver gen_maps.sh)."""
import json
import os
import sys

out, dest = sys.argv[1], sys.argv[2]
for entry in sorted(os.listdir(out)):
    if not entry.endswith(".d"):
        continue
    name = entry[:-2]
    code_path = os.path.join(out, entry, name)
    map_path = code_path + ".map"
    if not (os.path.exists(code_path) and os.path.exists(map_path)):
        print("sem saída para", name)
        continue
    code = open(code_path, encoding="utf-8").read()
    lines = code.split("\n")
    # O cabeçalho do chunk é a primeira linha (`// ../w/maps/x.js`); no fim vêm uma linha em branco e o debugId.
    assert lines[0].startswith("// "), lines[0]
    body = "\n".join(lines[1:])
    marker = "\n//# debugId="
    body = body[: body.index(marker)]
    assert body.endswith("\n"), "o corpo do chunk não termina em quebra de linha"
    body = body[:-1] if body.endswith("\n\n") else body
    mappings = json.load(open(map_path, encoding="utf-8"))["mappings"]
    open(os.path.join(dest, name + ".bundle.expected"), "w", encoding="utf-8").write(body)
    open(os.path.join(dest, name + ".mappings.expected"), "w", encoding="utf-8").write(mappings)
    print("ok", name, len(lines) - 1, "linhas")
