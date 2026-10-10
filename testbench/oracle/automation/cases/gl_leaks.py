"""A superfície do greenlet.greenlet que o programa enxerga: o estado interno não aparece (dir, __dict__, vars,
pickle), o tipo é estático como no C (o __dict__ da classe é o do oráculo), as funções de módulo são
builtin_function_or_method de greenlet._greenlet e o dicionário de instância guarda só o que o programa grava."""
import inspect
import pickle
import weakref

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
for call in (lambda: g.__reduce__(), lambda: g.__reduce_ex__(2), lambda: pickle.dumps(g)):
    try:
        print('pickle_like', call())
    except Exception as exc:
        print('pickle_like', type(exc).__name__, exc)
print('getstate', hasattr(g, '__getstate__'))
print('del', hasattr(g, '__del__'), hasattr(greenlet.greenlet, '__del__'), hasattr(greenlet.greenlet, '__slots__'))
print('flags', hex(greenlet.greenlet.__flags__))
g.x = 1
print('x', g.__dict__, vars(g), [n for n in dir(g) if n not in dir(greenlet.greenlet)])


class S(greenlet.greenlet):
    pass


print('sub_vars', list(vars(S)))
print('errors', list(vars(greenlet.error)), list(vars(greenlet.GreenletExit)))
print('unswitchable', list(vars(greenlet._greenlet.UnswitchableGreenlet)))
print('weakref', weakref.ref(g)() is g)
