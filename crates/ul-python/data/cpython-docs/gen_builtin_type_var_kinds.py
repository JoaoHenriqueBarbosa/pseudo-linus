"""Gera `builtin-type-var-kinds.tsv`: `tipo<TAB>chave=tipo-do-descritor ...`, na ordem de `vars(tipo)`.

O tipo é `type(vars(T)[chave]).__name__` (`wrapper_descriptor`, `method_descriptor`, `member_descriptor`,
`getset_descriptor`, `classmethod_descriptor`, `staticmethod`, `builtin_function_or_method`...): o que
`getattr(T, chave)` esconde, porque ele já aplica o `__get__`. Usa os mesmos nomes de tipo (`gen_common`) de
`gen_builtin_type_vars.py`. Roda no oráculo da bancada:

    docker run --rm -i -v "$PWD/crates/ul-python/data/cpython-docs:/g:ro" pseudo-linus-oracle:dev \
        python3 -I /g/gen_builtin_type_var_kinds.py /g/builtin-type-dir.tsv \
        > crates/ul-python/data/cpython-docs/builtin-type-var-kinds.tsv
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_common

print('# Gerado no oráculo (python3.13 do Debian 13) por gen_builtin_type_var_kinds.py: '
      'tipo<TAB>chave=tipo-do-descritor separado por espaço.')
for name in gen_common.type_names(sys.argv[1]):
    t = gen_common.resolve(name)
    if t is not None:
        print(name + '\t' + ' '.join('%s=%s' % (k, type(v).__name__) for k, v in vars(t).items()))
