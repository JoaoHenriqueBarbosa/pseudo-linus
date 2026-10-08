"""`_functools`: o tipo `_lru_cache_wrapper` que o `functools.lru_cache` usa no CPython (o `functools.py` do Debian
só cai na versão em Python quando este módulo falta)."""

from types import MethodType
from _thread import RLock

# A construção de chaves é a do `functools`, que só existe por inteiro depois de importar este módulo: a função é
# buscada na primeira instância e guardada aqui.
_make_key = None
_sentinel = object()


def _key_maker():
    global _make_key
    if _make_key is None:
        from functools import _make_key as maker
        _make_key = maker
    return _make_key


class _lru_cache_wrapper:
    """Create a cached callable that wraps another function.

user_function:      the function being cached

maxsize:  0         for no caching
          None      for unlimited cache size
          n         for a bounded cache

typed:    False     cache f(3) and f(3.0) as identical calls
          True      cache f(3) and f(3.0) as distinct calls

cache_info_type:    namedtuple class with the fields:
                        hits misses currsize maxsize
"""

    __module__ = 'functools'
    __slots__ = ('__dict__', '_function', '_maxsize', '_typed', '_info_type', '_cache', '_hits', '_misses', '_lock',
                 '_key')

    def __init__(self, user_function, maxsize, typed, cache_info_type):
        if not callable(user_function):
            raise TypeError('the first argument must be callable')
        if maxsize is not None:
            if not hasattr(type(maxsize), '__index__'):
                raise TypeError('maxsize should be integer or None')
            maxsize = max(maxsize.__index__(), 0)
        self._function = user_function
        self._maxsize = maxsize
        self._typed = bool(typed)
        self._info_type = cache_info_type
        self._cache = {}
        self._hits = 0
        self._misses = 0
        self._lock = RLock()
        self._key = _key_maker()

    def __call__(self, /, *args, **kwds):
        maxsize = self._maxsize
        if maxsize == 0:
            # Sem cache: toda chamada é um erro de cache.
            self._misses += 1
            return self._function(*args, **kwds)
        key = self._key(args, kwds, self._typed)
        cache = self._cache
        if maxsize is None:
            result = cache.get(key, _sentinel)
            if result is not _sentinel:
                self._hits += 1
                return result
            self._misses += 1
            result = self._function(*args, **kwds)
            cache[key] = result
            return result
        # Cache limitado: o dicionário guarda a ordem de uso, do menos para o mais recente.
        lock = self._lock
        with lock:
            result = cache.pop(key, _sentinel)
            if result is not _sentinel:
                cache[key] = result
                self._hits += 1
                return result
            self._misses += 1
        result = self._function(*args, **kwds)
        with lock:
            # Outra thread pode ter posto a mesma chave enquanto o lock estava solto: então a entrada fica como está.
            if key not in cache:
                if len(cache) >= maxsize:
                    del cache[next(iter(cache))]
                cache[key] = result
        return result

    def cache_info(self):
        """Report cache statistics"""
        with self._lock:
            return self._info_type(self._hits, self._misses, self._maxsize, len(self._cache))

    def cache_clear(self):
        """Clear the cache and cache statistics"""
        with self._lock:
            self._cache.clear()
            self._hits = 0
            self._misses = 0

    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return MethodType(self, obj)

    def __copy__(self):
        return self

    def __deepcopy__(self, memo):
        return self

    def __reduce__(self):
        return self.__qualname__
