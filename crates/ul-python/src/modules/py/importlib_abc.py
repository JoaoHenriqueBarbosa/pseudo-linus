"""importlib.abc do sandbox (Python embutido): as classes abstratas de localizadores e carregadores."""

import abc


class Loader(metaclass=abc.ABCMeta):
    """Abstract base class for import loaders."""

    def create_module(self, spec):
        return None

    def load_module(self, fullname):
        raise ImportError

    def module_repr(self, module):
        raise NotImplementedError


class MetaPathFinder(metaclass=abc.ABCMeta):
    """Abstract base class for import finders on sys.meta_path."""

    def invalidate_caches(self):
        """An optional method for clearing the finder's cache, if any.
        This method is used by importlib.invalidate_caches().
        """


class PathEntryFinder(metaclass=abc.ABCMeta):
    """Abstract base class for path entry finders used by PathFinder."""

    def invalidate_caches(self):
        """An optional method for clearing the finder's cache, if any.
        This method is used by PathFinder.invalidate_caches().
        """


class ResourceLoader(Loader):
    """Abstract base class for loaders which can return data from their
    back-end storage.

    This ABC represents one of the optional protocols specified by PEP 302.

    """

    @abc.abstractmethod
    def get_data(self, path):
        """Abstract method which when implemented should return the bytes for
        the specified path.  The path must be a str."""
        raise OSError


class InspectLoader(Loader):
    """Abstract base class for loaders which support inspection about the
    modules they can load.

    This ABC represents one of the optional protocols specified by PEP 302.

    """

    def is_package(self, fullname):
        raise ImportError

    def get_code(self, fullname):
        source = self.get_source(fullname)
        if source is None:
            return None
        return self.source_to_code(source)

    @abc.abstractmethod
    def get_source(self, fullname):
        raise ImportError

    @staticmethod
    def source_to_code(data, path='<string>'):
        return compile(data, path, 'exec', dont_inherit=True)


class ExecutionLoader(InspectLoader):
    """Abstract base class for loaders that wish to support the execution of
    modules as scripts.

    This ABC represents one of the optional protocols specified in PEP 302.

    """

    @abc.abstractmethod
    def get_filename(self, fullname):
        raise ImportError

    def get_code(self, fullname):
        source = self.get_source(fullname)
        if source is None:
            return None
        try:
            path = self.get_filename(fullname)
        except ImportError:
            return self.source_to_code(source)
        else:
            return self.source_to_code(source, path)


class FileLoader(ResourceLoader, ExecutionLoader):
    """Abstract base class partially implementing the ResourceLoader and
    ExecutionLoader ABCs."""

    def __init__(self, fullname, path):
        self.name = fullname
        self.path = path

    def get_filename(self, name=None):
        return self.path

    def get_data(self, path):
        with open(path, 'rb') as file:
            return file.read()


class SourceLoader(ResourceLoader, ExecutionLoader):
    """Abstract base class for loading source code (and optionally any
    corresponding bytecode)."""

    def path_mtime(self, path):
        raise OSError

    def path_stats(self, path):
        return {'mtime': self.path_mtime(path)}

    def set_data(self, path, data):
        """Write the bytes to the path (if possible)."""
