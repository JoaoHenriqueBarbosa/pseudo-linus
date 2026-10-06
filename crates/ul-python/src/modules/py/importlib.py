"""importlib: `import_module`, `reload` e `invalidate_caches` sobre o import do interpretador."""

import sys
import _sys
import types

from . import machinery as _machinery
from . import util as _util

__all__ = ['__import__', 'import_module', 'invalidate_caches', 'reload']

__import__ = __import__


def import_module(name, package=None):
    """Importa o módulo `name`; nomes relativos (`.x`) pedem `package`."""
    level = 0
    if name.startswith('.'):
        if not package:
            raise TypeError("the 'package' argument is required to perform a relative import for %r" % name)
        for character in name:
            if character != '.':
                break
            level += 1
        parts = package.split('.')
        if level - 1 >= len(parts):
            raise ImportError('attempted relative import beyond top-level package')
        base = '.'.join(parts[:len(parts) - (level - 1)])
        rest = name[level:]
        name = base + '.' + rest if rest else base
    return __import__(name, None, None, ['_'], 0)


def invalidate_caches():
    """Não há cache de buscadores para invalidar."""


def reload(module):
    """Roda de novo o código do módulo, nas mesmas globais."""
    if not isinstance(module, types.ModuleType):
        raise TypeError('reload() argument must be a module, not %s' % type(module).__name__)
    name = module.__name__
    if sys.modules.get(name) is not module:
        raise ImportError('module %s not in sys.modules' % name)
    return _sys._reload(module)
