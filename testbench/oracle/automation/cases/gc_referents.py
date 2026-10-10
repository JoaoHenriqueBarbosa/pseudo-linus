"""gc.get_referents como o tp_traverse do CPython 3.13: o que cada tipo básico visita, em que ordem, e o greenlet."""

import gc
import sys

import greenlet


def names(*objs):
    return [type(r).__name__ for r in gc.get_referents(*objs)]


def show(name, call):
    try:
        print(name, call())
    except Exception as exc:
        print(name, type(exc).__name__, exc)


class Plain:
    pass


class WithInit:
    def __init__(self):
        self.a = [1]
        self.b = 'x'


class Slotted:
    __slots__ = ('x', 'y')


class SlottedDict:
    __slots__ = ('x', '__dict__')


def outer():
    cell = []

    def inner(a, b=(1,), *, c={}):
        """doc"""
        return cell
    return inner


def plain_func():
    pass


s = Slotted()
s.x = [1]
sd = SlottedDict()
sd.x = (2,)
sd.z = {}
w = WithInit()
p = Plain()
inner = outer()

show('scalars', lambda: names(1, 1.5, 'a', b'a', None, True, 2 ** 100, 1j))
show('list', lambda: names([1, 'a', None, [2]]))
show('list_ident', lambda: gc.get_referents([p, w]) == [p, w])
show('tuple', lambda: names((1, 'a', [2])))
show('dict_str', lambda: names({'a': [1], 'b': 2}))
show('dict_mixed', lambda: names({1: [1], 'b': (2,)}))
show('dict_tuple_key', lambda: names({(1,): 'v'}))
show('set', lambda: sorted(names({1, 'a', (2,)})))
show('frozenset', lambda: sorted(names(frozenset({1, 'a', (2,)}))))
show('plain', lambda: names(p))
show('plain_type', lambda: gc.get_referents(p)[-1:] == [Plain] if gc.get_referents(p) else [])
show('with_init', lambda: names(w))
show('slotted', lambda: names(s))
show('slotted_empty', lambda: names(Slotted()))
show('slotted_dict', lambda: names(sd))
show('func', lambda: names(inner))
show('plain_func', lambda: names(plain_func))
show('lambda', lambda: names(lambda: 0))
show('cell', lambda: names(inner.__closure__[0]))
show('method', lambda: names(w.__init__))
show('builtin_method', lambda: names([].append))
show('builtin', lambda: names(len))
show('module', lambda: names(sys.modules['gc']))
show('range', lambda: names(range(3)))
show('slice', lambda: names(slice(1, [2], None)))
show('bytearray', lambda: names(bytearray(b'a')))
show('many', lambda: names([1], (2,), {'a': 3}))
show('none_args', lambda: names())
show('type_plain', lambda: names(Plain))
show('property', lambda: names(property(plain_func)))
show('staticmethod', lambda: names(staticmethod(plain_func)))
show('classmethod', lambda: names(classmethod(plain_func)))
show('iter_list', lambda: names(iter([1, 2])))
show('enumerate', lambda: names(enumerate([1])))
show('zip', lambda: names(zip([1], [2])))
show('map', lambda: names(map(str, [1])))


def gen():
    yield 1


show('generator', lambda: names(gen()))

g = greenlet.greenlet(lambda: 1)
show('greenlet_new', lambda: sorted(names(g)))
show('greenlet_parent', lambda: [r is greenlet.getcurrent() for r in gc.get_referents(g) if type(r) is greenlet.greenlet])
g.__dict__
show('greenlet_dict', lambda: sorted(names(g)))
show('greenlet_main', lambda: sorted(names(greenlet.getcurrent())))
done = greenlet.greenlet(lambda: 1)
done.switch()
show('greenlet_dead', lambda: sorted(names(done)))
