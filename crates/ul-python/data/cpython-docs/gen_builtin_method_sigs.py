"""Gera `builtin-method-sigs.tsv`: `tipo<TAB>nome[<TAB>assinatura]` com o `__text_signature__` dos chamáveis do
`vars(tipo)` de cada tipo embutido (sem assinatura, o chamável não tem), mais a linha `tipo<TAB>__text_signature__
<TAB>assinatura` com a do próprio tipo (`float.__text_signature__`, `(x=0, /)`). Os tipos são os de
`builtin-type-dir.tsv` (as exceções, os iteradores e as visões de `dict` entram por ele) e os de `EXTRA`.
Roda no oráculo da bancada:

    docker run --rm -i -v "$PWD/crates/ul-python/data/cpython-docs:/g:ro" pseudo-linus-oracle:dev \
        python3 -I /g/gen_builtin_method_sigs.py /g/builtin-type-dir.tsv \
        > crates/ul-python/data/cpython-docs/builtin-method-sigs.tsv
"""
import os
import sys
import types

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_common

# Tipos que o `dir()` dos valores embutidos não lista mas o interpretador consulta.
EXTRA = [types.ModuleType, types.SimpleNamespace, types.MappingProxyType]

print('# Gerado no oráculo (python3.13 do Debian 13): tipo<TAB>nome[<TAB>assinatura].')
seen = []
for t in [gen_common.resolve(n) for n in gen_common.type_names(sys.argv[1])] + EXTRA:
    if t is None or t in seen:
        continue
    seen.append(t)
    own = getattr(t, '__text_signature__', None)
    if own is not None:
        print(t.__name__ + '\t__text_signature__\t' + own)
    for name in sorted(vars(t)):
        v = vars(t)[name]
        if not callable(v) and not isinstance(v, (classmethod, staticmethod)):
            continue
        sig = getattr(v, '__text_signature__', None)
        if sig is None and isinstance(v, (classmethod, staticmethod)):
            sig = getattr(getattr(t, name), '__text_signature__', None)
        print(t.__name__ + '\t' + name + ('\t' + sig if sig is not None else ''))
