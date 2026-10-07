"""itertools do sandbox (Python embutido)."""


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


def cycle(iterable):
    saved = []
    for element in iterable:
        yield element
        saved.append(element)
    while saved:
        for element in saved:
            yield element


def repeat(obj, times=None):
    if times is None:
        while True:
            yield obj
    else:
        for _ in range(times):
            yield obj


def accumulate(iterable, func=None, *, initial=None):
    it = iter(iterable)
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


class chain:
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


def compress(data, selectors):
    return (d for d, s in zip(data, selectors) if s)


def dropwhile(predicate, iterable):
    it = iter(iterable)
    for x in it:
        if not predicate(x):
            yield x
            break
    for x in it:
        yield x


def takewhile(predicate, iterable):
    for x in iterable:
        if predicate(x):
            yield x
        else:
            break


def filterfalse(predicate, iterable):
    if predicate is None:
        predicate = bool
    for x in iterable:
        if not predicate(x):
            yield x


def groupby(iterable, key=None):
    it = iter(iterable)
    sentinel = object()
    current = sentinel
    current_key = sentinel
    while True:
        if current is sentinel:
            try:
                current = next(it)
            except StopIteration:
                return
            current_key = current if key is None else key(current)
        group_key = current_key
        group = []
        while True:
            group.append(current)
            try:
                current = next(it)
            except StopIteration:
                current = sentinel
                break
            current_key = current if key is None else key(current)
            if current_key != group_key:
                break
        yield group_key, iter(group)


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


def pairwise(iterable):
    it = iter(iterable)
    try:
        a = next(it)
    except StopIteration:
        return
    for b in it:
        yield (a, b)
        a = b


def starmap(function, iterable):
    for args in iterable:
        yield function(*args)


def tee(iterable, n=2):
    data = list(iterable)
    return tuple(iter(data) for _ in range(n))


def zip_longest(*iterables, fillvalue=None):
    iterators = [iter(it) for it in iterables]
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


def product(*iterables, repeat=1):
    pools = [tuple(pool) for pool in iterables] * repeat
    result = [[]]
    for pool in pools:
        result = [x + [y] for x in result for y in pool]
    for prod in result:
        yield tuple(prod)


def permutations(iterable, r=None):
    pool = tuple(iterable)
    n = len(pool)
    r = n if r is None else r
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


def combinations(iterable, r):
    pool = tuple(iterable)
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


def combinations_with_replacement(iterable, r):
    pool = tuple(iterable)
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


def batched(iterable, n):
    if n < 1:
        raise ValueError('n must be at least one')
    it = iter(iterable)
    while True:
        batch = tuple(islice(it, n))
        if not batch:
            return
        yield batch
