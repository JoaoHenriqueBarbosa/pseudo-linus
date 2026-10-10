"""O __hash__ de usuário como o CPython o usa: a exceção dele sai de hash(), da inserção e da busca em dict e set e
do hash de tupla; o resultado que não é int é TypeError; int grande e bool são reduzidos como o int; -1 vira -2;
subclasse de int com __hash__ próprio; __hash__ = None."""


class Boom:
    def __hash__(self):
        raise KeyError('boom')


class Ret:
    def __init__(self, value):
        self.value = value

    def __hash__(self):
        return self.value


class MyInt(int):
    def __hash__(self):
        return 7


class NoHash:
    __hash__ = None


def show(name, call):
    try:
        print(name, call())
    except Exception as exc:
        print(name, type(exc).__name__, exc)


show('hash', lambda: hash(Boom()))
show('dict_set', lambda: {Boom(): 1})
show('dict_get', lambda: {1: 2}.get(Boom()))
show('dict_in', lambda: Boom() in {1: 2})
show('set_add', lambda: {Boom()})
show('set_in', lambda: Boom() in {1})
show('tuple', lambda: hash((1, Boom())))
show('frozenset', lambda: frozenset([Boom()]))
show('str_ret', lambda: hash(Ret('x')))
show('float_ret', lambda: hash(Ret(1.5)))
show('none_ret', lambda: hash(Ret(None)))
show('big_ret', lambda: hash(Ret(2 ** 64 + 5)))
show('bool_ret', lambda: hash(Ret(True)))
show('minus_one', lambda: hash(Ret(-1)))
show('myint', lambda: hash(MyInt(3)))
show('nohash', lambda: hash(NoHash()))
