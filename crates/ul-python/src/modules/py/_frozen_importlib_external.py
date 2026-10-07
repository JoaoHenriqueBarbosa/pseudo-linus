"""_frozen_importlib_external: a importação a partir de caminhos (`PathFinder` de `sys.meta_path`,
os carregadores de arquivo e o `FileFinder`)."""

import sys


SOURCE_SUFFIXES = ['.py']
BYTECODE_SUFFIXES = ['.pyc']
DEBUG_BYTECODE_SUFFIXES = OPTIMIZED_BYTECODE_SUFFIXES = BYTECODE_SUFFIXES
EXTENSION_SUFFIXES = ['.cpython-313-x86_64-linux-gnu.so', '.abi3.so', '.so']

MAGIC_NUMBER = (3571).to_bytes(2, 'little') + b'\r\n'


class WindowsRegistryFinder:

    """Meta path finder for modules declared in the Windows registry."""

    REGISTRY_KEY = (
        'Software\\Python\\PythonCore\\{sys_version}'
        '\\Modules\\{fullname}')
    REGISTRY_KEY_DEBUG = (
        'Software\\Python\\PythonCore\\{sys_version}'
        '\\Modules\\{fullname}\\Debug')
    DEBUG_BUILD = False

    @staticmethod
    def _open_registry(key):
        try:
            return winreg.OpenKey(winreg.HKEY_CURRENT_USER, key)
        except OSError:
            return winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, key)

    @classmethod
    def _search_registry(cls, fullname):
        if cls.DEBUG_BUILD:
            registry_key = cls.REGISTRY_KEY_DEBUG
        else:
            registry_key = cls.REGISTRY_KEY
        key = registry_key.format(fullname=fullname,
                                  sys_version='%d.%d' % sys.version_info[:2])
        try:
            with cls._open_registry(key) as hkey:
                filepath = winreg.QueryValue(hkey, '')
        except OSError:
            return None
        return filepath

    @classmethod
    def find_spec(cls, fullname, path=None, target=None):
        filepath = cls._search_registry(fullname)
        if filepath is None:
            return None
        return None


class _LoaderBasics:

    """Base class of common code needed by both SourceLoader and
    SourcelessFileLoader."""

    def is_package(self, fullname):
        """Concrete implementation of InspectLoader.is_package by checking if
        the path returned by get_filename has a filename of '__init__.py'."""
        filename = self.get_filename(fullname).rpartition('/')[2]
        filename_base = filename.rsplit('.', 1)[0]
        tail_name = fullname.rpartition('.')[2]
        return filename_base == '__init__' and tail_name != '__init__'

    def create_module(self, spec):
        """Use default semantics for module creation."""

    def exec_module(self, module):
        """Execute the module."""
        code = self.get_code(module.__name__)
        if code is None:
            raise ImportError(f'cannot load module {module.__name__!r} when '
                              'get_code() returns None')
        exec(code, module.__dict__)

    def load_module(self, fullname):
        """This method is deprecated."""
        import importlib.util
        spec = importlib.util.spec_from_file_location(fullname, self.get_filename(fullname), loader=self)
        module = importlib.util.module_from_spec(spec)
        sys.modules[module.__name__] = module
        self.exec_module(module)
        return sys.modules[module.__name__]


class SourceLoader(_LoaderBasics):

    def path_mtime(self, path):
        """Optional method that returns the modification time (an int) for the
        specified path (a str).

        Raises OSError when the path cannot be handled.
        """
        raise OSError

    def path_stats(self, path):
        """Optional method returning a metadata dict for the specified
        path (a str).

        Possible keys:
        - 'mtime' (mandatory) is the numeric timestamp of last source
          code modification;
        - 'size' (optional) is the size in bytes of the source code.

        Implementing this method allows the loader to read bytecode files.
        Raises OSError when the path cannot be handled.
        """
        return {'mtime': self.path_mtime(path)}

    def set_data(self, path, data):
        """Optional method which writes data (bytes) to a file path (a str).

        Implementing this method allows for the writing of bytecode files.
        """

    def get_source(self, fullname):
        """Concrete implementation of InspectLoader.get_source."""
        path = self.get_filename(fullname)
        try:
            source_bytes = self.get_data(path)
        except OSError as exc:
            raise ImportError('source not available through get_data()',
                              name=fullname) from exc
        return decode_source(source_bytes)

    def source_to_code(self, data, path, *, _optimize=-1):
        """Return the code object compiled from source.

        The 'data' argument can be any object type that compile() supports.
        """
        return compile(data, path, 'exec', dont_inherit=True, optimize=_optimize)

    def get_code(self, fullname):
        """Concrete implementation of InspectLoader.get_code.

        Reading of bytecode requires path_stats to be implemented. To write
        bytecode, set_data must also be implemented.

        """
        source_path = self.get_filename(fullname)
        return self.source_to_code(self.get_data(source_path), source_path)


class FileLoader:

    """Base file loader class which implements the loader protocol methods that
    require file system usage."""

    def __init__(self, fullname, path):
        """Cache the module name and the path to the file found by the
        finder."""
        self.name = fullname
        self.path = path

    def __eq__(self, other):
        return (self.__class__ == other.__class__ and
                self.__dict__ == other.__dict__)

    def __hash__(self):
        return hash(self.name) ^ hash(self.path)

    def load_module(self, fullname=None):
        """Load a module from a file.

        This method is deprecated.  Use exec_module() instead.

        """
        return super(FileLoader, self).load_module(fullname or self.name)

    def get_filename(self, fullname=None):
        """Return the path to the source file as found by the finder."""
        return self.path

    def get_data(self, path):
        """Return the data from path as raw bytes."""
        with open(path, 'rb') as file:
            return file.read()

    def get_resource_reader(self, module):
        from importlib.readers import FileReader
        return FileReader(self)


class SourceFileLoader(FileLoader, SourceLoader):

    """Concrete implementation of SourceLoader using the file system."""

    def path_stats(self, path):
        """Return the metadata for the path."""
        import os
        st = os.stat(path)
        return {'mtime': st.st_mtime, 'size': st.st_size}

    def set_data(self, path, data, *, _mode=0o666):
        """Write bytes data to a file."""


class SourcelessFileLoader(FileLoader, _LoaderBasics):

    """Loader which handles sourceless file imports."""

    def get_code(self, fullname):
        path = self.get_filename(fullname)
        data = self.get_data(path)
        if data[:4] != MAGIC_NUMBER:
            message = f'bad magic number in {fullname!r}: {data[:4]!r}'
            raise ImportError(message, name=fullname, path=path)
        import marshal
        return marshal.loads(memoryview(data)[16:])

    def get_source(self, fullname):
        """Return None as there is no source code."""
        return None


class ExtensionFileLoader(FileLoader, _LoaderBasics):

    """Loader for extension modules.

    The constructor is designed to work with FileFinder.

    """

    def __init__(self, name, path):
        self.name = name
        self.path = path

    def __eq__(self, other):
        return (self.__class__ == other.__class__ and
                self.__dict__ == other.__dict__)

    def __hash__(self):
        return hash(self.name) ^ hash(self.path)

    def create_module(self, spec):
        """Create an uninitialized extension module"""
        # Os módulos de extensão do sistema são os nativos do interpretador, achados pelo nome.
        import importlib
        return importlib.import_module(spec.name)

    def exec_module(self, module):
        """Initialize an extension module"""

    def is_package(self, fullname):
        """Return True if the extension module is a package."""
        file_name = self.path.rpartition('/')[2]
        return any(file_name == '__init__' + suffix
                   for suffix in EXTENSION_SUFFIXES)

    def get_code(self, fullname):
        """Return None as an extension module cannot create a code object."""
        return None

    def get_source(self, fullname):
        """Return None as extension modules have no source code."""
        return None

    def get_filename(self, fullname=None):
        """Return the path to the source file as found by the finder."""
        return self.path


class _NamespacePath:
    """Represents a namespace package's path.  It uses the module name
    to find its parent module, and from there it looks up the parent's
    __path__.  When this changes, the module's own path is recomputed,
    using path_finder.  For top-level modules, the parent module's path
    is sys.path."""

    # When invalidate_caches() is called, this epoch is incremented
    # https://bugs.python.org/issue45703
    _epoch = 0

    def __init__(self, name, path, path_finder):
        self._name = name
        self._path = path
        self._last_parent_path = tuple(self._get_parent_path())
        self._last_epoch = self._epoch
        self._path_finder = path_finder

    def _find_parent_path_names(self):
        """Returns a tuple of (parent-module-name, parent-path-attr-name)"""
        parent, dot, me = self._name.rpartition('.')
        if dot == '':
            # This is a top-level module. sys.path contains the parent path.
            return 'sys', 'path'
        # Not a top-level module. parent-module.__path__ contains the
        #  parent path.
        return parent, '__path__'

    def _get_parent_path(self):
        parent_module_name, path_attr_name = self._find_parent_path_names()
        return getattr(sys.modules[parent_module_name], path_attr_name)

    def _recalculate(self):
        # If the parent's path has changed, recalculate _path
        parent_path = tuple(self._get_parent_path()) # Make a copy
        if parent_path != self._last_parent_path or self._epoch != self._last_epoch:
            spec = self._path_finder(self._name, parent_path)
            # Note that no changes are made if a loader is returned, but we
            #  do remember the new parent path
            if spec is not None and spec.loader is None:
                if spec.submodule_search_locations:
                    self._path = spec.submodule_search_locations
            self._last_parent_path = parent_path     # Save the copy
            self._last_epoch = self._epoch
        return self._path

    def __iter__(self):
        return iter(self._recalculate())

    def __getitem__(self, index):
        return self._recalculate()[index]

    def __setitem__(self, index, path):
        self._path[index] = path

    def __len__(self):
        return len(self._recalculate())

    def __repr__(self):
        return f'_NamespacePath({self._path!r})'

    def __contains__(self, item):
        return item in self._recalculate()

    def append(self, item):
        self._path.append(item)


# This class is actually exposed publicly in a namespace package's __loader__
# attribute, so it should be available through a non-private name.
# https://github.com/python/cpython/issues/92054
class NamespaceLoader:
    def __init__(self, name, path, path_finder):
        self._path = _NamespacePath(name, path, path_finder)

    def is_package(self, fullname):
        return True

    def get_source(self, fullname):
        return ''

    def get_code(self, fullname):
        return compile('', '<string>', 'exec', dont_inherit=True)

    def create_module(self, spec):
        """Use default semantics for module creation."""

    def exec_module(self, module):
        pass

    def load_module(self, fullname):
        """Load a namespace module.

        This method is deprecated.  Use exec_module() instead.

        """
        import importlib.util
        spec = importlib.util.spec_from_loader(fullname, self, is_package=True)
        module = importlib.util.module_from_spec(spec)
        sys.modules[fullname] = module
        return module

    def get_resource_reader(self, module):
        from importlib.readers import NamespaceReader
        return NamespaceReader(self._path)


# We use this exclusively in module_from_spec() for backward-compatibility.
_NamespaceLoader = NamespaceLoader


class PathFinder:

    """Meta path finder for sys.path and package __path__ attributes."""

    @staticmethod
    def invalidate_caches():
        """Call the invalidate_caches() method on all path entry finders
        stored in sys.path_importer_caches (where implemented)."""
        for name, finder in list(sys.path_importer_cache.items()):
            # Drop entry if finder name is a relative path. The current
            # working directory may have changed.
            if finder is None or not name.startswith('/'):
                del sys.path_importer_cache[name]
            elif hasattr(finder, 'invalidate_caches'):
                finder.invalidate_caches()
        # Also invalidate the caches of _NamespacePaths
        # https://bugs.python.org/issue45703
        _NamespacePath._epoch += 1

    @classmethod
    def find_spec(cls, fullname, path=None, target=None):
        """Try to find a spec for 'fullname' on sys.path or 'path'.

        The search is based on sys.path_hooks and sys.path_importer_cache.
        """
        if path is None:
            import importlib.util
            try:
                return importlib.util.find_spec(fullname)
            except (ImportError, ValueError):
                return None
        from _frozen_importlib import ModuleSpec
        namespace_path = []
        for entry in path:
            if not isinstance(entry, str):
                continue
            finder = FileFinder(entry, (SourceFileLoader, SOURCE_SUFFIXES))
            spec = finder.find_spec(fullname, target)
            if spec is None:
                continue
            if spec.loader is not None:
                return spec
            namespace_path.extend(spec.submodule_search_locations or [])
        if namespace_path:
            spec = ModuleSpec(fullname, None)
            spec.submodule_search_locations = _NamespacePath(fullname, namespace_path, cls._get_spec)
            return spec
        return None

    @classmethod
    def _get_spec(cls, fullname, path, target=None):
        return cls.find_spec(fullname, path, target)

    @staticmethod
    def find_distributions(*args, **kwargs):
        """
        Find distributions.

        Return an iterable of all Distribution instances capable of
        loading the metadata for packages matching ``context.name``
        (or all names if ``None`` indicated) along the paths in the list
        of directories ``context.path``.
        """
        from importlib.metadata import MetadataPathFinder
        return MetadataPathFinder.find_distributions(*args, **kwargs)


class FileFinder:

    """File-based finder.

    Interactions with the file system are cached for performance, being
    refreshed when the directory the finder is handling has been modified.

    """

    def __init__(self, path, *loader_details):
        """Initialize with the path to search on and a variable number of
        2-tuples containing the loader and the file suffixes the loader
        recognizes."""
        import os
        loaders = []
        for loader, suffixes in loader_details:
            loaders.extend((suffix, loader) for suffix in suffixes)
        self._loaders = loaders
        # Base (directory) path
        if not path or path == '.':
            self.path = os.getcwd()
        else:
            self.path = os.path.abspath(path)
        self._path_mtime = -1
        self._path_cache = set()
        self._relaxed_path_cache = set()

    def invalidate_caches(self):
        """Invalidate the directory mtime."""
        self._path_mtime = -1

    def _get_spec(self, loader_class, fullname, path, smsl, target):
        from importlib.util import spec_from_file_location
        loader = loader_class(fullname, path)
        return spec_from_file_location(fullname, path, loader=loader,
                                       submodule_search_locations=smsl)

    def find_spec(self, fullname, target=None):
        """Try to find a spec for the specified module.

        Returns the matching spec, or None if not found.
        """
        import os
        from _frozen_importlib import ModuleSpec
        is_namespace = False
        tail_module = fullname.rpartition('.')[2]
        try:
            mtime = os.stat(self.path or os.getcwd()).st_mtime
        except OSError:
            mtime = -1
        if mtime != self._path_mtime:
            self._fill_cache()
            self._path_mtime = mtime
        cache = self._path_cache
        cache_module = tail_module
        # Check if the module is the name of a directory (and thus a package).
        base_path = os.path.join(self.path, tail_module)
        if cache_module in cache:
            for suffix, loader_class in self._loaders:
                init_filename = '__init__' + suffix
                full_path = os.path.join(base_path, init_filename)
                if os.path.isfile(full_path):
                    return self._get_spec(loader_class, fullname, full_path, [base_path], target)
            else:
                # If a namespace package, return the path if we don't
                #  find a module in the next section.
                is_namespace = os.path.isdir(base_path)
        # Check for a file w/ a proper suffix exists.
        for suffix, loader_class in self._loaders:
            full_path = os.path.join(self.path, tail_module + suffix)
            if cache_module + suffix in cache and os.path.isfile(full_path):
                return self._get_spec(loader_class, fullname, full_path, None, target)
        if is_namespace:
            spec = ModuleSpec(fullname, None)
            spec.submodule_search_locations = [base_path]
            return spec
        return None

    def _fill_cache(self):
        """Fill the cache of potential modules and packages for this directory."""
        import os
        try:
            contents = os.listdir(self.path or os.getcwd())
        except (FileNotFoundError, PermissionError, NotADirectoryError):
            # Directory has either been removed, turned into a file, or made
            # unreadable.
            contents = []
        self._path_cache = set(contents)

    @classmethod
    def path_hook(cls, *loader_details):
        """A class method which returns a closure to use on sys.path_hook
        which will return an instance using the specified loaders and the path
        called on the closure.

        If the path called on the closure is not a directory, ImportError is
        raised.

        """
        def path_hook_for_FileFinder(path):
            """Path hook for importlib.machinery.FileFinder."""
            import os
            if not os.path.isdir(path):
                raise ImportError('only directories are supported', path=path)
            return cls(path, *loader_details)

        return path_hook_for_FileFinder

    def __repr__(self):
        return f'FileFinder({self.path!r})'


class AppleFrameworkLoader(ExtensionFileLoader):
    """A loader for modules that have been packaged as frameworks for
    compatibility with Apple's iOS App Store policies.
    """


def decode_source(source_bytes):
    """Decode bytes representing source code and return the string.

    Universal newline support is used in the decoding.
    """
    import tokenize  # To avoid bootstrap issues.
    import io
    source_bytes_readline = io.BytesIO(source_bytes).readline
    encoding = tokenize.detect_encoding(source_bytes_readline)
    newline_decoder = io.IncrementalNewlineDecoder(None, True)
    return newline_decoder.decode(source_bytes.decode(encoding[0]))


def _spec_for_module(name, file, is_package, frozen=False):
    """O `__spec__` de um módulo carregado de arquivo (criado sob demanda pelo interpretador)."""
    from _frozen_importlib import BuiltinImporter, FrozenImporter, ModuleSpec
    if not file:
        return ModuleSpec(name, BuiltinImporter, origin='built-in')
    if frozen:
        import types
        spec = ModuleSpec(name, FrozenImporter, origin='frozen', is_package=is_package)
        spec.loader_state = types.SimpleNamespace(filename=file, origname=name)
        return spec
    # Extensão (nativa em Rust ou mypyc executado pelo `.py` irmão): o `__file__` é o `.so`, como no CPython.
    loader = ExtensionFileLoader(name, file) if file.endswith(tuple(EXTENSION_SUFFIXES)) else SourceFileLoader(name, file)
    spec = ModuleSpec(name, loader, origin=file, is_package=is_package)
    spec._set_fileattr = True
    if is_package:
        import os.path
        spec.submodule_search_locations = [os.path.dirname(file)]
    return spec
