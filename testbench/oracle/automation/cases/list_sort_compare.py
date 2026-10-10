"""list.sort compara na mesma ordem e com os mesmos pares do CPython (powersort, inserção binária, galope), e, quando a
comparação levanta no meio, deixa a lista na mesma permutação parcial."""

import random


class Item:
    log = []

    def __init__(self, v):
        self.v = v

    def __lt__(self, other):
        Item.log.append((self.v, other.v))
        return self.v < other.v


def run(values):
    Item.log.clear()
    lst = [Item(v) for v in values]
    lst.sort()
    return [i.v for i in lst], list(Item.log)


for values in ([3, 1, 2], [1, 2, 3], [3, 2, 1], [1, 1, 1], [2, 1, 3, 0], [1, 2, 2, 1], [5, 4, 4, 3], [1, 3, 2, 4, 6, 5], [2, 2, 1]):
    out, log = run(values)
    print(values, out, len(log), log)

random.seed(7)
for n in (10, 40, 64, 65, 100, 300, 1000):
    values = [random.randrange(n) for _ in range(n)]
    out, log = run(values)
    print(n, len(log), out == sorted(values), hash(tuple(log)) & 0xffffffff)

# Entradas com corridas naturais: crescente, decrescente, iguais, serra, blocos.
for label, values in (
    ('asc', list(range(200))),
    ('desc', list(range(200, 0, -1))),
    ('equal', [5] * 100),
    ('saw', [i % 17 for i in range(300)]),
    ('blocks', [(i // 20) * 3 - (i % 20) for i in range(400)]),
    ('organ', list(range(100)) + list(range(100, 0, -1))),
):
    out, log = run(values)
    print(label, len(log), out == sorted(values), hash(tuple(log)) & 0xffffffff)


class Boom(Item):
    limit = 0

    def __lt__(self, other):
        Boom.limit -= 1
        if Boom.limit < 0:
            raise RuntimeError('boom')
        return self.v < other.v


for n, limit in ((20, 15), (40, 70), (150, 400), (150, 900), (500, 2500)):
    random.seed(n)
    values = [random.randrange(n) for _ in range(n)]
    lst = [Boom(v) for v in values]
    Boom.limit = limit
    try:
        lst.sort()
        print('boom_ok', n, limit)
    except RuntimeError:
        print('boom', n, limit, [i.v for i in lst])

random.seed(3)
values = [random.randrange(50) for _ in range(120)]
lst = [Item(v) for v in values]
Item.log.clear()
lst.sort(key=lambda i: i.v // 3, reverse=True)
print('reverse_key', [i.v for i in lst], len(Item.log))
