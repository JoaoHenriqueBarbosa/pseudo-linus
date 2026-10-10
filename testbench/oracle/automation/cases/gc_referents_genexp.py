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

print('genexp_dict', names((k for k in {'a': 1})))
print('genexp_dict_items', names((k for k in {'a': 1}.items())))
print('genexp_set', names((k for k in {1})))
print('genexp_bytes', names((k for k in b'ab')))
print('genexp_bytearray', names((k for k in bytearray(b'ab'))))
print('genexp_str', names((k for k in 'ab')))


class Seq:
    calls = 0

    def __iter__(self):
        Seq.calls += 1
        return iter([1, 2])


g = (x for x in Seq())
print('genexp_instance', Seq.calls, names(g), list(g), Seq.calls)


class Bad:
    def __iter__(self):
        return 3


try:
    (x for x in Bad())
except TypeError as e:
    print('genexp_bad', e)


class It:
    def __init__(self):
        self.n = 0

    def __iter__(self):
        print('It.__iter__')
        return self

    def __next__(self):
        self.n += 1
        if self.n > 2:
            raise StopIteration
        return self.n


g = (x for x in It())
print('genexp_it', [type(r).__name__ for r in gc.get_referents(g)][-1])
print(list(g))
