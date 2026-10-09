#!/usr/bin/env python3
"""Monta a fita de uma SPA do laboratório a partir dos arquivos dela, sem rede: cada arquivo de
testbench/corpus/lab/<id>/ vira uma entrada servida em https://lab.test/<id>/<caminho>, e os de
testbench/corpus/lab/<id>/cdn/ em https://cdn.lab.test/<caminho> (origem cruzada, para os casos de CORS).

O tipo vem da extensão. Um arquivo `<nome>.headers` ao lado do arquivo acrescenta cabeçalhos (`Nome: valor` por
linha, inclusive `Status: 404`). O relógio de gravação é fixo, então o golden não depende de quando se gera.

Uso: lab-tape.py <id>   (escreve testbench/corpus/internet/<id>/tape.json e bodies/)
"""
import hashlib, json, os, sys

RECORDED_AT = 1791500000
TYPES = {".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".mjs": "text/javascript; charset=utf-8",
         ".css": "text/css; charset=utf-8", ".json": "application/json", ".txt": "text/plain; charset=utf-8",
         ".svg": "image/svg+xml", ".wasm": "application/wasm", ".bin": "application/octet-stream", ".xml": "application/xml"}

root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "corpus"))
lab_id = sys.argv[1]
source = os.path.join(root, "lab", lab_id)
out = os.path.join(root, "internet", lab_id)
os.makedirs(os.path.join(out, "bodies"), exist_ok=True)
for stale in os.listdir(os.path.join(out, "bodies")):
    os.remove(os.path.join(out, "bodies", stale))

entries = []
for directory, _, files in sorted(os.walk(source)):
    for name in sorted(files):
        if name.endswith(".headers"):
            continue
        path = os.path.join(directory, name)
        rel = os.path.relpath(path, source).replace(os.sep, "/")
        if rel.startswith("cdn/"):
            url = "https://cdn.lab.test/" + rel[len("cdn/"):]
        else:
            url = f"https://lab.test/{lab_id}/" + ("" if rel == "index.html" else rel)
        status = 200
        method = "GET"
        headers = [["content-type", TYPES.get(os.path.splitext(name)[1], "application/octet-stream")]]
        extra = path + ".headers"
        if os.path.exists(extra):
            for line in open(extra, encoding="utf-8").read().splitlines():
                if not line.strip():
                    continue
                key, value = line.split(":", 1)
                if key.strip().lower() == "status":
                    status = int(value)
                elif key.strip().lower() == "method":
                    method = value.strip().upper()
                else:
                    headers = [h for h in headers if h[0] != key.strip().lower()] + [[key.strip().lower(), value.strip()]]
        data = open(path, "rb").read()
        digest = hashlib.sha256(data).hexdigest()
        with open(os.path.join(out, "bodies", digest), "wb") as f:
            f.write(data)
        entries.append({"method": method, "url": url, "status": status, "headers": headers, "body": digest})

with open(os.path.join(out, "tape.json"), "w", encoding="utf-8") as f:
    json.dump({"url": f"https://lab.test/{lab_id}/", "recorded_at": RECORDED_AT, "entries": entries}, f, indent=1, ensure_ascii=False)
    f.write("\n")
print(f"{lab_id}: {len(entries)} entradas")
