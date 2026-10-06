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


def islice(iterable, *args):
    s = slice(*args)
    start = 0 if s.start is None else s.start
    stop = 9223372036854775807 if s.stop is None else s.stop
    step = 1 if s.step is None else s.step
    if start < 0 or stop < 0 or step <= 0:
        raise ValueError('Indices for islice() must be None or an integer: 0 <= x <= sys.maxsize.')
    it = iter(range(start, stop, step))
    try:
        nexti = next(it)
    except StopIteration:
        # Consome o iterável só até `start`.
        for i, element in zip(range(start), iterable):
            pass
        return
    try:
        for i, element in enumerate(iterable):
            if i == nexti:
                yield element
                nexti = next(it)
    except StopIteration:
        # Consome o iterável até `stop`, sem passar dele.
        for i, element in zip(range(i + 1, stop), iterable):
            pass


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
