"""pkgutil enxuto: `resolve_name` e `get_data` mínimo."""

import importlib
import re

__all__ = ['resolve_name']

_NAME_PATTERN = re.compile(r'^(?P<pkg>(?!\d)(\w+)(\.(?!\d)(\w+))*)(?P<cln>:(?P<obj>(?!\d)(\w+)(\.(?!\d)(\w+))*)?)?$')


def resolve_name(name):
    """Resolve `pacote.modulo` ou `pacote.modulo:objeto.atributo`."""
    m = _NAME_PATTERN.match(name)
    if not m:
        raise ValueError(f'invalid format: {name!r}')
    gd = m.groupdict()
    if gd.get('cln'):
        mod = importlib.import_module(gd['pkg'])
        parts = (gd.get('obj') or '').split('.') if gd.get('obj') else []
    else:
        parts = name.split('.')
        modname = parts.pop(0)
        mod = importlib.import_module(modname)
        while parts:
            p = parts[0]
            s = f'{modname}.{p}'
            try:
                mod = importlib.import_module(s)
                parts.pop(0)
                modname = s
            except ImportError:
                break
    result = mod
    for p in parts:
        result = getattr(result, p)
    return result
