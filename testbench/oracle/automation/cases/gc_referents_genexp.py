"""gc.get_referents de uma expressão geradora: o `.0` é o iterador que quem a criou já obteve (`GET_ITER` fora), e o
compilador do CPython troca a lista literal de constantes por tupla no `for`."""

import gc


def names(*objs):
    return [type(r).__name__ for r in gc.get_referents(*objs)]


print('genexp_range', names((i for i in range(3))))
cg = (i for i in [1, 2])
print('genexp_unstarted', names(cg))
next(cg)
print('genexp_started', names(cg))
