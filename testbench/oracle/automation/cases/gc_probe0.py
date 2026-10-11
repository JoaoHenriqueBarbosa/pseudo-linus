import gc, sys
print(sys.version.split()[0])
print(gc.get_threshold(), gc.get_count(), gc.isenabled(), gc.get_debug())
print(gc.get_stats())
print(gc.get_freeze_count())
print(len(gc.get_objects()) > 1000, len(gc.get_objects(0)) < 5000, len(gc.get_objects(1)), len(gc.get_objects(2)) > 100)
gc.collect()
print(gc.get_count())

class N:
    pass

def cyc(n):
    for _ in range(n):
        a = N(); b = N(); a.o = b; b.o = a

gc.collect()
cyc(10)
print('collect', gc.collect())
l = []; l.append(l)
d = {}; d['d'] = d
print('collect2', gc.collect())
del l, d
print('collect3', gc.collect())
a = N(); a.me = a
del a
print('collect4', gc.collect())
a = N(); b = N(); a.b = b; b.a = a
del a, b
print('collect5', gc.collect())
t = []
t.append((t,))
del t
print('collect6', gc.collect())

def mk():
    x = []
    def f():
        return x
    x.append(f)
mk()
print('collect closure', gc.collect())
print(gc.get_stats())
print(gc.get_count())
gc.disable()
cyc(10)
print(gc.get_count())
print(gc.collect(0), gc.collect(1), gc.collect(2))
print(gc.get_stats())
