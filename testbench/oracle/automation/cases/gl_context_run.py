import contextvars
import greenlet
from greenlet import getcurrent, greenlet as G

V = contextvars.ContextVar('v')
print('main before', getcurrent().gr_context)


def inside(callback):
    print('inside run, ctx set', getcurrent().gr_context is not None)
    V.set(1)
    callback()
    return 'done'


ctx = contextvars.copy_context()
print('after copy_context', getcurrent().gr_context is not None)
g = G(lambda: ctx.run(inside, getcurrent().parent.switch))
print('first switch', g.switch())
print('suspended in run', g.gr_context is not None)
print('finish', g.switch(), g.dead)
print('dead gr_context', g.gr_context)

# a thread/greenlet that never touched a context: run() leaves it with none underneath
fresh = contextvars.Context()
h = G(lambda: fresh.run(lambda: 7))
print('result', h.switch(), h.dead, h.gr_context)

# explicit context set before start stays after death (no run involved)
k = G(lambda: V.set(2))
k.gr_context = contextvars.copy_context()
k.switch()
print('explicit', k.dead, k.gr_context is not None)
