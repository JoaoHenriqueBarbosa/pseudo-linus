"""list.sort esvazia a lista enquanto ordena (a chave e o __lt__ enxergam uma lista vazia), devolve os itens ordenados
no fim, restaura a lista original quando a chave ou a comparação levantam, e acusa a modificação durante a ordenação."""

seen = []


def key_peek(x):
    seen.append((len(lst), list(lst)))
    return x


lst = [3, 1, 2]
lst.sort(key=key_peek)
print('key_peek', lst, seen)

seen.clear()


class Item:
    def __init__(self, v):
        self.v = v

    def __lt__(self, other):
        seen.append(len(lst))
        return self.v < other.v

    def __repr__(self):
        return 'I%d' % self.v


lst = [Item(3), Item(1), Item(2)]
lst.sort()
print('lt_peek', lst, seen)


def key_raise(x):
    if x == 2:
        raise KeyError(x)
    return x


lst = [3, 1, 2, 0]
try:
    lst.sort(key=key_raise)
except KeyError as e:
    print('key_raise', repr(e), lst)


class Boom:
    def __lt__(self, other):
        raise RuntimeError('boom')


lst = [Boom(), Boom()]
orig = list(lst)
try:
    lst.sort()
except RuntimeError as e:
    print('lt_raise', e, lst == orig, len(lst))


def key_append(x):
    lst.append(99)
    return x


lst = [3, 1, 2]
try:
    lst.sort(key=key_append)
except ValueError as e:
    print('key_append', e, lst)


def key_clear(x):
    lst.clear()
    return x


lst = [3, 1, 2]
try:
    lst.sort(key=key_clear)
except ValueError as e:
    print('key_clear', e, lst)


def key_extend_item(x):
    lst.extend([7, 8])
    return x


lst = [2, 1]
try:
    lst.sort(key=key_extend_item)
except ValueError as e:
    print('key_extend', e, lst)

lst = [3, 1, 2]
print('sorted_keeps', sorted(lst, key=lambda x: (len(lst), x)), lst)

lst = [3, 1, 2]
lst.sort(key=lambda x: -x, reverse=True)
print('reverse_key', lst)

lst = []
lst.sort(key=key_peek)
print('empty', lst)

lst = [1]
seen.clear()
lst.sort(key=key_peek)
print('one', lst, seen)


def key_len_item(x):
    return len(lst)


lst = [5, 4]
lst.sort(key=key_len_item)
print('key_len', lst)
