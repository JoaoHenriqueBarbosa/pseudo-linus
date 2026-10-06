"""functools do sandbox (Python embutido)."""

WRAPPER_ASSIGNMENTS = ('__module__', '__name__', '__qualname__', '__doc__')
WRAPPER_UPDATES = ('__dict__',)


def update_wrapper(wrapper, wrapped, assigned=WRAPPER_ASSIGNMENTS, updated=WRAPPER_UPDATES):
    for attr in assigned:
        try:
            value = getattr(wrapped, attr)
        except AttributeError:
            pass
        else:
            setattr(wrapper, attr, value)
    for attr in updated:
        try:
            extra = getattr(wrapped, attr)
        except AttributeError:
            continue
        for k, v in extra.items():
            setattr(wrapper, k, v)
    wrapper.__wrapped__ = wrapped
    return wrapper


def wraps(wrapped, assigned=WRAPPER_ASSIGNMENTS, updated=WRAPPER_UPDATES):
    def decorator(wrapper):
        return update_wrapper(wrapper, wrapped, assigned, updated)
    return decorator


_initial_missing = object()


def reduce(function, iterable, initial=_initial_missing):
    it = iter(iterable)
    if initial is _initial_missing:
        try:
            value = next(it)
        except StopIteration:
            raise TypeError('reduce() of empty iterable with no initial value') from None
    else:
        value = initial
    for element in it:
        value = function(value, element)
    return value


class partial:
    """`partial(func, *args, **keywords)`: congela argumentos de uma função."""

    def __init__(self, func, *args, **keywords):
        if not callable(func):
            raise TypeError('the first argument must be callable')
        if isinstance(func, partial):
            args = func.args + args
            keywords = {**func.keywords, **keywords}
            func = func.func
        self.func = func
        self.args = args
        self.keywords = keywords

    def __call__(self, *args, **keywords):
        keywords = {**self.keywords, **keywords}
        return self.func(*self.args, *args, **keywords)

    def __repr__(self):
        parts = [repr(self.func)]
        parts.extend(repr(x) for x in self.args)
        parts.extend('%s=%r' % (k, v) for k, v in self.keywords.items())
        return 'functools.partial(' + ', '.join(parts) + ')'


class partialmethod:
    def __init__(self, func, *args, **keywords):
        self.func = func
        self.args = args
        self.keywords = keywords

    def __get__(self, obj, cls=None):
        if obj is None:
            return self
        return partial(self.func, obj, *self.args, **self.keywords)


def cmp_to_key(mycmp):
    class K:
        def __init__(self, obj):
            self.obj = obj

        def __lt__(self, other):
            return mycmp(self.obj, other.obj) < 0

        def __gt__(self, other):
            return mycmp(self.obj, other.obj) > 0

        def __eq__(self, other):
            return mycmp(self.obj, other.obj) == 0

        def __le__(self, other):
            return mycmp(self.obj, other.obj) <= 0

        def __ge__(self, other):
            return mycmp(self.obj, other.obj) >= 0

        __hash__ = None
    return K


class _CacheInfo:
    def __init__(self, hits, misses, maxsize, currsize):
        self.hits = hits
        self.misses = misses
        self.maxsize = maxsize
        self.currsize = currsize

    def __iter__(self):
        return iter((self.hits, self.misses, self.maxsize, self.currsize))

    def __getitem__(self, i):
        return (self.hits, self.misses, self.maxsize, self.currsize)[i]

    def __eq__(self, other):
        return tuple(self) == tuple(other)

    def __repr__(self):
        return 'CacheInfo(hits=%r, misses=%r, maxsize=%r, currsize=%r)' % tuple(self)


def _make_key(args, kwds):
    if kwds:
        return (args, tuple(sorted(kwds.items())))
    return args


def lru_cache(maxsize=128, typed=False):
    if callable(maxsize) and not isinstance(maxsize, int):
        user_function, maxsize = maxsize, 128
        return _lru_wrapper(user_function, maxsize)

    def decorating_function(user_function):
        return _lru_wrapper(user_function, maxsize)
    return decorating_function


def _lru_wrapper(user_function, maxsize):
    cache = {}
    stats = [0, 0]

    def wrapper(*args, **kwds):
        key = _make_key(args, kwds)
        if key in cache:
            stats[0] += 1
            value = cache.pop(key)
            cache[key] = value
            return value
        stats[1] += 1
        result = user_function(*args, **kwds)
        cache[key] = result
        if maxsize is not None and len(cache) > maxsize:
            del cache[next(iter(cache))]
        return result

    def cache_info():
        return _CacheInfo(stats[0], stats[1], maxsize, len(cache))

    def cache_clear():
        cache.clear()
        stats[0] = 0
        stats[1] = 0

    wrapper.cache_info = cache_info
    wrapper.cache_clear = cache_clear
    wrapper.cache_parameters = lambda: {'maxsize': maxsize, 'typed': False}
    return update_wrapper(wrapper, user_function)


def cache(user_function):
    return _lru_wrapper(user_function, None)


_NOT_FOUND = object()


class cached_property:
    """Propriedade calculada uma vez e guardada no atributo da própria instância."""

    def __init__(self, func):
        self.func = func
        self.attrname = None
        self.__doc__ = getattr(func, '__doc__', None)

    def __set_name__(self, owner, name):
        if self.attrname is None:
            self.attrname = name
        elif name != self.attrname:
            raise TypeError(
                "Cannot assign the same cached_property to two different names "
                f"({self.attrname!r} and {name!r})."
            )

    def __get__(self, instance, owner=None):
        if instance is None:
            return self
        if self.attrname is None:
            raise TypeError("Cannot use cached_property instance without calling __set_name__ on it.")
        val = instance.__dict__.get(self.attrname, _NOT_FOUND)
        if val is _NOT_FOUND:
            val = self.func(instance)
            setattr(instance, self.attrname, val)
        return val


def total_ordering(cls):
    """Completa os métodos de comparação a partir de um deles e de `__eq__`."""
    def has(name):
        return name in cls.__dict__ or any(name in base.__dict__ for base in cls.__mro__[1:] if base is not object)

    if has('__lt__'):
        if not has('__gt__'):
            cls.__gt__ = lambda self, other: not (self < other) and self != other
        if not has('__le__'):
            cls.__le__ = lambda self, other: self < other or self == other
        if not has('__ge__'):
            cls.__ge__ = lambda self, other: not (self < other)
    elif has('__le__'):
        if not has('__ge__'):
            cls.__ge__ = lambda self, other: not (self <= other) or self == other
        if not has('__lt__'):
            cls.__lt__ = lambda self, other: self <= other and self != other
        if not has('__gt__'):
            cls.__gt__ = lambda self, other: not (self <= other)
    elif has('__gt__'):
        if not has('__lt__'):
            cls.__lt__ = lambda self, other: not (self > other) and self != other
        if not has('__ge__'):
            cls.__ge__ = lambda self, other: self > other or self == other
        if not has('__le__'):
            cls.__le__ = lambda self, other: not (self > other)
    elif has('__ge__'):
        if not has('__le__'):
            cls.__le__ = lambda self, other: not (self >= other) or self == other
        if not has('__gt__'):
            cls.__gt__ = lambda self, other: self >= other and self != other
        if not has('__lt__'):
            cls.__lt__ = lambda self, other: not (self >= other)
    else:
        raise ValueError('must define at least one ordering operation: < > <= >=')
    return cls


def singledispatch(func):
    registry = {}

    def dispatch(cls):
        for klass in cls.__mro__:
            if klass in registry:
                return registry[klass]
        return func

    def register(cls, f=None):
        if f is None:
            if isinstance(cls, type):
                return lambda f: register(cls, f)
            f = cls
            raise TypeError('singledispatch register() needs an explicit class')
        registry[cls] = f
        return f

    def wrapper(*args, **kw):
        if not args:
            raise TypeError(func.__name__ + ' requires at least 1 positional argument')
        return dispatch(type(args[0]))(*args, **kw)

    wrapper.register = register
    wrapper.dispatch = dispatch
    wrapper.registry = registry
    return update_wrapper(wrapper, func)
