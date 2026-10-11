"""O `_stack_saved` do greenlet: quanto da pilha de máquina de cada greenlet suspenso o greenlet do C copiou para o
heap, pelo caminho do `run`, pelas nativas no caminho até a troca e pela cópia preguiçosa entre greenlets."""

import functools

import greenlet

main = greenlet.getcurrent()
box = {}


def mk(f, *args, **kwargs):
    g = greenlet.greenlet(f)
    g.switch(*args, **kwargs)
    return g._stack_saved


def mkg(cls, *args, **kwargs):
    g = cls()
    g.switch(*args, **kwargs)
    return g._stack_saved


def plain():
    main.switch()


print('main', main._stack_saved, 'new', greenlet.greenlet(plain)._stack_saved)
g = greenlet.greenlet(lambda: main.switch(main._stack_saved))
print('main_while_g', g.switch(), g._stack_saved)
g.switch()
print('dead', g._stack_saved, g.dead)
print('def', mk(plain), 'lambda', mk(lambda: main.switch()))


class O:
    def m(self):
        main.switch()

    def __call__(self):
        main.switch()


class G(greenlet.greenlet):
    def run(self, *a, **k):
        main.switch()


def f(*a, **k):
    main.switch()


print('bound', mk(O().m), 'callable', mk(O()), 'partial', mk(functools.partial(lambda a: main.switch(), 1)))
print('args', mk(f, 1, 2), 'kwargs', mk(f, x=1), 'subclass', mkg(G), 'subclass_kw', mkg(G, x=1))


class R:
    def __str__(self):
        main.switch()
        return 'r'

    def __format__(self, spec):
        main.switch()
        return 'r'

    def __eq__(self, o):
        main.switch()
        return True

    __hash__ = object.__hash__

    def __getitem__(self, k):
        main.switch()
        return 1

    def __iter__(self):
        main.switch()
        return iter([1])

    def __len__(self):
        main.switch()
        return 1

    def __add__(self, o):
        main.switch()
        return 1

    def __lt__(self, o):
        main.switch()
        return True


class S:
    def __str__(self):
        main.switch()
        return 's'


def gen():
    main.switch()
    yield 1


def outer():
    yield from gen()


def forgen():
    for _ in gen():
        pass


print('pct', mk(lambda: '%s' % S()), 'str', mk(lambda: str(R())), 'format', mk(lambda: format(R())))
print('format_str', mk(lambda: format(S())), 'fstr', mk(lambda: f'{R()}'), 'fstr_str', mk(lambda: f'{S()}'))
print('eq', mk(lambda: R() == 1), 'in', mk(lambda: R() in [1]), 'getitem', mk(lambda: R()[0]))
print('iter', mk(lambda: [x for x in R()]), 'len', mk(lambda: len(R())), 'add', mk(lambda: R() + 1),
      'lt', mk(lambda: R() < 1))
print('sorted', mk(lambda: sorted([2, 1], key=lambda x: main.switch() or x)),
      'min', mk(lambda: min([2, 1], key=lambda x: main.switch() or x)),
      'map', mk(lambda: list(map(lambda x: main.switch(), [1]))),
      'filter', mk(lambda: list(filter(lambda x: main.switch(), [1]))))
print('gen_next', mk(lambda: next(gen())), 'gen_for', mk(forgen), 'list_yf', mk(lambda: list(outer())),
      'tuple', mk(lambda: tuple(gen())), 'sum', mk(lambda: sum(gen())))
print('nested', mk(lambda: sorted([1], key=lambda x: '%s' % S())),
      'sorted2', mk(lambda: sorted([2, 1], key=lambda x: sorted([1], key=lambda y: main.switch() or y))))


def inner():
    box['a_while_t'] = a._stack_saved
    main.switch()


a = greenlet.greenlet(lambda: greenlet.greenlet(inner).switch())
a.switch()
print('a_while_t', box['a_while_t'], 'a_after', a._stack_saved)

d = greenlet.greenlet(lambda: (main.switch(), box.__setitem__('m', main._stack_saved), main.switch()))
d.switch()
d.switch()
print('main_to_started', box['m'])
b = greenlet.greenlet(lambda: (main.switch(), box.__setitem__('m', main._stack_saved), main.switch()))
b.switch()


def k(x):
    if x == 1:
        b.switch()
    return 0


sorted([1, 2], key=k)
print('main_sorted_to_started', box['m'])
x = greenlet.greenlet(lambda: main.switch())
y = greenlet.greenlet(lambda: (x.switch(), main.switch()))
x.switch()
print('x_before', x._stack_saved)
y.switch()
print('y_to_x', x._stack_saved, x.dead, y._stack_saved)
