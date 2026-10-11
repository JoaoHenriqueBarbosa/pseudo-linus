"""A superfície do greenlet que o programa enxerga e os casos que o porte do ul-python errava: o estado interno não
aparece (dir, __dict__, vars, gc.get_referents, pickle), o __module__ das funções, o switch dentro de callback de
nativa (sorted, functools.reduce, map, __eq__ chamado por list.index), a subclasse com run como método solta e coletada
pelo gc de ciclos, e o gr_context trocando o contexto do contextvars por greenlet."""
import contextvars
import functools
import gc
import inspect
import pickle

import greenlet

g = greenlet.greenlet(lambda: 1)
print('dict', g.__dict__)
print('vars', vars(g))
print('dir_cls', dir(greenlet.greenlet))
print('dir_inst_extra', [n for n in dir(g) if n not in dir(greenlet.greenlet)])
for name in ('getcurrent', 'gettrace', 'settrace'):
    obj = getattr(greenlet, name)
    print(name, type(obj).__name__, getattr(obj, '__module__', None), getattr(obj, '__qualname__', None))
    print(name, 'same_as_cls', obj is getattr(greenlet.greenlet, name), inspect.isbuiltin(obj))
print('cls', greenlet.greenlet.__module__, greenlet.greenlet.__qualname__, greenlet.GreenletExit.__module__,
      greenlet.error.__module__, greenlet._greenlet.UnswitchableGreenlet.__module__)
print('module_names', sorted(n for n in dir(greenlet._greenlet) if not n.startswith('__')))
print('cls_dict', sorted(greenlet.greenlet.__dict__))
print('referents', sorted(type(r).__name__ for r in gc.get_referents(g)))
for call in (lambda: g.__reduce__(), lambda: g.__reduce_ex__(2), lambda: pickle.dumps(g)):
    try:
        print('pickle_like', call())
    except Exception as exc:
        print('pickle_like', type(exc).__name__, exc)
print('getstate', hasattr(g, '__getstate__'))

main = greenlet.getcurrent()


def drive(body):
    child = greenlet.greenlet(body)
    seen = []
    value = child.switch()
    while not child.dead:
        seen.append(value)
        value = child.switch()
    return seen, value


def sorted_body():
    def key(x):
        main.switch(('k', x))
        return -x
    return sorted([1, 2, 3], key=key)


print('sorted', drive(sorted_body))
print('reduce', drive(lambda: functools.reduce(lambda a, b: main.switch(('r', a, b)) or a + b, [1, 2, 3])))
print('map', drive(lambda: list(map(lambda x: main.switch(('m', x)) or x * 2, [1, 2]))))


class Eq:
    def __eq__(self, other):
        main.switch('eq')
        return True


print('eq', drive(lambda: [Eq()].index(1)))

log = []


class Sub(greenlet.greenlet):
    def run(self):
        try:
            main.switch('in')
        except greenlet.GreenletExit:
            log.append('exit')
            raise
        finally:
            log.append('finally')


sub = Sub()
print('sub', sub.switch())
del sub
print('sub_after_del', log)
gc.collect()
print('sub_after_collect', log)

var = contextvars.ContextVar('var', default='default')


def ctx_body():
    print('ctx_in', var.get())
    var.set('child')
    main.switch()
    print('ctx_back', var.get())


var.set('main')
child = greenlet.greenlet(ctx_body)
child.switch()
print('ctx_main', var.get())
print('ctx_gr_context', type(child.gr_context).__name__, child.gr_context.get(var))
child.switch()
fresh = greenlet.greenlet(ctx_body)
fresh.gr_context = contextvars.Context()
fresh.switch()
print('ctx_main2', var.get())
fresh.switch()
print('ctx_current', greenlet.getcurrent().gr_context is None)
try:
    greenlet.getcurrent().gr_context = 1
except Exception as exc:
    print('ctx_bad', type(exc).__name__, exc)
