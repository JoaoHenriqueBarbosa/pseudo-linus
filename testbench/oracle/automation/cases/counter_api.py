"""collections.Counter como o do CPython 3.13: operadores, most_common, elements, total, repr, comparações e erros."""

from collections import Counter

a = Counter('abracadabra')
b = Counter({'a': 2, 'b': -1, 'z': 3})
print('repr', a, b, Counter())
print('most', a.most_common(), a.most_common(2), a.most_common(0), Counter('ba').most_common(1))
print('elements', sorted(a.elements()), list(Counter(x=2, y=0, w=-1).elements()))
print('total', a.total(), b.total())
print('ops', a + b, a - b, a | b, a & b, +b, -b)
c = Counter(a)
c += b
print('iadd', c)
c -= Counter(a=10)
print('isub', c)
c |= Counter(q=4)
print('ior', c)
c &= Counter(q=2, r=1)
print('iand', c)
print('cmp', Counter(a=1) == Counter(a=1, b=0), Counter(a=1) <= Counter(a=2), Counter(a=2) < Counter(a=2),
      Counter(a=3) >= Counter(a=1), Counter(a=1) > Counter(), Counter(a=1) != {'a': 1})
try:
    Counter.fromkeys('abc')
except NotImplementedError as e:
    print('fromkeys', e)
d = Counter(a=3)
d.subtract({'a': 1, 'b': 2})
d.subtract('aa')
print('subtract', d)
d.update({'a': 5})
d.update('bb', z=1)
print('update', d)
del d['missing']
print('del', d, d['missing'])
print('copy', type(d.copy()).__name__, d.copy() == d)
print('reduce', d.__reduce__())
print('notimpl', a.__add__(1), a.__lt__(1))
try:
    a + 1
except TypeError as e:
    print('typeerror', e)
print('unorderable', repr(Counter({1: 1, 'x': 1})))
print('doc', Counter.__doc__.splitlines()[0])
print('heap', Counter(range(20)).most_common(3))
