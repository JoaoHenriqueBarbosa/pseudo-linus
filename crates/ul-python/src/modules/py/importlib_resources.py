"""importlib.resources enxuto: recursos são arquivos ao lado do `__file__` do pacote."""
import os as _os
import sys as _sys
import io as _io
import contextlib as _contextlib
import pathlib as _pathlib

__all__ = ['files', 'as_file', 'read_text', 'read_binary', 'open_text', 'open_binary', 'is_resource',
           'contents', 'path']


def _dir(package):
    if isinstance(package, str):
        if package not in _sys.modules:
            __import__(package)
        package = _sys.modules[package]
    f = getattr(package, '__file__', None)
    if f is None:
        paths = list(getattr(package, '__path__', []))
        if not paths:
            raise TypeError("'%s' is not a package" % getattr(package, '__name__', package))
        return _pathlib.Path(paths[0])
    return _pathlib.Path(_os.path.dirname(f))


def files(package):
    return _dir(package)


@_contextlib.contextmanager
def as_file(traversable):
    yield _pathlib.Path(traversable)


def open_binary(package, resource):
    return open(_dir(package) / resource, 'rb')


def open_text(package, resource, encoding='utf-8', errors='strict'):
    return open(_dir(package) / resource, 'r', encoding=encoding, errors=errors)


def read_binary(package, resource):
    with open_binary(package, resource) as f:
        return f.read()


def read_text(package, resource, encoding='utf-8', errors='strict'):
    with open_text(package, resource, encoding, errors) as f:
        return f.read()


def is_resource(package, name):
    return (_dir(package) / name).is_file()


def contents(package):
    return [p.name for p in _dir(package).iterdir()]


@_contextlib.contextmanager
def path(package, resource):
    yield _dir(package) / resource
