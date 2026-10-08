"""Gera `module-function-sigs.tsv`: `módulo<TAB>função[<TAB>assinatura]` com o `__text_signature__` das funções
de módulo em C (`builtin_function_or_method`), sem assinatura quando a função não tem. Roda no oráculo da bancada:

    docker run --rm -i -v "$PWD/crates/ul-python/data/cpython-docs:/g:ro" pseudo-linus-oracle:dev \
        python3 -I /g/gen_module_function_sigs.py \
        > crates/ul-python/data/cpython-docs/module-function-sigs.tsv
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_common

print('# Gerado no oráculo (python3.13 do Debian 13): módulo<TAB>função[<TAB>assinatura].')
for m in gen_common.c_modules():
    for attr in sorted(vars(m)):
        v = vars(m)[attr]
        if type(v).__name__ != 'builtin_function_or_method':
            continue
        sig = getattr(v, '__text_signature__', None)
        print(m.__name__ + '\t' + attr + ('\t' + sig if sig is not None else ''))
