"""types: nomes dos tipos de função, módulo, gerador etc., obtidos do próprio interpretador."""

import sys


def _f():
    pass


def _g():
    yield 1


class _C:
    def _m(self):
        pass


FunctionType = type(_f)
LambdaType = type(lambda: None)
CodeType = type(_f.__code__) if hasattr(_f, '__code__') else object
MethodType = type(_C()._m)
GeneratorType = type(_g())


async def _co():
    pass


async def _ag():
    yield 1


_coro = _co()
CoroutineType = type(_coro)
_coro.close()
_agen = _ag()
AsyncGeneratorType = type(_agen)
BuiltinFunctionType = type(len)
BuiltinMethodType = type([].append)
ModuleType = type(sys)
NoneType = type(None)
EllipsisType = type(Ellipsis)
NotImplementedType = type(NotImplemented)
class MappingProxyType:
    """Visão somente leitura de um mapeamento."""

    __slots__ = ('_mapping',)

    def __init__(self, mapping):
        if not hasattr(mapping, 'keys') or not hasattr(mapping, '__getitem__'):
            raise TypeError('mappingproxy() argument must be a mapping, not %s' % type(mapping).__name__)
        self._mapping = mapping

    def __getitem__(self, key):
        return self._mapping[key]

    def __iter__(self):
        return iter(self._mapping)

    def __len__(self):
        return len(self._mapping)

    def __contains__(self, key):
        return key in self._mapping

    def __eq__(self, other):
        if isinstance(other, MappingProxyType):
            other = other._mapping
        return self._mapping == other

    def __ne__(self, other):
        return not self == other

    __hash__ = None

    def __or__(self, other):
        if isinstance(other, MappingProxyType):
            other = other._mapping
        return self._mapping | other

    def __ror__(self, other):
        return other | self._mapping

    def __reversed__(self):
        return reversed(list(self._mapping))

    def get(self, key, default=None):
        return self._mapping.get(key, default)

    def keys(self):
        return self._mapping.keys()

    def values(self):
        return self._mapping.values()

    def items(self):
        return self._mapping.items()

    def copy(self):
        return self._mapping.copy()

    def __repr__(self):
        return 'mappingproxy(%r)' % (self._mapping,)

    def __str__(self):
        return str(self._mapping)


CellType = object
TracebackType = type(None)
FrameType = type(None)
GenericAlias = type(list[int])

try:
    raise ValueError
except ValueError as _exc:
    TracebackType = type(_exc.__traceback__)
    FrameType = type(_exc.__traceback__.tb_frame)

del _f, _g, _C


class SimpleNamespace:
    def __init__(self, mapping_or_iterable=(), /, **kwargs):
        for k, v in dict(mapping_or_iterable).items():
            setattr(self, k, v)
        for k, v in kwargs.items():
            setattr(self, k, v)

    def __repr__(self):
        items = ('{}={!r}'.format(k, v) for k, v in self.__dict__.items())
        return '{}({})'.format('namespace', ', '.join(items))

    def __eq__(self, other):
        if isinstance(self, SimpleNamespace) and isinstance(other, SimpleNamespace):
            return self.__dict__ == other.__dict__
        return NotImplemented


def new_class(name, bases=(), kwds=None, exec_body=None):
    ns = {}
    if exec_body is not None:
        exec_body(ns)
    return type(name, tuple(bases), ns)


def coroutine(func):
    return func
