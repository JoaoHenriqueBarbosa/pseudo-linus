import greenlet
from greenlet import greenlet as G


class SomeError(Exception):
    pass


class Parent(G):
    def __getattribute__(self, name):
        if name == 'run':
            raise SomeError
        return G.__getattribute__(self, name)


def state(g):
    return (g.dead, bool(g), repr(g).split(' ')[-1], g.parent is None)


parent = Parent()
print('parent before', state(parent))
seen = []
child = G(lambda: seen.append(42), parent)
for i in range(3):
    try:
        r = child.switch()
        print(i, 'returned', r)
    except BaseException as e:
        print(i, type(e).__name__)
    print(' seen', seen, 'child', state(child), 'parent', state(parent))
    print(' child.parent is parent', child.parent is parent)
