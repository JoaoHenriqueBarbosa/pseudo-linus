"""itertools do sandbox (Python embutido)."""

from types import GenericAlias as _GenericAlias

# Os iteradores do `itertools` são classes (tipos em C no CPython): cada uma valida os argumentos ao ser criada, como
# o `tp_new`, e entrega os itens por um gerador privado, que só existe aqui dentro.

_NOTHING = object()


def _same(a, b):
    """`PyObject_RichCompareBool(a, b, Py_EQ)`: a identidade vale antes da igualdade."""
    return a is b or bool(a == b)


class count:
    def __init__(self, start=0, step=1):
        self._n = start
        self._step = step

    def __iter__(self):
        return self

    def __next__(self):
        n = self._n
        self._n = n + self._step
        return n

    def __repr__(self):
        if self._step == 1:
            return 'count(%r)' % (self._n,)
        return 'count(%r, %r)' % (self._n, self._step)


def _cycle(it):
    saved = []
    for element in it:
        yield element
        saved.append(element)
    while saved:
        for element in saved:
            yield element


class cycle:
    def __init__(self, iterable, /):
        self._gen = _cycle(iter(iterable))

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


class repeat:
    def __init__(self, object, times=None):
        if times is not None and times < 0:
            times = 0
        self._object = object
        self._times = times

    def __iter__(self):
        return self

    def __next__(self):
        if self._times is not None:
            if self._times <= 0:
                raise StopIteration
            self._times -= 1
        return self._object

    def __length_hint__(self):
        if self._times is None:
            raise TypeError('len() of unsized object')
        return self._times

    def __repr__(self):
        if self._times is None:
            return 'repeat(%r)' % (self._object,)
        return 'repeat(%r, %d)' % (self._object, self._times)


def _accumulate(it, func, initial):
    total = initial
    if initial is None:
        try:
            total = next(it)
        except StopIteration:
            return
    yield total
    for element in it:
        total = total + element if func is None else func(total, element)
        yield total


class accumulate:
    def __init__(self, iterable, func=None, *, initial=None):
        self._gen = _accumulate(iter(iterable), func, initial)

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


class chain:
    __class_getitem__ = classmethod(_GenericAlias)

    def __init__(self, *iterables):
        self._iterables = iter(iterables)
        self._current = iter(())

    @classmethod
    def from_iterable(cls, iterables):
        obj = cls()
        obj._iterables = iter(iterables)
        return obj

    def __iter__(self):
        return self

    def __next__(self):
        while True:
            try:
                return next(self._current)
            except StopIteration:
                self._current = iter(next(self._iterables))


class compress:
    def __init__(self, data, selectors):
        self._gen = (d for d, s in zip(data, selectors) if s)

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


def _dropwhile(predicate, it):
    for x in it:
        if not predicate(x):
            yield x
            break
    for x in it:
        yield x


class dropwhile:
    def __init__(self, predicate, iterable, /):
        self._gen = _dropwhile(predicate, iter(iterable))

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


def _takewhile(predicate, it):
    for x in it:
        if predicate(x):
            yield x
        else:
            break


class takewhile:
    def __init__(self, predicate, iterable, /):
        self._gen = _takewhile(predicate, iter(iterable))

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


def _filterfalse(predicate, it):
    if predicate is None:
        predicate = bool
    for x in it:
        if not predicate(x):
            yield x


class filterfalse:
    def __init__(self, function, iterable, /):
        self._gen = _filterfalse(function, iter(iterable))

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


class groupby:
    """groupby(iterable, key=None) -> make an iterator that returns consecutive keys and groups from the iterable"""

    def __init__(self, iterable, key=None):
        self._it = iter(iterable)
        self._keyfunc = key
        self._tgtkey = _NOTHING
        self._currkey = _NOTHING
        self._currvalue = _NOTHING
        self._currgrouper = None

    def __iter__(self):
        return self

    def _step(self):
        newvalue = next(self._it)
        newkey = newvalue if self._keyfunc is None else self._keyfunc(newvalue)
        self._currvalue = newvalue
        self._currkey = newkey

    def __next__(self):
        self._currgrouper = None
        # Pula até a próxima chave diferente.
        if self._currvalue is _NOTHING:
            self._step()
        while self._tgtkey is not _NOTHING and _same(self._tgtkey, self._currkey):
            self._step()
        self._tgtkey = self._currkey
        grouper = _grouper(self, self._tgtkey)
        self._currgrouper = grouper
        return (self._currkey, grouper)


class _grouper:
    def __init__(self, parent, tgtkey):
        self._parent = parent
        self._tgtkey = tgtkey

    def __iter__(self):
        return self

    def __next__(self):
        parent = self._parent
        if parent._currgrouper is not self:
            raise StopIteration
        if parent._currvalue is _NOTHING:
            parent._step()
        if not _same(self._tgtkey, parent._currkey):
            raise StopIteration
        value = parent._currvalue
        parent._currvalue = _NOTHING
        parent._currkey = _NOTHING
        return value


class islice:
    """islice(iterable, stop) --> islice object
islice(iterable, start, stop[, step]) --> islice object

Return an iterator whose next() method returns selected values from an
iterable.  If start is specified, will skip all preceding elements;
otherwise, start defaults to zero.  Step defaults to one.  If
specified as another value, step determines how many values are
skipped between successive calls.  Works like a slice() on a list
but returns an iterator."""

    def __new__(cls, *args, **kwargs):
        # Validação na criação, como o `islice_new`: a mensagem de cada argumento é a do C.
        if cls is islice and kwargs:
            raise TypeError('islice() takes no keyword arguments')
        if len(args) < 2:
            raise TypeError(f'islice expected at least 2 arguments, got {len(args)}')
        if len(args) > 4:
            raise TypeError(f'islice expected at most 4 arguments, got {len(args)}')
        stop_msg = 'Stop argument for islice() must be None or an integer: 0 <= x <= sys.maxsize.'

        def as_index(v):
            from operator import index
            try:
                n = index(v)
            except TypeError:
                return -1
            return n if n <= 9223372036854775807 else -1

        start, stop, step = 0, -1, 1
        if len(args) == 2:
            if args[1] is not None:
                stop = as_index(args[1])
                if stop == -1:
                    raise ValueError(stop_msg)
        else:
            if args[1] is not None:
                start = as_index(args[1])
            if args[2] is not None:
                stop = as_index(args[2])
                if stop == -1:
                    raise ValueError(stop_msg)
        if start < 0 or stop < -1:
            raise ValueError('Indices for islice() must be None or an integer: 0 <= x <= sys.maxsize.')
        if len(args) == 4 and args[3] is not None:
            step = as_index(args[3])
        if step < 1:
            raise ValueError('Step for islice() must be a positive integer or None.')
        self = object.__new__(cls)
        self._it = iter(args[0])
        self._next = start
        self._stop = stop
        self._step = step
        self._cnt = 0
        return self

    def __iter__(self):
        return self

    def __next__(self):
        # Como o `islice_next`: pula até o próximo índice, e no fim solta o iterável.
        it = self._it
        if it is None:
            raise StopIteration
        stop = self._stop
        while self._cnt < self._next:
            try:
                next(it)
            except StopIteration:
                self._it = None
                raise
            self._cnt += 1
        if stop != -1 and self._cnt >= stop:
            self._it = None
            raise StopIteration
        try:
            item = next(it)
        except StopIteration:
            self._it = None
            raise
        self._cnt += 1
        oldnext = self._next
        self._next += self._step
        if self._next < oldnext or (stop != -1 and self._next > stop):
            self._next = stop
        return item


def _pairwise(it):
    try:
        a = next(it)
    except StopIteration:
        return
    for b in it:
        yield (a, b)
        a = b


class pairwise:
    def __init__(self, iterable, /):
        self._gen = _pairwise(iter(iterable))

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


def _starmap(function, it):
    for args in it:
        yield function(*args)


class starmap:
    def __init__(self, function, iterable, /):
        self._gen = _starmap(function, iter(iterable))

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


_LINKCELLS = 57


class _tee_dataobject:
    """Um bloco de até `_LINKCELLS` itens já lidos do iterador, ligado ao bloco seguinte."""

    def __init__(self, it):
        self._it = it
        self._values = []
        self._nextlink = None

    def _getitem(self, i):
        if i < len(self._values):
            return self._values[i]
        value = next(self._it)
        self._values.append(value)
        return value


class _tee:
    def __init__(self, dataobj, index=0):
        self._data = dataobj
        self._index = index

    def __iter__(self):
        return self

    def __next__(self):
        if self._index >= _LINKCELLS:
            following = self._data._nextlink
            if following is None:
                following = self._data._nextlink = _tee_dataobject(self._data._it)
            self._data = following
            self._index = 0
        value = self._data._getitem(self._index)
        self._index += 1
        return value

    def __copy__(self):
        return _tee(self._data, self._index)


def tee(iterable, n=2, /):
    """Returns a tuple of n independent iterators."""
    if n < 0:
        raise ValueError('n must be >= 0')
    if n == 0:
        return ()
    it = iter(iterable)
    copyfunc = getattr(it, '__copy__', None)
    if copyfunc is None:
        copyable = _tee(_tee_dataobject(it))
        copyfunc = copyable.__copy__
    else:
        copyable = it
    result = [copyable]
    for _ in range(n - 1):
        result.append(copyfunc())
    return tuple(result)


def _zip_longest(iterators, fillvalue):
    if not iterators:
        return
    active = len(iterators)
    while True:
        values = []
        for i, it in enumerate(iterators):
            if it is None:
                values.append(fillvalue)
                continue
            try:
                values.append(next(it))
            except StopIteration:
                active -= 1
                iterators[i] = None
                values.append(fillvalue)
        if active == 0:
            return
        yield tuple(values)


class zip_longest:
    def __init__(self, *iterables, fillvalue=None):
        self._gen = _zip_longest([iter(it) for it in iterables], fillvalue)

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


def _product(pools):
    if any(not pool for pool in pools):
        return
    indices = [0] * len(pools)
    yield tuple(pool[i] for pool, i in zip(pools, indices))
    while True:
        for i in reversed(range(len(pools))):
            indices[i] += 1
            if indices[i] < len(pools[i]):
                break
            indices[i] = 0
        else:
            return
        yield tuple(pool[i] for pool, i in zip(pools, indices))


class product:
    def __init__(self, *iterables, repeat=1):
        if repeat < 0:
            raise ValueError('repeat argument cannot be negative')
        self._gen = _product([tuple(pool) for pool in iterables] * repeat)

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


def _permutations(pool, r):
    n = len(pool)
    if r > n:
        return
    indices = list(range(n))
    cycles = list(range(n, n - r, -1))
    yield tuple(pool[i] for i in indices[:r])
    while n:
        for i in reversed(range(r)):
            cycles[i] -= 1
            if cycles[i] == 0:
                indices[i:] = indices[i + 1:] + indices[i:i + 1]
                cycles[i] = n - i
            else:
                j = cycles[i]
                indices[i], indices[-j] = indices[-j], indices[i]
                yield tuple(pool[i] for i in indices[:r])
                break
        else:
            return


class permutations:
    def __init__(self, iterable, r=None):
        pool = tuple(iterable)
        r = len(pool) if r is None else r
        if r < 0:
            raise ValueError('r must be non-negative')
        self._gen = _permutations(pool, r)

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


def _combinations(pool, r):
    n = len(pool)
    if r > n:
        return
    indices = list(range(r))
    yield tuple(pool[i] for i in indices)
    while True:
        for i in reversed(range(r)):
            if indices[i] != i + n - r:
                break
        else:
            return
        indices[i] += 1
        for j in range(i + 1, r):
            indices[j] = indices[j - 1] + 1
        yield tuple(pool[i] for i in indices)


class combinations:
    def __init__(self, iterable, r):
        pool = tuple(iterable)
        if r < 0:
            raise ValueError('r must be non-negative')
        self._gen = _combinations(pool, r)

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


def _combinations_with_replacement(pool, r):
    n = len(pool)
    if not n and r:
        return
    indices = [0] * r
    yield tuple(pool[i] for i in indices)
    while True:
        for i in reversed(range(r)):
            if indices[i] != n - 1:
                break
        else:
            return
        indices[i:] = [indices[i] + 1] * (r - i)
        yield tuple(pool[i] for i in indices)


class combinations_with_replacement:
    def __init__(self, iterable, r):
        pool = tuple(iterable)
        if r < 0:
            raise ValueError('r must be non-negative')
        self._gen = _combinations_with_replacement(pool, r)

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


def _batched(it, n, strict):
    while True:
        batch = tuple(islice(it, n))
        if not batch:
            return
        if strict and len(batch) != n:
            raise ValueError('batched(): incomplete batch')
        yield batch


class batched:
    def __init__(self, iterable, n, *, strict=False):
        if n < 1:
            raise ValueError('n must be at least one')
        self._gen = _batched(iter(iterable), n, strict)

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)


del _GenericAlias
