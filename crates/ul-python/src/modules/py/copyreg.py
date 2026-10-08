"""Helper to provide extensibility for pickle.

This is only useful to add pickle support for extension types defined in
C, not for instances of user-defined classes.
"""

__all__ = ["pickle", "constructor",
           "add_extension", "remove_extension", "clear_extension_cache"]

dispatch_table = {}

def pickle(ob_type, pickle_function, constructor_ob=None):
    if not callable(pickle_function):
        raise TypeError("reduction functions must be callable")
    dispatch_table[ob_type] = pickle_function

    # The constructor_ob function is a vestige of safe for unpickling.
    # There is no reason for the caller to pass it anymore.
    if constructor_ob is not None:
        constructor(constructor_ob)

def constructor(object):
    if not callable(object):
        raise TypeError("constructors must be callable")

# Example: provide pickling support for complex numbers.

def pickle_complex(c):
    return complex, (c.real, c.imag)

pickle(complex, pickle_complex, complex)

def pickle_union(obj):
    import functools, operator
    return functools.reduce, (operator.or_, obj.__args__)

pickle(type(int | str), pickle_union)

# Support for pickling new-style objects

def _reconstructor(cls, base, state):
    if base is object:
        obj = object.__new__(cls)
    else:
        obj = base.__new__(cls, state)
        init = getattr(base, '__init__', None)
        if init is not None and init != getattr(object, '__init__', None):
            init(obj, state)
    return obj

_HEAPTYPE = 1<<9
_new_type = type(int.__new__)

# Python code for object.__reduce_ex__ for protocols 0 and 1

def _reduce_ex(self, proto):
    assert proto < 2
    cls = self.__class__
    for base in cls.__mro__:
        if hasattr(base, '__flags__') and not base.__flags__ & _HEAPTYPE:
            break
        new = base.__new__
        if isinstance(new, _new_type) and new.__self__ is base:
            break
    else:
        base = object # not really reachable
    if base is object:
        state = None
    else:
        if base is cls:
            raise TypeError(f"cannot pickle {cls.__name__!r} object")
        state = base(self)
    args = (cls, base, state)
    try:
        getstate = self.__getstate__
    except AttributeError:
        if getattr(self, "__slots__", None):
            raise TypeError(f"cannot pickle {cls.__name__!r} object: "
                            f"a class that defines __slots__ without "
                            f"defining __getstate__ cannot be pickled "
                            f"with protocol {proto}") from None
        try:
            dict = self.__dict__
        except AttributeError:
            dict = None
    else:
        if (type(self).__getstate__ is object.__getstate__ and
            getattr(self, "__slots__", None)):
            raise TypeError("a class that defines __slots__ without "
                            "defining __getstate__ cannot be pickled")
        dict = getstate()
    if dict:
        return _reconstructor, args, dict
    else:
        return _reconstructor, args

# Helper for __reduce_ex__ protocol 2

def __newobj__(cls, *args):
    return cls.__new__(cls, *args)

def __newobj_ex__(cls, args, kwargs):
    """Used by pickle protocol 4, instead of __newobj__ to allow classes with
    keyword-only arguments to be pickled correctly.
    """
    return cls.__new__(cls, *args, **kwargs)

def _slotnames(cls):
    """Return a list of slot names for a given class.

    This needs to find slots defined by the class and its bases, so we
    can't simply return the __slots__ attribute.  We must walk down
    the Method Resolution Order and concatenate the __slots__ of each
    class found there.  (This assumes classes don't modify their
    __slots__ attribute to misrepresent their slots after the class is
    defined.)
    """

    # Get the value from a cache in the class if possible
    names = cls.__dict__.get("__slotnames__")
    if names is not None:
        return names

    # Not cached -- calculate the value
    names = []
    if not hasattr(cls, "__slots__"):
        # This class has no slots
        pass
    else:
        # Slots found -- gather slot names from all base classes
        for c in cls.__mro__:
            if "__slots__" in c.__dict__:
                slots = c.__dict__['__slots__']
                # if class has a single slot, it can be given as a string
                if isinstance(slots, str):
                    slots = (slots,)
                for name in slots:
                    # special descriptors
                    if name in ("__dict__", "__weakref__"):
                        continue
                    # mangled names
                    elif name.startswith('__') and not name.endswith('__'):
                        stripped = c.__name__.lstrip('_')
                        if stripped:
                            names.append('_%s%s' % (stripped, name))
                        else:
                            names.append(name)
                    else:
                        names.append(name)

    # Cache the outcome in the class if at all possible
    try:
        cls.__slotnames__ = names
    except:
        pass # But don't die if we can't

    return names

# A registry of extension codes.  This is an ad-hoc compression
# mechanism.  Whenever a global reference to <module>, <name> is about
# to be pickled, the (<module>, <name>) tuple is looked up here to see
# if it is a registered extension code for it.  Extension codes are
# universal, so that the meaning of a pickle does not depend on
# context.  (There are also some codes reserved for local use that
# don't have this restriction.)  Codes are positive ints; 0 is
# reserved.

_extension_registry = {}                # key -> code
_inverted_registry = {}                 # code -> key
_extension_cache = {}                   # code -> object
# Don't ever rebind those names:  pickling grabs a reference to them when
# it's initialized, and won't see a rebinding.

def add_extension(module, name, code):
    """Register an extension code."""
    code = int(code)
    if not 1 <= code <= 0x7fffffff:
        raise ValueError("code out of range")
    key = (module, name)
    if (_extension_registry.get(key) == code and
        _inverted_registry.get(code) == key):
        return # Redundant registrations are benign
    if key in _extension_registry:
        raise ValueError("key %s is already registered with code %s" %
                         (key, _extension_registry[key]))
    if code in _inverted_registry:
        raise ValueError("code %s is already in use for key %s" %
                         (code, _inverted_registry[code]))
    _extension_registry[key] = code
    _inverted_registry[code] = key

def remove_extension(module, name, code):
    """Unregister an extension code.  For testing only."""
    key = (module, name)
    if (_extension_registry.get(key) != code or
        _inverted_registry.get(code) != key):
        raise ValueError("key %s is not registered with code %s" %
                         (key, code))
    del _extension_registry[key]
    del _inverted_registry[code]
    if code in _extension_cache:
        del _extension_cache[code]

def clear_extension_cache():
    _extension_cache.clear()

# Standard extension code assignments

# Reserved ranges

# First  Last Count  Purpose
#     1   127   127  Reserved for Python standard library
#   128   191    64  Reserved for Zope
#   192   239    48  Reserved for 3rd parties
#   240   255    16  Reserved for private use (will never be assigned)
#   256   Inf   Inf  Reserved for future assignment

# Extension codes are assigned by the Python Software Foundation.


def _object_getstate(self):
    """`object.__getstate__`: o `__dict__` (ou `None` se vazio) e os campos de `__slots__`."""
    d = getattr(self, '__dict__', None)
    state = dict(d) if d else None
    slots = {}
    for klass in type(self).__mro__:
        names = klass.__dict__.get('__slots__', ())
        if isinstance(names, str):
            names = (names,)
        for name in names:
            if name in ('__dict__', '__weakref__'):
                continue
            if name.startswith('__') and not name.endswith('__'):
                name = '_%s%s' % (klass.__name__.lstrip('_'), name)
            if hasattr(self, name):
                slots[name] = getattr(self, name)
    if slots:
        return (state, slots)
    return state


def _object_reduce(self):
    """`object.__reduce__`: reconstrução pelo protocolo 0/1."""
    return _object_reduce_ex(self, 1)


def _object_reduce_ex(self, proto=0):
    """`object.__reduce_ex__`: o `(callable, args, estado, ...)` que o `pickle` e o `copy` usam."""
    cls = type(self)
    # O `__reduce__` próprio de alguma classe do MRO; o de `object` é este mesmo protocolo.
    # Os `method_descriptor` dos tipos embutidos (o `BaseException.__reduce__`) são tratados mais abaixo.
    custom = next((k.__dict__['__reduce__'] for k in cls.__mro__
                   if k is not object and '__reduce__' in k.__dict__
                   and type(k.__dict__['__reduce__']).__name__ != 'method_descriptor'), None)
    if custom is not None and getattr(custom, '__module__', None) != 'copyreg':
        return custom(self)
    # `BaseException` tem `__reduce__` próprio (`BaseException_reduce`): nunca passa pelo protocolo de `object`.
    if isinstance(self, BaseException):
        return _exception_reduce(self)
    own_getstate = any(k is not object and '__getstate__' in k.__dict__ for k in cls.__mro__)
    if proto < 2 and getattr(cls, '__slots__', None) and not own_getstate:
        raise TypeError("a class that defines __slots__ without defining __getstate__ cannot be pickled")
    getstate = getattr(self, '__getstate__', None)
    state = getstate() if getstate is not None else _object_getstate(self)
    if proto >= 2:
        getnewargs_ex = getattr(self, '__getnewargs_ex__', None)
        if getnewargs_ex is not None:
            args, kwargs = getnewargs_ex()
            if kwargs:
                return __newobj_ex__, (cls, args, kwargs), state
        else:
            getnewargs = getattr(self, '__getnewargs__', None)
            if getnewargs is not None:
                args = getnewargs()
            else:
                args = ()
                for base in (tuple, str, bytes, int, float, frozenset):
                    if isinstance(self, base):
                        args = (base(self),)
                        break
        listitems = iter(self) if isinstance(self, list) else None
        dictitems = iter(self.items()) if isinstance(self, dict) else None
        return __newobj__, (cls,) + tuple(args), state, listitems, dictitems
    base = object
    for candidate in (tuple, list, dict, set, frozenset, str, bytes, int, float):
        if isinstance(self, candidate):
            base = candidate
            break
    # `copyreg._reduce_ex`: um tipo embutido exato não se reconstrói pelo protocolo 0 e 1.
    if base is cls and base is not object:
        raise TypeError("cannot pickle %r object" % cls.__name__)
    return _reconstructor, (cls, base, None if base is object else base(self)), state


# Os campos que o `OSError` mantém fora do `__dict__` (membros do tipo em C).
_OSERROR_MEMBERS = ('errno', 'strerror', 'filename', 'filename2')


def _exception_dict(self):
    """O `__dict__` de uma exceção: sem `args` nem os campos que o tipo em C guarda fora dele."""
    members = ('args',) + (_OSERROR_MEMBERS if isinstance(self, OSError) else ())
    d = getattr(self, '__dict__', None) or {}
    return {k: v for k, v in d.items() if k not in members}


def _exception_reduce(self):
    """`BaseException.__reduce__` e as variantes de `OSError`, `ImportError` e `AttributeError`
    (`BaseException_reduce`, `OSError_reduce`, `ImportError_reduce`, `AttributeError_reduce`)."""
    cls = type(self)
    args = tuple(self.args)
    state = _exception_dict(self)
    if isinstance(self, OSError):
        # `args` guarda só `errno` e `strerror` quando há nome de arquivo; ele volta como terceiro argumento
        # e, havendo `filename2`, o `winerror` (`None` fora do Windows) abre caminho para ele.
        filename = getattr(self, 'filename', None)
        if filename is not None:
            args = args[:2] + (filename,)
            filename2 = getattr(self, 'filename2', None)
            if filename2 is not None:
                args += (None, filename2)
    elif isinstance(self, ImportError):
        for key in ('name', 'path', 'name_from'):
            value = getattr(self, key, None)
            if value is not None:
                state[key] = value
    elif isinstance(self, AttributeError):
        return cls, args, _exception_getstate(self)
    if state:
        return cls, args, state
    return cls, args


def _exception_getstate(self):
    """`__getstate__` de uma exceção: o `AttributeError` leva `name` e `args` junto do `__dict__` (o `obj`
    não entra de propósito, GH-103352); as demais usam o de `object`."""
    if isinstance(self, AttributeError):
        state = _exception_dict(self)
        if getattr(self, 'name', None) is not None:
            state['name'] = self.name
        state['args'] = tuple(self.args)
        return state
    return _object_getstate(self)


def _exception_setstate(self, state):
    """`BaseException.__setstate__`: cada item do dicionário vira atributo."""
    if state is None:
        return None
    if not isinstance(state, dict):
        raise TypeError("state is not a dictionary")
    for key, value in state.items():
        if key == 'args':
            value = tuple(value)
            if value == tuple(self.args):
                continue
        setattr(self, key, value)
    return None


def _type_mro(cls):
    """`type.mro(cls)`."""
    return list(cls.__mro__)


def _builtin_reduce_ex(self, proto=0):
    """`__reduce_ex__` dos valores embutidos sem classe em Python (`range`, `Ellipsis`, `slice`,
    `bytearray`)."""
    if self is Ellipsis:
        return 'Ellipsis'
    if type(self).__name__ == 'builtin_function_or_method':
        return self.__qualname__
    if isinstance(self, range):
        return range, (self.start, self.stop, self.step)
    if isinstance(self, slice):
        return slice, (self.start, self.stop, self.step)
    if isinstance(self, bytearray):
        # `_common_reduce` do bytearrayobject.c: antes do protocolo 3 o conteúdo vai como texto latin-1.
        if proto < 3:
            return bytearray, (bytes(self).decode('latin-1'), 'latin-1'), None
        return bytearray, (bytes(self),), None
    raise TypeError("cannot pickle '%s' object" % type(self).__name__)


def _builtin_reduce(self):
    """`__reduce__` dos mesmos valores: o protocolo 2."""
    return _builtin_reduce_ex(self, 2)
