import gc, weakref, sys

log = []


class D:
    def __init__(self, name):
        self.name = name

    def __del__(self):
        log.append(('del', self.name))


def pair(a, b):
    x = D(a); y = D(b); x.o = y; y.o = x


gc.collect()
pair('a1', 'a2')
print('collected', gc.collect(), log)
log.clear()

# finalizer runs once, in creation order, for three in a ring
x = D('r1'); y = D('r2'); z = D('r3'); x.n = y; y.n = z; z.n = x
del x, y, z
print('ring', gc.collect(), log)
log.clear()

# resurrection
saved = []


class R:
    def __del__(self):
        log.append('R del')
        saved.append(self)


r = R(); r.me = r
rr = weakref.ref(r)
del r
print('resurrect', gc.collect(), log, len(saved), rr() is saved[0])
log.clear()
print('second collect (still alive, finalizer not rerun)', gc.collect(), log)
print('is_finalized', gc.is_finalized(saved[0]))
saved.clear()
print('after drop', gc.collect(), log)
log.clear()

# weakref callbacks to cycle garbage: before the finalizer
cb = []


class W:
    def __del__(self):
        cb.append('W del')


w = W(); w.me = w
ref = weakref.ref(w, lambda r: cb.append('callback'))
del w
print('weakref', gc.collect(), cb, ref() is None)
cb.clear()

# the weakref itself in the garbage: no callback
class H:
    pass


h = H(); h.me = h
h.ref = weakref.ref(h, lambda r: cb.append('should not run'))
del h
print('weakref in garbage', gc.collect(), cb)

# DEBUG_SAVEALL
class S:
    pass


gc.set_debug(gc.DEBUG_SAVEALL)
s1 = S(); s2 = S(); s1.o = s2; s2.o = s1
del s1, s2
print('saveall', gc.collect(), len(gc.garbage), all(type(o).__name__ == 'S' for o in gc.garbage))
gc.set_debug(0)
del gc.garbage[:]
print('garbage cleared', gc.collect(), len(gc.garbage))

# callbacks
events = []


def cbk(phase, info):
    events.append((phase, dict(info)))


gc.callbacks.append(cbk)
pair('c1', 'c2')
log.clear()
print('with callbacks', gc.collect())
print(events)
gc.callbacks.remove(cbk)
events.clear()
gc.collect(0)
print('after remove', events)

# a __del__ that raises is reported and the cycle is still collected
errs = []
old = sys.unraisablehook
sys.unraisablehook = lambda u: errs.append((u.exc_type.__name__, str(u.exc_value)))


class E:
    def __del__(self):
        raise ValueError('boom')


e = E(); e.me = e
del e
print('raising del', gc.collect(), errs)
sys.unraisablehook = old

# __del__ that allocates a new cycle while collecting
class A:
    def __del__(self):
        pair('nested1', 'nested2')


a = A(); a.me = a
log.clear()
del a
print('allocating del', gc.collect(), gc.collect(), sorted(log))
