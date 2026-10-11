import gc


class N:
    pass


def cyc(n):
    for _ in range(n):
        a = N(); b = N(); a.o = b; b.o = a


def stats():
    return [dict(s) for s in gc.get_stats()]


print(gc.get_threshold(), gc.isenabled())
gc.collect()
base = stats()
cyc(10)
print('pairs', gc.collect())
l = []; l.append(l)
d = {}; d['d'] = d
print('alive', gc.collect())
del l, d
print('list+dict', gc.collect())
a = N(); a.me = a
del a
print('self', gc.collect())
a = N(); b = N(); a.b = b; b.a = a
del a, b
print('pair', gc.collect())
t = []
t.append((t,))
del t
print('tuple', gc.collect())


def mk():
    x = []

    def f():
        return x
    x.append(f)


mk()
print('closure', gc.collect())


def mk2():
    x = []
    y = []

    def f():
        return x, y
    x.append(f)
    y.append(x)


mk2()
print('closure2', gc.collect())

s = set()
s.add(frozenset([1]))
holder = [s]
s.add(id)
del holder
print('plain', gc.collect())

o = N()
o.__dict__
o.self = o
del o
print('materialized dict', gc.collect())

class P(list):
    pass

p = P()
p.append(p)
del p
print('list subclass', gc.collect())

# stats
after = stats()
print([(a['collections'] - b['collections'], a['collected'] - b['collected']) for a, b in zip(after, base)])

# explicit generations
gc.disable()
print('disabled', gc.isenabled())
cyc(5)
print('gen0', gc.collect(0))
cyc(5)
print('gen1', gc.collect(1))
cyc(5)
print('gen2', gc.collect(2))
gc.enable()
print('enabled', gc.isenabled())
try:
    gc.collect(3)
except Exception as e:
    print(type(e).__name__, e)
try:
    gc.collect(-1)
except Exception as e:
    print(type(e).__name__, e)

gc.set_threshold(5, 2, 3)
print(gc.get_threshold())
gc.set_threshold(100)
print(gc.get_threshold())
gc.set_threshold(2000, 10, 10)

# memory: a loop of cycles does not grow the heap
gc.collect()
before = len(gc.get_objects())
for _ in range(20000):
    cyc(1)
gc.collect()
after = len(gc.get_objects())
print('no growth', after - before < 200)
# automatic collection keeps the heap bounded without explicit collect
gc.collect()
before = len(gc.get_objects())
for _ in range(50000):
    cyc(1)
print('auto bounded', len(gc.get_objects()) - before < 10000)
