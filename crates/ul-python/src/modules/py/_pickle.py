"""Optimized C implementation for the Python pickle module."""

# O `_pickle` do CPython é C. Aqui as classes e funções dele nascem do `pickle.py` do Debian, carregado uma
# segunda vez como `_pickle_impl` (o texto é o mesmo arquivo, não uma cópia): `Pickler` e `Unpickler` são
# clones de `_Pickler` e `_Unpickler` sem a base escondida na herança, com o `__module__` de `_pickle`. O
# `pickle` de verdade faz `from _pickle import ...` no fim, como no CPython. As exceções e o `PickleBuffer`
# vêm antes de tudo: o `pickle.py` importa o `PickleBuffer` já na quarta linha útil.

from io import BytesIO as _BytesIO


class PickleError(Exception):
    """A common base class for the other pickling exceptions."""


class PicklingError(PickleError):
    """This exception is raised when an unpicklable object is passed to the dump() method."""


class UnpicklingError(PickleError):
    """This exception is raised when there is a problem unpickling an object, such as a security violation."""


class PickleBuffer:
    """Wrapper for potentially out-of-band buffers"""

    __module__ = 'pickle'

    def __init__(self, buffer):
        self._view = memoryview(buffer)

    def _live(self):
        if self._view is None:
            raise ValueError('operation forbidden on released PickleBuffer object')
        return self._view

    def raw(self):
        """Return a memoryview of the raw memory underlying this buffer.

Will raise BufferError is the buffer isn't contiguous."""
        view = self._live()
        if not view.contiguous:
            raise BufferError('cannot extract raw buffer from non-contiguous buffer')
        return view.cast('B')

    def release(self):
        """Release the underlying buffer exposed by the PickleBuffer object."""
        if self._view is not None:
            self._view.release()
            self._view = None

    def __buffer__(self, flags, /):
        return self._live()


import _pickle_impl as _impl

# O que o `pickle.py` carregado como `_pickle_impl` achou (`PickleError`...) é o que ele mesmo definiu, porque a
# importação de `Pickler` ainda não existia: os erros do `_Pickler` e do `_Unpickler` têm de ser os daqui.
for _name in ('PickleError', 'PicklingError', 'UnpicklingError'):
    setattr(_impl, _name, globals()[_name])
del _name

# Os opcodes que chamam código do usuário (o `__init__` de uma classe, um `__setstate__`) deixam a exceção dele
# passar crua, como o `_pickle` em C; nos outros, dado corrompido vira `UnpicklingError`.
_USER_CODE_OPS = frozenset(('load_reduce', 'load_newobj', 'load_newobj_ex', 'load_inst', 'load_obj', 'load_build'))


class _Dispatch(dict):
    """A tabela de opcodes do `Unpickler`: um opcode desconhecido é `UnpicklingError`, não `KeyError`."""

    def __missing__(self, code):
        raise UnpicklingError('invalid load key, %r.' % bytes([code]).decode('latin-1'))


def _guarded(code, step):
    def guarded(self):
        try:
            step(self)
        except KeyError:
            raise UnpicklingError('invalid load key, %r.' % bytes([code]).decode('latin-1')) from None
        except (ValueError, IndexError, TypeError) as exc:
            if isinstance(exc, ValueError) and 'unsupported pickle protocol' in str(exc):
                raise
            raise UnpicklingError(str(exc)) from None

    return guarded


def _clone(source, name, **overrides):
    """Uma classe nova com os atributos de `source` (sem o `__dict__` e o `__weakref__` dela), em `_pickle`."""
    namespace = {k: v for k, v in vars(source).items() if k not in ('__dict__', '__weakref__', '__doc__')}
    namespace.update(overrides)
    namespace['__module__'] = '_pickle'
    namespace['__qualname__'] = name
    return type(name, (), namespace)


def _pickler_init(self, file, protocol=None, fix_imports=True, buffer_callback=None):
    _impl._Pickler.__init__(self, file, protocol, fix_imports=fix_imports, buffer_callback=buffer_callback)


Pickler = _clone(_impl._Pickler, 'Pickler', __init__=_pickler_init,
                 __doc__='This takes a binary file for writing a pickle data stream.')
Unpickler = _clone(
    _impl._Unpickler, 'Unpickler',
    dispatch=_Dispatch({code: step if step.__name__ in _USER_CODE_OPS else _guarded(code, step)
                        for code, step in _impl._Unpickler.dispatch.items()}),
    __doc__='This takes a binary file for reading a pickle data stream.')
del _pickler_init


def dump(obj, file, protocol=None, *, fix_imports=True, buffer_callback=None):
    """Write a pickled representation of obj to the open file object file."""
    Pickler(file, protocol, fix_imports, buffer_callback).dump(obj)


def dumps(obj, protocol=None, *, fix_imports=True, buffer_callback=None):
    """Return the pickled representation of the object as a bytes object."""
    out = _BytesIO()
    Pickler(out, protocol, fix_imports, buffer_callback).dump(obj)
    return out.getvalue()


def _no_buffers(buffers):
    return None if buffers is None or (type(buffers) is tuple and not buffers) else buffers


def load(file, *, fix_imports=True, encoding='ASCII', errors='strict', buffers=()):
    """Read and return an object from the pickle data stored in a file."""
    return Unpickler(file, fix_imports=fix_imports, encoding=encoding, errors=errors, buffers=_no_buffers(buffers)).load()


def loads(data, /, *, fix_imports=True, encoding='ASCII', errors='strict', buffers=()):
    """Read and return an object from the given pickle data."""
    if isinstance(data, str):
        raise TypeError("a bytes-like object is required, not 'str'")
    return Unpickler(_BytesIO(data), fix_imports=fix_imports, encoding=encoding, errors=errors,
                     buffers=_no_buffers(buffers)).load()
