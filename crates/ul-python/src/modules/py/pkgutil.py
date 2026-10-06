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


from collections import namedtuple as _namedtuple

ModuleInfo = _namedtuple('ModuleInfo', 'module_finder name ispkg')
ModuleInfo.__doc__ = 'A namedtuple with minimal info about a module.'


def iter_modules(path=None, prefix=''):
    """Módulos e pacotes achados em `path` (ou em `sys.path`) e os embutidos na VM."""
    import os
    import sys
    seen = set()
    roots = list(path) if path is not None else list(sys.path)
    for root in roots:
        root = root or '.'
        try:
            names = sorted(os.listdir(root))
        except OSError:
            continue
        for name in names:
            full = os.path.join(root, name)
            modname = None
            ispkg = False
            if name.endswith('.py') and name != '__init__.py':
                modname = name[:-3]
            elif os.path.isdir(full) and os.path.isfile(os.path.join(full, '__init__.py')):
                modname, ispkg = name, True
            if modname and modname.isidentifier() and modname not in seen:
                seen.add(modname)
                yield ModuleInfo(root, prefix + modname, ispkg)
    if path is None:
        import sys as _s
        for name in sorted(_s.stdlib_module_names):
            if not name.startswith('_') and name not in seen:
                seen.add(name)
                yield ModuleInfo(None, prefix + name, False)


def walk_packages(path=None, prefix='', onerror=None):
    for info in iter_modules(path, prefix):
        yield info
        if info.ispkg:
            try:
                __import__(info.name)
            except ImportError:
                if onerror is not None:
                    onerror(info.name)
            except Exception:
                if onerror is not None:
                    onerror(info.name)
                else:
                    raise
            else:
                import sys
                path = getattr(sys.modules[info.name], '__path__', None) or []
                yield from walk_packages(path, info.name + '.', onerror)


def get_data(package, resource):
    import importlib
    import os
    mod = importlib.import_module(package)
    base = os.path.dirname(getattr(mod, '__file__', '') or '')
    with open(os.path.join(base, *resource.split('/')), 'rb') as f:
        return f.read()


def find_loader(fullname):
    return None


__all__ += ['iter_modules', 'walk_packages', 'ModuleInfo', 'get_data']
