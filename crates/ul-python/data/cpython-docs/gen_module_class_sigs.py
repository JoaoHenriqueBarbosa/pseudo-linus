"""Gera `module-class-sigs.tsv`: `módulo<TAB>tipo<TAB>assinatura` com o `__text_signature__` dos tipos definidos
nos módulos em C (os de `__module__` igual ao nome do módulo, mais `builtins.memoryview`) e nos módulos Python do
CPython que reexportam tipos de C (`functools`, `collections`, `operator`, `itertools`, `decimal`, `types`). Só entram
os tipos que têm assinatura. Roda no oráculo da bancada:

    docker run --rm -i -v "$PWD/crates/ul-python/data/cpython-docs:/g:ro" pseudo-linus-oracle:dev \
        python3 -I /g/gen_module_class_sigs.py \
        > crates/ul-python/data/cpython-docs/module-class-sigs.tsv
"""
import importlib
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_common

PYTHON_FRONTS = ['functools', 'collections', 'operator', 'itertools', 'decimal', 'types', 'posix', '_thread']

print('# Gerado no oráculo (python3.13 do Debian 13) por gen_module_class_sigs.py: módulo<TAB>tipo<TAB>assinatura.')
rows = set()
for m in gen_common.c_modules() + [importlib.import_module(n) for n in PYTHON_FRONTS] + [importlib.import_module('builtins')]:
    for value in vars(m).values():
        if not isinstance(value, type):
            continue
        sig = getattr(value, '__text_signature__', None)
        if sig is None:
            continue
        if value.__module__ in (m.__name__, 'builtins') or m.__name__ in PYTHON_FRONTS:
            rows.add((value.__module__, value.__name__, sig))
for module, name, sig in sorted(rows):
    print(module + '\t' + name + '\t' + sig)
