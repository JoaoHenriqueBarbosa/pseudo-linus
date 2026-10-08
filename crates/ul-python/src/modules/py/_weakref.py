"""Weak-reference related module."""

# O `_weakref` do CPython é C. Aqui o núcleo é nativo (`_wref.ref`, que não herda de nada) e este módulo monta
# em cima dele os tipos do CPython: `ReferenceType` (que `weakref.WeakMethod` e `weakref.KeyedRef` herdam) e os
# dois procuradores. Os tipos dizem `__module__ == 'weakref'`, como lá.

from _wref import ref as _native_ref

_GenericAlias = type(list[int])
_FunctionType = type(lambda: None)
_BuiltinType = type(len)


class ReferenceType:
    """Referência fraca: chamá-la devolve o objeto, ou `None` depois que ele morreu."""

    __module__ = 'weakref'

    def __new__(cls, ob, callback=None, /):
        self = object.__new__(cls)
        self._wr_cb = callback
        self._wr_hash = None
        if callback is None:
            self._wr_ref = _native_ref(ob)
        else:
            # A função nativa guarda `_dead` e não a referência (que a guardaria de volta): o elo até aqui é fraco.
            me = _native_ref(self)

            def _dead(_dead_ref, me=me):
                alive = me()
                if alive is not None:
                    cb, alive._wr_cb = alive._wr_cb, None
                    if cb is not None:
                        cb(alive)

            self._wr_ref = _native_ref(ob, _dead)
        return self

    def __init__(self, ob, callback=None, /):
        pass

    def __call__(self):
        return self._wr_ref()

    @property
    def __callback__(self):
        return self._wr_cb

    def __hash__(self):
        if self._wr_hash is None:
            ob = self._wr_ref()
            if ob is None:
                raise TypeError('weak object has gone away')
            self._wr_hash = hash(ob)
        return self._wr_hash

    def __eq__(self, other):
        if not isinstance(other, ReferenceType):
            return NotImplemented
        a = self._wr_ref()
        b = other._wr_ref()
        if a is None or b is None:
            return self is other
        return a == b

    def __ne__(self, other):
        if not isinstance(other, ReferenceType):
            return NotImplemented
        a = self._wr_ref()
        b = other._wr_ref()
        if a is None or b is None:
            return self is not other
        return a != b

    def __repr__(self):
        ob = self._wr_ref()
        if ob is None:
            return '<weakref at %#x; dead>' % id(self)
        # `weakref_repr` acrescenta o `__name__` quando o tipo do referente o define (tipos e funções).
        if isinstance(ob, type) or type(ob) in (_FunctionType, _BuiltinType):
            return "<weakref at %#x; to '%s' at %#x (%s)>" % (id(self), type(ob).__name__, id(ob), ob.__name__)
        return "<weakref at %#x; to '%s' at %#x>" % (id(self), type(ob).__name__, id(ob))

    __class_getitem__ = classmethod(_GenericAlias)


ref = ReferenceType


def _proxy_dead():
    raise ReferenceError('weakly-referenced object no longer exists')


def _proxy_methods(callable_proxy):
    """Os métodos que os dois procuradores repassam ao referente (cada tipo ganha a sua cópia)."""

    def target(self):
        ob = object.__getattribute__(self, '_wr_ref')()
        if ob is None:
            _proxy_dead()
        return ob

    def init(self, ob, callback=None):
        if callback is None:
            object.__setattr__(self, '_wr_ref', _native_ref(ob))
        else:
            me = _native_ref(self)

            def _dead(_dead_ref, me=me):
                alive = me()
                if alive is not None:
                    callback(alive)

            object.__setattr__(self, '_wr_ref', _native_ref(ob, _dead))

    methods = {
        '__init__': init,
        '__getattr__': lambda self, name: getattr(target(self), name),
        '__setattr__': lambda self, name, value: setattr(target(self), name, value),
        '__delattr__': lambda self, name: delattr(target(self), name),
        '__repr__': lambda self: '<weakproxy at %#x to %s at %#x>' % (
            id(self), type(target(self)).__name__, id(target(self))),
        '__str__': lambda self: str(target(self)),
        '__bool__': lambda self: bool(target(self)),
        '__len__': lambda self: len(target(self)),
        '__iter__': lambda self: iter(target(self)),
        '__next__': lambda self: next(target(self)),
        '__contains__': lambda self, item: item in target(self),
        '__getitem__': lambda self, key: target(self)[key],
        '__setitem__': lambda self, key, value: target(self).__setitem__(key, value),
        '__delitem__': lambda self, key: target(self).__delitem__(key),
        '__eq__': lambda self, other: target(self) == other,
        '__ne__': lambda self, other: target(self) != other,
        '__hash__': None,
        '__module__': 'weakref',
    }
    if callable_proxy:
        methods['__call__'] = lambda self, /, *args, **kwargs: target(self)(*args, **kwargs)
    return methods


ProxyType = type('ProxyType', (), _proxy_methods(False))
CallableProxyType = type('CallableProxyType', (), _proxy_methods(True))


def proxy(object, callback=None, /):
    """Create a proxy object that weakly references 'object'.

'callback', if given, is called with a reference to the
proxy when 'object' is about to be finalized."""
    if callable(object):
        return CallableProxyType(object, callback)
    return ProxyType(object, callback)


def getweakrefcount(object, /):
    """Return the number of weak references to 'object'."""
    return 0


def getweakrefs(object, /):
    """Return a list of all weak reference objects pointing to 'object'."""
    return []


def _remove_dead_weakref(dict, key, /):
    """Atomically remove key from dict if it points to a dead weakref."""
    try:
        wr = dict[key]
    except KeyError:
        return
    if wr() is None:
        del dict[key]


del _proxy_methods
