"""Gera tests/corpus/<caso>/expected/{stdout,stderr,code} no oráculo (bun 1.4.2, imagem pl-oracle-bun-check).

Cada caso tem `cmd.json` (o argv depois de `bun`) e `files/` (montado em /app, que é também o diretório de trabalho
do kernel de teste). Uso, da raiz do repositório: python3 -I crates/ul-bun/scripts/gen-corpus.py [caso...]

O stdout vai para um arquivo, como `bun a.js > saida`; um caso com o arquivo `stdout-pipe` ao lado de `cmd.json` roda com o stdin e o
stdout num pipe (`bun a.js | cat`), que é o que o kernel de teste dá ao programa (um `Bun.write(Bun.stdout, ...)` num
arquivo escreve no deslocamento 0 e embaralha a saída).
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
    piped = os.path.exists(os.path.join(case, 'stdout-pipe'))
    script = ('{ : | bun "$@" 2>/out/stderr; echo $? >/out/code; } | cat >/out/stdout' if piped
              else 'bun "$@" >/out/stdout 2>/out/stderr; echo $? >/out/code')
    subprocess.run(
        ['docker', 'run', '--rm', '--entrypoint', '', '-v', os.path.join(case, 'files') + ':/app', '-v', out + ':/out', '-w', '/app',
         'pl-oracle-bun-check', 'sh', '-c', script, 'sh'] + argv,
        check=True, timeout=120)
    print(name, open(os.path.join(out, 'code')).read().strip())
