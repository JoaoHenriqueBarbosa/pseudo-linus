"""O `__del__` roda como quadro do laço (pode trocar de greenlet); os irmãos que morrem juntos rodam em sequência, sem
empilhar; o que morre dentro de um `__del__` roda dentro dele; o erro dele vai ao `unraisablehook`."""
import sys
import greenlet

results = []
main = greenlet.getcurrent()


class RunCallable:
    def __del__(self):
        results.append(('RunCallable', '__del__'))
        main.switch('from RunCallable')


class G(greenlet.greenlet):
    def __getattribute__(self, name):
        if name == 'run':
            results.append(('G.__getattribute__', 'run'))
            return run_func
        return object.__getattribute__(self, name)


def run_func():
    results.append(('run_func', 'enter'))


g = G(RunCallable())
x = g.switch()
results.append(('main: g.switch()', x))
g.switch()
print('RESULTS', results)

order = []


class B:
    def __init__(self, n):
        self.n = n

    def __del__(self):
        order.append(('del', self.n))


class A:
    def __del__(self):
        order.append('A start')
        b = B('inner')
        del b
        order.append('A end')


first = A()
second = A()
plain = B('plain')
del first, second, plain
order.append('after del')
print(order)

many = []
many.extend(B(i) for i in range(1500))
order.clear()
del many
print(len(order), sorted(order)[0], sorted(order)[-1])

caught = []
sys.unraisablehook = lambda u: caught.append((u.exc_type.__name__, str(u.exc_value), type(u.object).__name__))


class C:
    def __del__(self):
        raise ValueError('boom')


c = C()
del c
print(caught)


class D:
    def __del__(self):
        try:
            raise KeyError('x')
        except KeyError:
            pass


try:
    raise RuntimeError('outer')
except RuntimeError:
    d = D()
    del d
    print(sys.exc_info()[0].__name__)


def make_and_drop():
    local = B('in function')
    return 'returned'


order.clear()
print(make_and_drop(), order)
