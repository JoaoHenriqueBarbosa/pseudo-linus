"""O switch de greenlet dentro de código Python chamado por uma função nativa (a chave de sorted, list.sort, min,
o callback de map, filter e functools.reduce, functools.partial, compreensões, geradores, sum e any sobre
geradores, str.join, property, __init__, __next__ de usuário, __enter__ e __exit__): no CPython a pilha de C é
copiada junto, e a troca funciona em qualquer profundidade."""
import functools

import greenlet

main = greenlet.getcurrent()


def drive(body):
    child = greenlet.greenlet(body)
    seen = []
    try:
        value = child.switch()
        while not child.dead:
            seen.append(value)
            value = child.switch()
        return seen, value
    except Exception as exc:
        return type(exc).__name__, str(exc)


def k(x):
    main.switch(('k', x))
    return -x


class Enter:
    def __enter__(self):
        k(1)

    def __exit__(self, *exc):
        k(2)


class Counter:
    n = 0

    def __iter__(self):
        return self

    def __next__(self):
        k(self.n)
        if self.n > 1:
            raise StopIteration
        self.n += 1
        return self.n


def gen():
    yield k(1)


def with_body():
    with Enter():
        pass


def list_sort():
    items = [1, 2, 3]
    result = items.sort(key=k)
    return result, items


class Eq:
    def __init__(self, tag, equal=True):
        self.tag = tag
        self.equal = equal

    def __eq__(self, other):
        main.switch(('eq', self.tag))
        return self.equal

    __hash__ = object.__hash__


class Text:
    def __repr__(self):
        main.switch('repr')
        return 'R'

    def __str__(self):
        main.switch('str')
        return 'S'


def remove():
    items = [Eq('a', False), Eq('b'), Eq('c')]
    items.remove(1)
    return [x.tag for x in items]


class Shrink:
    def __eq__(self, other):
        main.switch('shrink')
        shrinking.clear()
        return False


shrinking = []


def shrink():
    shrinking[:] = [Shrink(), 1, 2]
    return 2 in shrinking, shrinking


cases = {
    'sorted': lambda: sorted([1, 2, 3], key=k),
    'sorted_reverse': lambda: sorted([1, 2, 3], key=k, reverse=True),
    'list.sort': list_sort,
    'min': lambda: min([1, 2], key=k),
    'max': lambda: max([1, 2], key=k),
    'map': lambda: list(map(k, [1, 2])),
    'filter': lambda: list(filter(k, [1, 2])),
    'reduce': lambda: functools.reduce(lambda a, b: main.switch(('r', a, b)) or a + b, [1, 2, 3]),
    'partial': lambda: functools.partial(k, 5)(),
    'listcomp': lambda: [k(x) for x in (1, 2)],
    'sum_gen': lambda: sum(k(x) for x in (1, 2)),
    'any_gen': lambda: any(k(x) for x in (1, 2)),
    'join_gen': lambda: ''.join(str(k(x)) for x in (1, 2)),
    'property': lambda: type('P', (), {'p': property(lambda s: k(1))})().p,
    'next': lambda: list(Counter()),
    'with': with_body,
    'generator': lambda: list(gen()),
    'in': lambda: 1 in [Eq('a', False), Eq('b')],
    'not_in': lambda: 1 not in (Eq('a', False),),
    'in_item': lambda: Eq('x', False) in [1, 2],
    'list.index': lambda: [Eq('a', False), Eq('b')].index(1),
    'list.index_bounds': lambda: [Eq('a'), Eq('b'), Eq('c')].index(1, 1, 3),
    'list.index_missing': lambda: [Eq('a', False)].index(1),
    'list.count': lambda: [Eq('a'), Eq('b', False), Eq('c')].count(1),
    'list.remove': remove,
    'list.remove_missing': lambda: [Eq('a', False)].remove(1),
    'tuple.index': lambda: (Eq('a', False), Eq('b')).index(1),
    'tuple.index_missing': lambda: (Eq('a', False),).index(1),
    'tuple.count': lambda: (Eq('a'), Eq('b')).count(1),
    'shrink': shrink,
    'repr': lambda: repr(Text()),
    'str': lambda: str(Text()),
}
for name, body in cases.items():
    print(name, drive(body))
