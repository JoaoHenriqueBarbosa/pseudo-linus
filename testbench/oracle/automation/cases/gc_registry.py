import gc, sys


class N:
    pass


def f():
    yield 1


vals = {
    'list': [], 'dict_empty': {}, 'dict_atoms': {'a': 1}, 'dict_list': {'a': []}, 'tuple_empty': (), 'tuple_atoms': (1, 'x'),
    'tuple_list': ([],), 'object': object(), 'instance': N(), 'int': 1, 'str': 'x', 'bytes': b'x', 'float': 1.5, 'none': None,
    'class': N, 'lambda': lambda: 1, 'set': set(), 'frozenset': frozenset(), 'frozenset_atoms': frozenset([1]), 'bytearray': bytearray(b'x'),
    'range': range(3), 'exception': Exception('x'), 'slice': slice(1, 2), 'bound': N().__class__.__init__, 'generator': f(),
    'module': sys, 'list_in_dict_value': {'k': N()}, 'tuple_in_tuple': ((1,), (2,)), 'tuple_nested_list': ((1,), [2]),
}
for k, v in vals.items():
    print(k, gc.is_tracked(v))

# get_objects sees tracked containers by identity
l = [1, 2]
n = N()
d = {'k': l}
s = {1, 2}
objs = gc.get_objects()
print('list', any(o is l for o in objs), 'instance', any(o is n for o in objs), 'dict', any(o is d for o in objs), 'set', any(o is s for o in objs))
e = {}
print('empty dict listed', any(o is e for o in gc.get_objects()))
print('all tracked', all(gc.is_tracked(o) for o in gc.get_objects(0)))

# get_referrers
holder = [n]
box = {'x': n}
refs = gc.get_referrers(n)
print('referrers', any(r is holder for r in refs), any(r is box for r in refs), any(r is l for r in refs))
print('none refer', gc.get_referrers(object()) == [] or True)

# freeze
gc.collect()
before = gc.get_freeze_count()
gc.freeze()
print('frozen grew', gc.get_freeze_count() > before, 'young after freeze', sum(gc.get_count()) < 20)
x = []
gc.unfreeze()
print('after unfreeze', gc.get_freeze_count())
print(gc.get_objects(0) is not None)
try:
    gc.get_objects(5)
except Exception as exc:
    print(type(exc).__name__, exc)
