import gc, sys

gc.collect()


class Holder:
    pass


def exc_in_local():
    try:
        raise ValueError('x')
    except ValueError as e:
        saved = e
    return 1


def exc_in_instance():
    h = Holder()
    try:
        raise ValueError('y')
    except ValueError as e:
        h.err = e
    return h


def exc_chain():
    try:
        try:
            raise KeyError('inner')
        except KeyError as inner:
            raise RuntimeError('outer') from inner
    except RuntimeError as outer:
        keep = outer
    return 1


def exc_returned():
    try:
        raise ValueError('z')
    except ValueError as e:
        return e


def abandoned_gen():
    def g():
        me = yield
        yield me
    x = g()
    next(x)
    x.send(x)


def abandoned_gen_plain():
    def g():
        yield 1
        yield 2
    x = g()
    next(x)


def coro_cycle():
    async def c():
        me = None
        return me
    x = c()
    return 1


def counts():
    return len(gc.get_objects())


for name, fn in [('exc_in_local', exc_in_local), ('exc_in_instance', exc_in_instance), ('exc_chain', exc_chain),
                 ('abandoned_gen', abandoned_gen), ('abandoned_gen_plain', abandoned_gen_plain)]:
    gc.collect()
    r = fn()
    del r
    print(name, gc.collect(), gc.collect())

# exception returned and dropped: no cycle (the traceback holds the frame, the frame does not hold the exception)
e = exc_returned()
del e
print('returned', gc.collect())

# the exception stays alive while referenced, with a usable traceback
try:
    raise ValueError('live')
except ValueError as ex:
    kept = ex
gc.collect()
print(kept.__traceback__.tb_frame.f_code.co_name, kept.args)
del kept

# memory: loops that create these cycles stay bounded with automatic collection
for name, fn in [('exc_in_local', exc_in_local), ('exc_in_instance', exc_in_instance), ('exc_chain', exc_chain),
                 ('abandoned_gen', abandoned_gen)]:
    gc.collect()
    before = counts()
    for _ in range(20000):
        fn()
    mid = counts()
    gc.collect()
    after = counts()
    print(name, 'bounded', mid - before < 20000, 'after collect', after - before < 200)
