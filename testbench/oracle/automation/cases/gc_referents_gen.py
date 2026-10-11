"""gc.get_referents de geradores e iteradores como o tp_traverse do CPython 3.13: o quadro (função, código, locais, pilha),
o quadro limpo, os iteradores com a origem (e sem ela depois de esgotados) e as funções de módulo."""

import gc


def names(*objs):
    return [type(r).__name__ for r in gc.get_referents(*objs)]


def g0():
    yield 1


def g1(a, b=2):
    x = [a]
    y = yield x
    z = (y, b)
    yield z


def g2(a):
    yield from g0()


def closure_gen(n):
    def inner():
        yield n
    return inner()


gen = g1(5)
print('unstarted_args', names(gen))
print('function_identity', gc.get_referents(gen)[2] is g1)
next(gen)
print('started', names(gen))
gen.send(7)
print('second', names(gen))
gen2 = g2(1)
next(gen2)
print('yield_from', names(gen2))
done = g0()
list(done)
print('exhausted', names(done))
print('closed', (lambda c: (c.close(), names(c))[1])(g1(1)))
started_closed = g1(1)
next(started_closed)
started_closed.close()
print('started_closed', names(started_closed))
cl = closure_gen(3)
print('closure_unstarted', names(cl))
next(cl)
print('closure_started', names(cl))
print('len', names(len))
print('print', names(print))
print('isinstance', names(isinstance))
print('sorted', names(sorted))
print('int_type', names(int))
li = [1, 2, 3]
it = iter(li)
print('list_iter_identity', gc.get_referents(it)[0] is li)
next(it)
print('list_iter_adv', names(it))
print('list_iter_done', names((lambda it: (list(it), it)[1])(iter([1]))))
print('tuple_iter', names(iter((1, 2))))
print('str_iter', names(iter('ab')))
print('range_iter', names(iter(range(3))))
print('dict_iter', names(iter({1: 2})))
print('set_iter', names(iter({1})))
print('filter', names(filter(None, [1])))
print('enumerate_adv', names((lambda e: (next(e), e)[1])(enumerate([1, 2]))))
print('zip_adv', names((lambda e: (next(e), e)[1])(zip([1, 2], [3, 4]))))
print('zip_empty', names(zip()))
print('map_two', names(map(lambda a, b: a, [1], [2])))
print('map_adv', names((lambda e: (next(e), e)[1])(map(str, [1, 2]))))
print('callable_iter', names(iter(lambda: 1, 2)))
