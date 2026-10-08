"""collections.abc do sandbox (Python embutido)."""

from abc import ABCMeta, abstractmethod
from types import GenericAlias

_NONE_TYPE = type(None)


def _check_methods(C, *methods):
    mro = getattr(C, '__mro__', None)
    if mro is None:
        return NotImplemented
    for method in methods:
        for B in mro:
            if method in B.__dict__:
                if B.__dict__[method] is None:
                    return NotImplemented
                break
        else:
            return NotImplemented
    return True


def _is_iterator(obj):
    try:
        return iter(obj) is obj
    except TypeError:
        return False


def _is_hashable(obj):
    try:
        hash(obj)
    except TypeError:
        return False
    return True


def _is_generator(obj):
    return type(obj).__name__ == 'generator'


_INSTANCE_CHECKS = {
    'Iterator': _is_iterator,
    'Generator': _is_generator,
    'Callable': callable,
    'Hashable': _is_hashable,
}

_CALLABLE_TYPES = frozenset({
    'function', 'builtin_function_or_method', 'method', 'method-wrapper', 'wrapper_descriptor',
    'method_descriptor', 'classmethod_descriptor', 'type',
})
_ITERATOR_TYPES = frozenset({
    'generator', 'enumerate', 'zip', 'map', 'filter', 'reversed', 'callable_iterator',
})


def _native_abcs(subclass):
    """As ABCs que um tipo nativo do CPython (iterador, gerador, função) satisfaz pelos seus slots."""
    if getattr(subclass, '__module__', None) != 'builtins':
        return ()
    name = getattr(subclass, '__name__', '')
    if name in _CALLABLE_TYPES:
        return ('Callable',)
    if name == 'generator':
        return ('Generator', 'Iterator', 'Iterable')
    if name.endswith('iterator') or name in _ITERATOR_TYPES:
        return ('Iterator', 'Iterable')
    if name == 'coroutine':
        return ('Coroutine', 'Awaitable')
    if name == 'async_generator':
        return ('AsyncGenerator', 'AsyncIterator', 'AsyncIterable')
    return ()


class _Builtins(ABCMeta):
    """Metaclasse que conhece quais tipos embutidos satisfazem a interface."""

    def __instancecheck__(cls, instance):
        check = _INSTANCE_CHECKS.get(cls.__name__)
        if check is not None:
            return check(instance)
        return cls.__subclasscheck__(type(instance))

    def __subclasscheck__(cls, subclass):
        if subclass in cls._builtins_:
            return True
        if cls.__module__ == __name__ and cls.__name__ in _native_abcs(subclass):
            return True
        return ABCMeta.__subclasscheck__(cls, subclass)

    def __subclasshook__(cls, subclass):
        names = cls._methods_
        if not names:
            return NotImplemented
        return _check_methods(subclass, *names)


class Hashable(metaclass=_Builtins):
    _builtins_ = (int, float, str, bool, tuple, bytes, frozenset, range, _NONE_TYPE)
    _methods_ = ('__hash__',)

    @abstractmethod
    def __hash__(self):
        return 0


class Awaitable(metaclass=_Builtins):
    _builtins_ = ()
    _methods_ = ('__await__',)

    __class_getitem__ = classmethod(GenericAlias)

class Coroutine(Awaitable):
    _builtins_ = ()
    _methods_ = ('__await__', 'send', 'throw', 'close')


class AsyncIterable(metaclass=_Builtins):
    _builtins_ = ()
    _methods_ = ('__aiter__',)

    __class_getitem__ = classmethod(GenericAlias)

class AsyncIterator(AsyncIterable):
    _builtins_ = ()
    _methods_ = ('__aiter__', '__anext__')


class AsyncGenerator(AsyncIterator):
    _builtins_ = ()
    _methods_ = ('__aiter__', '__anext__', 'asend', 'athrow', 'aclose')


class Iterable(metaclass=_Builtins):
    _builtins_ = (list, tuple, str, dict, set, frozenset, bytes, bytearray, range)
    _methods_ = ('__iter__',)

    @abstractmethod
    def __iter__(self):
        while False:
            yield None

    __class_getitem__ = classmethod(GenericAlias)


class Iterator(Iterable):
    _builtins_ = ()
    _methods_ = ('__iter__', '__next__')

    @abstractmethod
    def __next__(self):
        raise StopIteration

    def __iter__(self):
        return self


class Reversible(Iterable):
    _builtins_ = (list, tuple, str, dict, range, bytes, bytearray)
    _methods_ = ('__reversed__', '__iter__')

    @abstractmethod
    def __reversed__(self):
        while False:
            yield None


class Generator(Iterator):
    _builtins_ = ()
    _methods_ = ('__iter__', '__next__', 'send', 'throw', 'close')


class Sized(metaclass=_Builtins):
    _builtins_ = (list, tuple, str, dict, set, frozenset, bytes, bytearray, range)
    _methods_ = ('__len__',)

    @abstractmethod
    def __len__(self):
        return 0


class Container(metaclass=_Builtins):
    _builtins_ = (list, tuple, str, dict, set, frozenset, bytes, bytearray, range)
    _methods_ = ('__contains__',)

    @abstractmethod
    def __contains__(self, x):
        return False

    __class_getitem__ = classmethod(GenericAlias)


class Collection(Sized, Iterable, Container):
    _builtins_ = (list, tuple, str, dict, set, frozenset, bytes, bytearray, range)
    _methods_ = ('__len__', '__iter__', '__contains__')


class Callable(metaclass=_Builtins):
    _builtins_ = ()
    _methods_ = ('__call__',)

    @abstractmethod
    def __call__(self, *args, **kwds):
        return False

    def __class_getitem__(cls, params):
        import typing
        return typing.Callable[params[0], params[1]] if isinstance(params, tuple) and len(params) == 2 else typing.Callable


class Sequence(Reversible, Collection):
    _builtins_ = (list, tuple, str, bytes, bytearray, range)
    _methods_ = ()

    @abstractmethod
    def __getitem__(self, index):
        raise IndexError

    def __iter__(self):
        i = 0
        try:
            while True:
                v = self[i]
                yield v
                i += 1
        except IndexError:
            return

    def __contains__(self, value):
        for v in self:
            if v is value or v == value:
                return True
        return False

    def __reversed__(self):
        for i in reversed(range(len(self))):
            yield self[i]

    def index(self, value, start=0, stop=None):
        if start is not None and start < 0:
            start = max(len(self) + start, 0)
        if stop is not None and stop < 0:
            stop += len(self)
        i = start
        while stop is None or i < stop:
            try:
                v = self[i]
            except IndexError:
                break
            if v is value or v == value:
                return i
            i += 1
        raise ValueError('%r is not in sequence' % (value,))

    def count(self, value):
        return sum(1 for v in self if v is value or v == value)


class MutableSequence(Sequence):
    _builtins_ = (list, bytearray)
    _methods_ = ()

    @abstractmethod
    def __setitem__(self, index, value):
        raise IndexError

    @abstractmethod
    def __delitem__(self, index):
        raise IndexError

    @abstractmethod
    def insert(self, index, value):
        raise IndexError

    def append(self, value):
        self.insert(len(self), value)

    def clear(self):
        try:
            while True:
                self.pop()
        except IndexError:
            pass

    def reverse(self):
        n = len(self)
        for i in range(n // 2):
            self[i], self[n - i - 1] = self[n - i - 1], self[i]

    def extend(self, values):
        if values is self:
            values = list(values)
        for v in values:
            self.append(v)

    def pop(self, index=-1):
        v = self[index]
        del self[index]
        return v

    def remove(self, value):
        del self[self.index(value)]

    def __iadd__(self, values):
        self.extend(values)
        return self


class Set(Collection):
    _builtins_ = (set, frozenset)
    _methods_ = ()

    def __le__(self, other):
        if not isinstance(other, Set):
            return NotImplemented
        if len(self) > len(other):
            return False
        for elem in self:
            if elem not in other:
                return False
        return True

    def __lt__(self, other):
        if not isinstance(other, Set):
            return NotImplemented
        return len(self) < len(other) and self.__le__(other)

    def __gt__(self, other):
        if not isinstance(other, Set):
            return NotImplemented
        return len(self) > len(other) and self.__ge__(other)

    def __ge__(self, other):
        if not isinstance(other, Set):
            return NotImplemented
        if len(self) < len(other):
            return False
        for elem in other:
            if elem not in self:
                return False
        return True

    def __eq__(self, other):
        if not isinstance(other, Set):
            return NotImplemented
        return len(self) == len(other) and self.__le__(other)

    def isdisjoint(self, other):
        for value in other:
            if value in self:
                return False
        return True


class MutableSet(Set):
    _builtins_ = (set,)
    _methods_ = ()

    @abstractmethod
    def add(self, value):
        raise NotImplementedError

    @abstractmethod
    def discard(self, value):
        raise NotImplementedError

    def remove(self, value):
        if value not in self:
            raise KeyError(value)
        self.discard(value)

    def pop(self):
        it = iter(self)
        try:
            value = next(it)
        except StopIteration:
            raise KeyError from None
        self.discard(value)
        return value

    def clear(self):
        try:
            while True:
                self.pop()
        except KeyError:
            pass


class Mapping(Collection):
    _builtins_ = (dict,)
    _methods_ = ()

    @abstractmethod
    def __getitem__(self, key):
        raise KeyError

    def get(self, key, default=None):
        try:
            return self[key]
        except KeyError:
            return default

    def __contains__(self, key):
        try:
            self[key]
        except KeyError:
            return False
        return True

    def keys(self):
        return list(self)

    def items(self):
        return [(key, self[key]) for key in self]

    def values(self):
        return [self[key] for key in self]

    def __eq__(self, other):
        if not isinstance(other, Mapping):
            return NotImplemented
        return dict(self.items()) == dict(other.items())

    __hash__ = None


class MutableMapping(Mapping):
    _builtins_ = (dict,)
    _methods_ = ()

    @abstractmethod
    def __setitem__(self, key, value):
        raise KeyError

    @abstractmethod
    def __delitem__(self, key):
        raise KeyError

    def pop(self, key, *default):
        try:
            value = self[key]
        except KeyError:
            if default:
                return default[0]
            raise
        del self[key]
        return value

    def popitem(self):
        try:
            key = next(iter(self))
        except StopIteration:
            raise KeyError from None
        value = self[key]
        del self[key]
        return key, value

    def clear(self):
        try:
            while True:
                self.popitem()
        except KeyError:
            pass

    def update(self, other=(), /, **kwds):
        if isinstance(other, Mapping):
            for key in other:
                self[key] = other[key]
        elif hasattr(other, 'keys'):
            for key in other.keys():
                self[key] = other[key]
        else:
            for key, value in other:
                self[key] = value
        for key, value in kwds.items():
            self[key] = value

    def setdefault(self, key, default=None):
        try:
            return self[key]
        except KeyError:
            self[key] = default
        return default


class ByteString(Sequence):
    _builtins_ = (bytes, bytearray)
    _methods_ = ()


class Buffer(metaclass=_Builtins):
    """Objetos com o protocolo de buffer (PEP 688)."""

    _builtins_ = (bytes, bytearray, memoryview)
    _methods_ = ('__buffer__',)

    @abstractmethod
    def __buffer__(self, flags, /):
        raise NotImplementedError


class MappingView(Sized):
    _builtins_ = ()
    _methods_ = ()

    __class_getitem__ = classmethod(GenericAlias)

class KeysView(MappingView, Set):
    _builtins_ = ()
    _methods_ = ()


class ItemsView(MappingView, Set):
    _builtins_ = ()
    _methods_ = ()


class ValuesView(MappingView, Collection):
    _builtins_ = ()
    _methods_ = ()
