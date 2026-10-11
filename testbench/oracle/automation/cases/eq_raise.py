"""A exceção levantada por um __eq__ de usuário sai de toda comparação feita por código nativo (list.index,
list.count, list.remove, in, tuple.index, tuple.count, range, dict e set, operator, ==, igualdade de lista, de
dicionário e de referência fraca, iter com sentinela), e a lista mudada pelo __eq__ no meio da busca é relida
a cada passo."""
import operator
import weakref


class Boom:
    def __eq__(self, other):
        raise KeyError('boom')

    __hash__ = object.__hash__


class Collide:
    def __init__(self, tag):
        self.tag = tag

    def __hash__(self):
        return 1

    def __eq__(self, other):
        raise LookupError(self.tag)


b = Boom()
cases = [
    ('list.index', lambda: [b].index(1)),
    ('list.count', lambda: [b, b].count(1)),
    ('list.remove', lambda: [b].remove(1)),
    ('in_list', lambda: 1 in [b]),
    ('in_tuple', lambda: 1 in (b,)),
    ('tuple.index', lambda: (b,).index(1)),
    ('tuple.count', lambda: (b,).count(1)),
    ('range.index', lambda: range(3).index(b)),
    ('range.count', lambda: range(3).count(b)),
    ('list_eq', lambda: [b] == [1]),
    ('tuple_eq', lambda: (b,) == (1,)),
    ('dict_eq', lambda: {'k': b} == {'k': 1}),
    ('dict_key', lambda: {Collide('a'): 1}[Collide('b')]),
    ('set_in', lambda: Collide('c') in {Collide('d')}),
    ('set_add', lambda: {Collide('e'), Collide('f')}),
    ('operator.eq', lambda: operator.eq(b, 1)),
    ('operator.contains', lambda: operator.contains([b], 1)),
    ('operator.countOf', lambda: operator.countOf([b], 1)),
    ('operator.indexOf', lambda: operator.indexOf([b], 1)),
    ('items_in', lambda: ('k', 1) in {'k': b}.items()),
    ('iter_sentinel', lambda: list(iter(lambda: 1, b))),
]


class Ref:
    def __eq__(self, other):
        raise ValueError('ref')

    __hash__ = object.__hash__


r1, r2 = Ref(), Ref()
cases.append(('weakref_eq', lambda: weakref.ref(r1) == weakref.ref(r2)))

for name, call in cases:
    try:
        print(name, call())
    except Exception as exc:
        print(name, type(exc).__name__, exc)

items = [0, 1, 2, 3]


class Shrink:
    def __eq__(self, other):
        items.clear()
        return False


items[0] = Shrink()
print('shrink_in', 3 in items, items)
items = [Shrink(), 1, 2]
try:
    print('shrink_index', items.index(2))
except ValueError as exc:
    print('shrink_index', exc)
