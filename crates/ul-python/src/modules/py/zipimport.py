"""zipimport: importação de módulos `.py` de dentro de um arquivo zip (entradas `.zip` e `.whl` em
`sys.path`), com a mesma interface de importer e loader do CPython."""

import sys
import os

__all__ = ['ZipImportError', 'zipimporter']

path_sep = '/'


class ZipImportError(ImportError):
    pass


def _is_regular(path):
    try:
        st = os.stat(path)
    except (OSError, ValueError):
        return False
    return (st.st_mode & 0o170000) == 0o100000


class zipimporter:
    """zipimporter(archivepath) -> zipimporter object

    Create a new zipimporter instance. 'archivepath' must be a path to
    a zipfile, or to a specific path inside a zipfile. For example, it can be
    '/tmp/myimport.zip', or '/tmp/myimport.zip/mydirectory', if mydirectory is a
    valid directory inside the archive.

    'ZipImportError is raised if 'archivepath' doesn't point to a valid Zip
    archive.

    The 'archive' attribute of zipimporter objects contains the name of the
    zipfile targeted.
    """

    def __init__(self, path):
        path = os.fspath(path)
        if not path:
            raise ZipImportError('archive path is empty', path=path)
        prefix = []
        while True:
            if _is_regular(path):
                break
            try:
                os.stat(path)
            except (OSError, ValueError):
                dirname, basename = os.path.split(path)
                if dirname == path:
                    raise ZipImportError('not a Zip file', path=path)
                path = dirname
                prefix.append(basename)
                continue
            raise ZipImportError('not a Zip file', path=path)
        import zipfile
        try:
            with zipfile.ZipFile(path) as z:
                names = z.namelist()
        except (OSError, zipfile.BadZipFile):
            raise ZipImportError("not a Zip file: %r" % path, path=path)
        self._files = set(names)
        self.archive = path
        self.prefix = path_sep.join(prefix[::-1])
        if self.prefix:
            self.prefix += path_sep

    def _module_path(self, fullname):
        return self.prefix + fullname.rpartition('.')[2]

    def _module_info(self, fullname):
        path = self._module_path(fullname)
        if path + path_sep + '__init__.py' in self._files:
            return True
        if path + '.py' in self._files:
            return False
        return None

    def _is_dir(self, path):
        dirpath = path + path_sep
        return dirpath in self._files or any(n.startswith(dirpath) for n in self._files)

    def find_spec(self, fullname, target=None):
        """Create a ModuleSpec for the specified module.

        Returns None if the module cannot be found.
        """
        import importlib.util
        import importlib.machinery
        ispkg = self._module_info(fullname)
        if ispkg is not None:
            return importlib.util.spec_from_loader(fullname, self, is_package=ispkg)
        modpath = self._module_path(fullname)
        if self._is_dir(modpath):
            path = f'{self.archive}{path_sep}{modpath}'
            spec = importlib.machinery.ModuleSpec(name=fullname, loader=None, is_package=True)
            spec.submodule_search_locations.append(path)
            return spec
        return None

    def _source_path(self, fullname):
        ispkg = self._module_info(fullname)
        if ispkg is None:
            raise ZipImportError(f"can't find module {fullname!r}", name=fullname)
        path = self._module_path(fullname)
        return path + (path_sep + '__init__.py' if ispkg else '.py')

    def get_code(self, fullname):
        """get_code(fullname) -> code object.

        Return the code object for the specified module. Raise ZipImportError
        if the module couldn't be imported.
        """
        inner = self._source_path(fullname)
        source = self.get_data(inner).replace(b'\r\n', b'\n').replace(b'\r', b'\n')
        return compile(source, self.archive + path_sep + inner, 'exec', dont_inherit=True)

    def get_data(self, pathname):
        """get_data(pathname) -> string with file data.

        Return the data associated with 'pathname'. Raise OSError if
        the file wasn't found.
        """
        key = pathname
        if key.startswith(self.archive + path_sep):
            key = key[len(self.archive + path_sep):]
        if key not in self._files:
            raise OSError(0, '', key)
        import zipfile
        with zipfile.ZipFile(self.archive) as z:
            return z.read(key)

    def get_filename(self, fullname):
        """get_filename(fullname) -> filename string.

        Return the filename for the specified module or raise ZipImportError
        if it couldn't be imported.
        """
        return self.archive + path_sep + self._source_path(fullname)

    def get_source(self, fullname):
        """get_source(fullname) -> source string.

        Return the source code for the specified module. Raise ZipImportError
        if the module couldn't be found, return None if the archive does
        contain the module, but has no source for it.
        """
        return self.get_data(self._source_path(fullname)).decode()

    def is_package(self, fullname):
        """is_package(fullname) -> bool.

        Return True if the module specified by fullname is a package.
        Raise ZipImportError if the module couldn't be found.
        """
        ispkg = self._module_info(fullname)
        if ispkg is None:
            raise ZipImportError(f"can't find module {fullname!r}", name=fullname)
        return ispkg

    def create_module(self, spec):
        return None

    def exec_module(self, module):
        """Execute the module."""
        code = self.get_code(module.__spec__.name)
        exec(code, module.__dict__)

    def load_module(self, fullname):
        """load_module(fullname) -> module.

        Load the module specified by 'fullname'. 'fullname' must be the
        fully qualified (dotted) module name. It returns the imported
        module, or raises ZipImportError if it could not be imported.
        """
        import importlib.util
        spec = self.find_spec(fullname)
        if spec is None:
            raise ZipImportError(f"can't find module {fullname!r}", name=fullname)
        module = importlib.util.module_from_spec(spec)
        sys.modules[fullname] = module
        try:
            self.exec_module(module)
        except BaseException:
            del sys.modules[fullname]
            raise
        return sys.modules[fullname]

    def invalidate_caches(self):
        """Invalidates the cache of file data of the archive path."""
        import zipfile
        try:
            with zipfile.ZipFile(self.archive) as z:
                self._files = set(z.namelist())
        except (OSError, zipfile.BadZipFile):
            self._files = set()

    def __repr__(self):
        return f'<zipimporter object "{self.archive}{path_sep}{self.prefix}">'
