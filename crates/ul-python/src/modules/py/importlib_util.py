"""importlib.util: busca de módulos e construção de módulos a partir de um `ModuleSpec`."""

import sys
import types

from . import machinery
from .machinery import ModuleSpec, SourceFileLoader
from _frozen_importlib_external import MAGIC_NUMBER

_POPULATE = object()


def resolve_name(name, package):
    """Nome absoluto de um nome relativo (`.x`) dentro de `package`."""
    if not name.startswith('.'):
        return name
    if not package:
        raise ImportError('no package specified for %r (required for relative module names)' % name)
    level = 0
    for character in name:
        if character != '.':
            break
        level += 1
    parts = package.rsplit('.', level - 1)
    if len(parts) < level:
        raise ImportError('attempted relative import beyond top-level package')
    base = parts[0]
    rest = name[level:]
    return '%s.%s' % (base, rest) if rest else base


def spec_from_file_location(name, location=None, *, loader=None, submodule_search_locations=_POPULATE):
    """Um `ModuleSpec` para o arquivo `location`."""
    import os
    if location is None:
        location = '<unknown>'
        if hasattr(loader, 'get_filename'):
            try:
                location = loader.get_filename(name)
            except ImportError:
                pass
    else:
        location = os.fspath(location)
        if not os.path.isabs(location):
            location = os.path.abspath(location)
    if loader is None:
        if location.endswith(tuple(machinery.EXTENSION_SUFFIXES)):
            loader = machinery.ExtensionFileLoader(name, location)
        elif location.endswith('.py'):
            loader = SourceFileLoader(name, location)
        else:
            return None
    spec = ModuleSpec(name, loader, origin=location)
    spec._set_fileattr = True
    if submodule_search_locations is _POPULATE:
        if loader is not None and hasattr(loader, 'is_package') and loader.is_package(name):
            submodule_search_locations = [os.path.dirname(location)]
        else:
            submodule_search_locations = None
    if submodule_search_locations is not None:
        spec.submodule_search_locations = list(submodule_search_locations)
        if spec.submodule_search_locations == [] and location:
            spec.submodule_search_locations.append(location.rpartition('/')[0])
    return spec


def module_from_spec(spec):
    """Cria o módulo descrito por `spec` (sem executá-lo)."""
    module = None
    if hasattr(spec.loader, 'create_module'):
        module = spec.loader.create_module(spec)
    if module is None:
        module = types.ModuleType(spec.name)
    module.__spec__ = spec
    module.__loader__ = spec.loader
    module.__package__ = spec.parent
    if spec.submodule_search_locations is not None:
        module.__path__ = spec.submodule_search_locations
    if spec.has_location:
        module.__file__ = spec.origin
    return module


def _zip_spec(entry, fullname):
    """O spec de `fullname` numa entrada de `sys.path` que é um zip (ou um diretório dentro dele)."""
    import zipimport
    importer = sys.path_importer_cache.get(entry)
    if importer is None:
        try:
            importer = zipimport.zipimporter(entry)
        except zipimport.ZipImportError:
            return None
        sys.path_importer_cache[entry] = importer
    if not isinstance(importer, zipimport.zipimporter):
        return None
    return importer.find_spec(fullname)


def _extension_file(stem):
    """O `<stem><sufixo>.so` que o sandbox consegue carregar, na ordem de `EXTENSION_SUFFIXES`, ou `None`.

    O sandbox nunca executa código de máquina: uma extensão só é carregável se o `.py` irmão (o
    fonte que o mypyc compilou) existe e roda no lugar dela. Sem ele o `.so` conta como ausente."""
    import os
    if not os.path.isfile(stem + '.py'):
        return None
    for suffix in machinery.EXTENSION_SUFFIXES:
        if os.path.isfile(stem + suffix):
            return stem + suffix
    return None


def find_spec(name, package=None):
    """O `ModuleSpec` de `name` sem importá-lo, ou `None` se não existir."""
    import os
    fullname = resolve_name(name, package) if name.startswith('.') else name
    if fullname in sys.modules:
        module = sys.modules[fullname]
        if module is None:
            return None
        spec = getattr(module, '__spec__', None)
        if spec is None:
            raise ValueError('%s.__spec__ is not set' % fullname)
        return spec
    parent, _, leaf = fullname.rpartition('.')
    if parent:
        parent_module = __import__(parent, None, None, ['_'], 0)
        search = getattr(parent_module, '__path__', None)
        if search is None:
            raise ModuleNotFoundError('__path__ attribute not found on %r while trying to find %r' % (parent, fullname), name=fullname)
    else:
        search = sys.path
    for entry in search:
        base = entry or os.getcwd()
        if not os.path.isdir(base):
            spec = _zip_spec(base, fullname)
            if spec is not None:
                return spec
            continue
        init = _extension_file(os.path.join(base, leaf, '__init__')) or os.path.join(base, leaf, '__init__.py')
        if os.path.isfile(init):
            return spec_from_file_location(fullname, init, submodule_search_locations=[os.path.join(base, leaf)])
        path = _extension_file(os.path.join(base, leaf)) or os.path.join(base, leaf + '.py')
        if os.path.isfile(path):
            return spec_from_file_location(fullname, path)
    return machinery.BuiltinImporter.find_spec(fullname)


def spec_from_loader(name, loader, *, origin=None, is_package=None):
    """Return a module spec based on various loader methods."""
    if origin is None:
        origin = getattr(loader, '_ORIGIN', None)
    if not origin and hasattr(loader, 'get_filename'):
        if is_package is None:
            return spec_from_file_location(name, loader=loader)
        search = [] if is_package else None
        return spec_from_file_location(name, loader=loader, submodule_search_locations=search)
    if is_package is None:
        if hasattr(loader, 'is_package'):
            try:
                is_package = loader.is_package(name)
            except ImportError:
                is_package = None
        else:
            is_package = False
    return ModuleSpec(name, loader, origin=origin, is_package=is_package)


def source_hash(source_bytes):
    import hashlib
    return hashlib.sha256(source_bytes).digest()[:8]


def cache_from_source(path, debug_override=None, *, optimization=None):
    import os
    head, tail = os.path.split(path)
    base = tail.rsplit('.', 1)[0]
    return os.path.join(head, '__pycache__', base + '.cpython-313.pyc')


def source_from_cache(path):
    import os
    head, tail = os.path.split(path)
    if os.path.basename(head) != '__pycache__':
        raise ValueError('__pycache__ not bottom-level directory in %r' % path)
    base = tail.split('.')[0]
    return os.path.join(os.path.dirname(head), base + '.py')
