"""Torna determinísticos os casos do tempfile e do savelog (transformação em massa nos .toml).

tempfile: os nomes aleatórios ficam no diretório de trabalho e entram na comparação de arquivos;
cada script que cria arquivos passa a apagá-los no fim, preservando o código de saída.
savelog: o gzip grava o mtime do arquivo no cabeçalho; cada `> log` ganha um touch com data fixa.
"""
import re
import sys

base = "/home/john/projects/pseudo-linus/testbench/corpus/cases/utillinux/"

path = base + "tempfile.toml"
text = open(path).read()
creators = re.compile(r"tempfile -d|--dir=|TMPDIR=|mkdir ")


def fix_tempfile(m):
    body = m.group(1)
    if not creators.search(body) or "rm -rf -- ./*" in body:
        return m.group(0)
    return 'script = "' + body + '; rc=$?; rm -rf -- ./*; exit $rc"'


new = re.sub(r'^script = "(.*)"$', fix_tempfile, text, flags=re.M)
changed = sum(1 for a, b in zip(text.splitlines(), new.splitlines()) if a != b)
open(path, "w").write(new)
print("tempfile:", changed)

path = base + "savelog.toml"
text = open(path).read()
new = re.sub(r"> log;(?! touch)", "> log; touch -d '2026-01-15 12:00:00' log;", text)
print("savelog:", len(re.findall(r"> log; touch -d '2026-01-15", new)) - len(re.findall(r"> log; touch -d '2026-01-15", text)))
open(path, "w").write(new)
sys.exit(0)
