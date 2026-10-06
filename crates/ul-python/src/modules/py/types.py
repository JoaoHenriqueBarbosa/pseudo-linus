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
BuiltinFunctionType = type(len)
BuiltinMethodType = type([].append)
ModuleType = type(sys)
NoneType = type(None)
EllipsisType = type(Ellipsis)
NotImplementedType = type(NotImplemented)
MappingProxyType = dict
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
