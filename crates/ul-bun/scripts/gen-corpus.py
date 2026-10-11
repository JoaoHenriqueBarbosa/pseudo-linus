"""Gera tests/corpus/<caso>/expected/{stdout,stderr,code} no oráculo (bun 1.4.2, imagem pl-oracle-bun-check).

Cada caso tem `cmd.json` (o argv depois de `bun`) e `files/` (montado em /app, que é também o diretório de trabalho
do kernel de teste). Uso, da raiz do repositório: python3 -I crates/ul-bun/scripts/gen-corpus.py [caso...]
"""
import json
import os
import subprocess
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'tests', 'corpus')
ROOT = os.path.abspath(ROOT)
names = sys.argv[1:] or sorted(os.listdir(ROOT))
for name in names:
    case = os.path.join(ROOT, name)
    argv = json.load(open(os.path.join(case, 'cmd.json')))
    out = os.path.join(case, 'expected')
    os.makedirs(out, exist_ok=True)
    subprocess.run(
        ['docker', 'run', '--rm', '--entrypoint', '', '-v', os.path.join(case, 'files') + ':/app', '-v', out + ':/out', '-w', '/app',
         'pl-oracle-bun-check', 'sh', '-c', 'bun "$@" >/out/stdout 2>/out/stderr; echo $? >/out/code', 'sh'] + argv,
        check=True, timeout=120)
    print(name, open(os.path.join(out, 'code')).read().strip())
