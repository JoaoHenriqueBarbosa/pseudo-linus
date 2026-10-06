"""weakref: referências fracas reais (`_weakref.ref`) e as coleções fracas em Python.

Diferenças em relação ao CPython: a função de retorno de `ref` roda quando a referência morta é
observada pela primeira vez (a VM não tem gancho de coleta), então as coleções abaixo descartam as
entradas mortas na leitura, na iteração e em `len`. `proxy` e `finalize` automático no coletor não
existem."""

from _weakref import ref

ReferenceType = type(ref(type('_probe', (), {})))
KeyedRef = ref

__all__ = ['ref', 'WeakMethod', 'WeakSet', 'WeakKeyDictionary', 'WeakValueDictionary', 'ReferenceType', 'finalize']


class WeakMethod:
    """Referência fraca a um método ligado: pelo objeto e pela função, não pelo método transitório."""

    def __init__(self, meth, callback=None):
        try:
            obj = meth.__self__
            func = meth.__func__
        except AttributeError:
            raise TypeError('argument should be a bound method, not {}'.format(type(meth))) from None
        self._obj_ref = ref(obj, callback)
        self._func_ref = ref(func, callback)
        self._meth_type = type(meth)

    def __call__(self):
        obj = self._obj_ref()
        func = self._func_ref()
        if obj is None or func is None:
            return None
        return self._meth_type(obj, func)

    def __eq__(self, other):
        if isinstance(other, WeakMethod):
            a, b = self(), other()
            if a is None or b is None:
                return self is other
            return self._obj_ref == other._obj_ref and self._func_ref == other._func_ref
        return NotImplemented

    def __hash__(self):
        return hash((self._obj_ref, self._func_ref))


class WeakValueDictionary:
    def __init__(self, other=(), /, **kw):
        self.data = {}
        self.update(other, **kw)

    def _live(self):
        for key in list(self.data):
            if self.data[key]() is None:
                del self.data[key]

    def __getitem__(self, key):
        o = self.data[key]()
        if o is None:
            del self.data[key]
            raise KeyError(key)
        return o

    def __setitem__(self, key, value):
        self.data[key] = ref(value)

    def __delitem__(self, key):
        del self.data[key]

    def __contains__(self, key):
        r = self.data.get(key)
        return r is not None and r() is not None

    def __len__(self):
        self._live()
        return len(self.data)

    def __iter__(self):
        self._live()
        return iter(list(self.data))

    def __repr__(self):
        return '<%s at %#x>' % (self.__class__.__name__, id(self))

    def get(self, key, default=None):
        r = self.data.get(key)
        o = r() if r is not None else None
        return default if o is None else o

    def keys(self):
        self._live()
        return list(self.data)

    def values(self):
        return [o for o in (r() for r in list(self.data.values())) if o is not None]

    def items(self):
        out = []
        for k, r in list(self.data.items()):
            o = r()
            if o is not None:
                out.append((k, o))
        return out

    def valuerefs(self):
        self._live()
        return list(self.data.values())

    def pop(self, key, *args):
        r = self.data.pop(key, None)
        o = r() if r is not None else None
        if o is None:
            if args:
                return args[0]
            raise KeyError(key)
        return o

    def setdefault(self, key, default=None):
        o = self.get(key)
        if o is None:
            self[key] = default
            return default
        return o

    def update(self, other=(), /, **kw):
        if hasattr(other, 'items'):
            other = other.items()
        for k, v in other:
            self[k] = v
        for k, v in kw.items():
            self[k] = v

    def clear(self):
        self.data.clear()

    def copy(self):
        new = WeakValueDictionary()
        for k, v in self.items():
            new[k] = v
        return new

    __copy__ = copy


class WeakKeyDictionary:
    def __init__(self, dict=None):
        self.data = {}
        if dict is not None:
            self.update(dict)

    def _live(self):
        for r in list(self.data):
            if r() is None:
                del self.data[r]

    def __getitem__(self, key):
        return self.data[ref(key)]

    def __setitem__(self, key, value):
        self.data[ref(key)] = value

    def __delitem__(self, key):
        del self.data[ref(key)]

    def __contains__(self, key):
        try:
            wr = ref(key)
        except TypeError:
            return False
        return wr in self.data

    def __len__(self):
        self._live()
        return len(self.data)

    def __iter__(self):
        self._live()
        return iter([o for o in (r() for r in list(self.data)) if o is not None])

    def __repr__(self):
        return '<%s at %#x>' % (self.__class__.__name__, id(self))

    def get(self, key, default=None):
        return self.data.get(ref(key), default)

    def keys(self):
        return list(self)

    def values(self):
        self._live()
        return list(self.data.values())

    def items(self):
        out = []
        for r, v in list(self.data.items()):
            o = r()
            if o is not None:
                out.append((o, v))
        return out

    def keyrefs(self):
        self._live()
        return list(self.data)

    def pop(self, key, *args):
        return self.data.pop(ref(key), *args)

    def setdefault(self, key, default=None):
        return self.data.setdefault(ref(key), default)

    def update(self, other=(), /, **kwargs):
        if hasattr(other, 'items'):
            other = other.items()
        for k, v in other:
            self[k] = v
        for k, v in kwargs.items():
            self[k] = v

    def clear(self):
        self.data.clear()

    def copy(self):
        new = WeakKeyDictionary()
        for k, v in self.items():
            new[k] = v
        return new

    __copy__ = copy


class WeakSet:
    def __init__(self, data=None):
        self.data = set()
        if data is not None:
            for item in data:
                self.add(item)

    def _live(self):
        for r in list(self.data):
            if r() is None:
                self.data.discard(r)

    def __iter__(self):
        self._live()
        return iter([o for o in (r() for r in list(self.data)) if o is not None])

    def __len__(self):
        self._live()
        return len(self.data)

    def __contains__(self, item):
        try:
            wr = ref(item)
        except TypeError:
            return False
        return wr in self.data

    def __repr__(self):
        return '<%s at %#x>' % (self.__class__.__name__, id(self))

    def add(self, item):
        self.data.add(ref(item))

    def discard(self, item):
        self.data.discard(ref(item))

    def remove(self, item):
        self.data.remove(ref(item))

    def pop(self):
        while True:
            r = self.data.pop()
            o = r()
            if o is not None:
                return o

    def clear(self):
        self.data.clear()

    def copy(self):
        return self.__class__(self)

    def update(self, other):
        for item in other:
            self.add(item)

    def __ior__(self, other):
        self.update(other)
        return self

    def union(self, other):
        return self.__class__(list(self) + list(other))

    def __or__(self, other):
        return self.union(other)

    def intersection(self, other):
        other = set(other)
        return self.__class__(item for item in self if item in other)

    __and__ = intersection

    def difference(self, other):
        other = set(other)
        return self.__class__(item for item in self if item not in other)

    __sub__ = difference

    def issubset(self, other):
        return set(self) <= set(other)

    def issuperset(self, other):
        return set(self) >= set(other)

    def isdisjoint(self, other):
        return len(self.intersection(other)) == 0


class finalize:
    """Chama `func(*args, **kwargs)` uma vez: ao chamar o objeto ou quando `obj` já morreu e alguém
    consulta `alive` ou chama `detach`/`peek` (não há coleta que dispare no instante da morte)."""

    def __init__(self, obj, func, /, *args, **kwargs):
        self._ref = ref(obj)
        self._info = (func, args, kwargs)
        self.atexit = True

    @property
    def alive(self):
        if self._info is not None and self._ref() is None:
            self()
        return self._info is not None

    def __call__(self, _=None):
        info, self._info = self._info, None
        if info is not None:
            func, args, kwargs = info
            return func(*args, **kwargs)

    def detach(self):
        obj = self._ref()
        info, self._info = self._info, None
        if obj is not None and info is not None:
            return (obj, info[0], info[1], info[2])

    def peek(self):
        obj = self._ref()
        if obj is not None and self._info is not None:
            return (obj, self._info[0], self._info[1], self._info[2])
