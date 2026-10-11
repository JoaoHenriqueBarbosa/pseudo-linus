"""gc.get_referents do greenlet: quais campos o tp_traverse visita (pai, run, contexto, quadro, __dict__), por estado."""

import contextvars
import gc

import greenlet

main = greenlet.getcurrent()


def label(g, run, ctx, r):
    for name, obj in (('self', g), ('main', main), ('run', run), ('ctx', ctx)):
        if obj is not None and r is obj:
            return name
    if hasattr(g, '__dict__') and r is g.__dict__:
        return 'dict'
    return type(r).__name__


def refs(title, g, run=None, ctx=None):
    print(title, [label(g, run, ctx, r) for r in gc.get_referents(g)])


def body():
    refs('running', greenlet.getcurrent(), body)
    main.switch()


refs('main', main)
g = greenlet.greenlet(body)
refs('new', g, body)
g.switch()
refs('suspended', g, body)
g.switch()
refs('dead', g, body)

child = greenlet.greenlet(lambda: 1, parent=g)
refs('parent_dead', child)
other = greenlet.greenlet(lambda: 1)
other.parent = child
refs('parent_child', other)

c = contextvars.Context()
withctx = greenlet.greenlet(lambda: 1)
withctx.gr_context = c
refs('with_ctx', withctx, None, c)
ctxrun = greenlet.greenlet(lambda: main.switch())
ctxrun.gr_context = c
ctxrun.switch()
refs('ctx_suspended', ctxrun, None, c)
ctxrun.switch()
refs('ctx_dead', ctxrun, None, c)


class Sub(greenlet.greenlet):
    def run(self):
        main.switch()


s = Sub()
refs('sub_new', s)
s.switch()
refs('sub_suspended', s)
s.attr = [1]
refs('sub_attr', s)
refs('sub_attr_dict', s)
vars(s)
refs('sub_vars', s)
print('count_dead', len(gc.get_referents(g)))
