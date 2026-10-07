"""importlib.machinery: `ModuleSpec` e os carregadores de arquivo-fonte."""

import sys


class ModuleSpec:
    """Como e de onde um módulo é carregado."""

    def __init__(self, name, loader, *, origin=None, loader_state=None, is_package=None):
        self.name = name
        self.loader = loader
        self.origin = origin
        self.loader_state = loader_state
        self.submodule_search_locations = [] if is_package else None
        self._uninitialized_submodules = []
        self._set_fileattr = False
        self._cached = None

    def __repr__(self):
        args = ['name=%r' % self.name, 'loader=%r' % self.loader]
        if self.origin is not None:
            args.append('origin=%r' % self.origin)
        if self.submodule_search_locations is not None:
            args.append('submodule_search_locations=%r' % self.submodule_search_locations)
        return 'ModuleSpec(%s)' % ', '.join(args)

    def __eq__(self, other):
        try:
            return (self.name == other.name and self.loader == other.loader and self.origin == other.origin
                    and self.submodule_search_locations == other.submodule_search_locations
                    and self.cached == other.cached and self.has_location == other.has_location)
        except AttributeError:
            return NotImplemented

    @property
    def cached(self):
        return self._cached

    @cached.setter
    def cached(self, cached):
        self._cached = cached

    @property
    def parent(self):
        if self.submodule_search_locations is None:
            return self.name.rpartition('.')[0]
        return self.name

    @property
    def has_location(self):
        return self._set_fileattr

    @has_location.setter
    def has_location(self, value):
        self._set_fileattr = bool(value)


class BuiltinImporter:
    """Módulos que o interpretador traz embutidos."""

    @classmethod
    def find_spec(cls, fullname, path=None, target=None):
        import _sys
        if _sys._is_builtin_module(fullname):
            return ModuleSpec(fullname, cls, origin='built-in')
        return None

    @classmethod
    def create_module(cls, spec):
        return None

    @classmethod
    def exec_module(cls, module):
        pass


class SourceFileLoader:
    """Carrega um módulo a partir de um arquivo `.py`."""

    def __init__(self, fullname, path):
        self.name = fullname
        self.path = path

    def __eq__(self, other):
        return self.__class__ == other.__class__ and self.__dict__ == other.__dict__

    def __hash__(self):
        return hash(self.name) ^ hash(self.path)

    def get_filename(self, fullname=None):
        return self.path

    def get_data(self, path):
        with open(path, 'rb') as f:
            return f.read()

    def get_source(self, fullname=None):
        return self.get_data(self.path).decode('utf-8')

    def is_package(self, fullname=None):
        import os.path
        return os.path.basename(self.path).rsplit('.', 1)[0] == '__init__'

    def source_to_code(self, data, path, *, _optimize=-1):
        return compile(data, path, 'exec', dont_inherit=True, optimize=_optimize)

    def get_code(self, fullname=None):
        return self.source_to_code(self.get_data(self.path), self.path)

    def create_module(self, spec):
        return None

    def exec_module(self, module):
        exec(self.get_code(module.__name__), module.__dict__)

    def load_module(self, fullname=None):
        import importlib.util
        spec = importlib.util.spec_from_file_location(fullname or self.name, self.path, loader=self)
        module = importlib.util.module_from_spec(spec)
        sys.modules[module.__name__] = module
        self.exec_module(module)
        return module


SOURCE_SUFFIXES = ['.py']
BYTECODE_SUFFIXES = ['.pyc']
EXTENSION_SUFFIXES = []
all_suffixes = lambda: SOURCE_SUFFIXES + BYTECODE_SUFFIXES + EXTENSION_SUFFIXES


def _spec_for_module(name, file, is_package):
    """O `__spec__` de um módulo carregado de arquivo (criado sob demanda pelo interpretador)."""
    spec = ModuleSpec(name, SourceFileLoader(name, file), origin=file, is_package=is_package)
    spec._set_fileattr = True
    if is_package:
        import os.path
        spec.submodule_search_locations = [os.path.dirname(file)]
    return spec
