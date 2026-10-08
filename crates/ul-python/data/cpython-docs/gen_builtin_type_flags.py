"""Gera `builtin-type-flags.tsv`: `tipo<TAB>tipo.__flags__` em hexadecimal.

Roda no oráculo da bancada, com os nomes de tipo de `builtin-type-dir.tsv` (primeira coluna):

    docker run --rm -i -v "$PWD/crates/ul-python/data/cpython-docs:/g:ro" pseudo-linus-oracle:dev \
        python3 -I /g/gen_builtin_type_flags.py /g/builtin-type-dir.tsv \
        > crates/ul-python/data/cpython-docs/builtin-type-flags.tsv
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_common

print('# Gerado no oráculo (python3.13 do Debian 13) por gen_builtin_type_flags.py: tipo<TAB>__flags__.')
for name in gen_common.type_names(sys.argv[1]):
    t = gen_common.resolve(name)
    if t is not None:
        print('%s\t%#x' % (name, t.__flags__))
