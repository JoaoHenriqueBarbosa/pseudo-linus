"""zipimport enxuto: o import real a partir de zip é feito pelo interpretador (entradas `.zip` em `sys.path`);
aqui ficam a classe e a exceção para quem as consulta."""

import os
import zipfile

__all__ = ['ZipImportError', 'zipimporter']


class ZipImportError(ImportError):
    pass


class zipimporter:
    def __init__(self, path):
        path = os.fspath(path)
        if not path:
            raise ZipImportError('archive path is empty', path=path)
        if not os.path.isfile(path) or not zipfile.is_zipfile(path):
            raise ZipImportError('not a Zip file', path=path)
        self.archive = path
        self.prefix = ''

    def _names(self):
        with zipfile.ZipFile(self.archive) as z:
            return z.namelist()

    def find_spec(self, fullname, target=None):
        leaf = fullname.rpartition('.')[2]
        names = self._names()
        if leaf + '/__init__.py' in names or leaf + '.py' in names:
            return object()
        return None

    def get_data(self, pathname):
        key = pathname
        if key.startswith(self.archive + os.sep):
            key = key[len(self.archive) + 1:]
        with zipfile.ZipFile(self.archive) as z:
            try:
                return z.read(key)
            except KeyError:
                raise OSError(0, '', pathname)

    def get_source(self, fullname):
        leaf = fullname.rpartition('.')[2]
        for key in (leaf + '/__init__.py', leaf + '.py'):
            if key in self._names():
                return self.get_data(key).decode()
        raise ZipImportError("can't find module %r" % fullname, name=fullname)

    def is_package(self, fullname):
        return fullname.rpartition('.')[2] + '/__init__.py' in self._names()

    def __repr__(self):
        return '<zipimporter object "%s">' % self.archive
