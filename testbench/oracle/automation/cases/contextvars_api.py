"""contextvars como o _contextvars do CPython: Context como mapeamento, copy_context, Context.run, ContextVar.set e
reset com Token, um contexto por thread e por tarefa do asyncio, e a superfície observável dos tipos estáticos."""

import asyncio
import contextvars
import pickle
import re
import threading

import _contextvars


def clean(text):
    return re.sub(r'0x[0-9a-f]+', '0x?', str(text))


def show(name, call):
    try:
        print(name, clean(call()))
    except Exception as exc:
        print(name, type(exc).__name__, clean(exc))


v = contextvars.ContextVar('v', default=1)
w = contextvars.ContextVar('w')

show('mod_file', lambda: _contextvars.__file__)
show('mod_doc', lambda: _contextvars.__doc__)
show('mod_dir', lambda: dir(_contextvars))
show('cv_all', lambda: contextvars.__all__)
show('cv_dir', lambda: [n for n in dir(contextvars) if not n.startswith('__')])
show('types', lambda: [t.__module__ + '.' + t.__qualname__ for t in (contextvars.Context, contextvars.ContextVar,
                                                                      contextvars.Token)])
show('same', lambda: contextvars.Context is _contextvars.Context)
show('repr_v', lambda: repr(v))
show('repr_w', lambda: repr(w))
show('name', lambda: v.name)
show('get_default', lambda: v.get())
show('get_missing', lambda: w.get())
show('get_arg', lambda: w.get(5))
t1 = v.set(10)
show('after_set', lambda: v.get())
show('token_repr', lambda: repr(t1))
show('token_var', lambda: t1.var is v)
show('token_old', lambda: t1.old_value is contextvars.Token.MISSING)
show('missing_repr', lambda: repr(contextvars.Token.MISSING))
show('missing_type', lambda: type(contextvars.Token.MISSING))
t2 = v.set(20)
show('old_value', lambda: t2.old_value)
show('reset', lambda: v.reset(t2))
show('after_reset', lambda: v.get())
show('reset_again', lambda: v.reset(t2))
show('used_repr', lambda: repr(t2))
show('reset_wrong_var', lambda: w.reset(t1))
show('reset_bad', lambda: v.reset(5))

ctx = contextvars.copy_context()
show('ctx_type', lambda: type(ctx))
show('ctx_len', lambda: len(ctx))
show('ctx_get', lambda: ctx[v])
show('ctx_in', lambda: (v in ctx, w in ctx))
show('ctx_keys', lambda: list(ctx.keys()) == [v])
show('ctx_values', lambda: list(ctx.values()))
show('ctx_items', lambda: [(k.name, val) for k, val in ctx.items()])
show('ctx_iter', lambda: [k.name for k in ctx])
show('ctx_getm', lambda: ctx.get(w, 'dflt'))
show('ctx_missing_key', lambda: ctx[w])
show('ctx_bad_key', lambda: ctx['x'])
show('ctx_bad_in', lambda: 'x' in ctx)
show('ctx_hash', lambda: hash(ctx))
show('ctx_eq', lambda: ctx == ctx.copy())


def inner(arg):
    w.set(arg)
    return (v.get(), w.get(), len(contextvars.copy_context()))


show('run', lambda: ctx.run(inner, 7))
show('run_isolated', lambda: w.get('outside'))
show('run_ctx_sees', lambda: ctx[w])
show('run_twice', lambda: ctx.run(ctx.run, inner, 1))
show('new_ctx', lambda: len(contextvars.Context()))
show('new_ctx_args', lambda: contextvars.Context(1))
show('cv_noname', lambda: contextvars.ContextVar())
show('cv_badname', lambda: contextvars.ContextVar(5))
show('token_new', lambda: contextvars.Token())


def subclass(base):
    class Sub(base):
        pass
    return Sub


show('sub_ctx', lambda: subclass(contextvars.Context))
show('sub_cv', lambda: subclass(contextvars.ContextVar))
show('sub_token', lambda: subclass(contextvars.Token))
show('generic', lambda: contextvars.ContextVar[int])
show('pickle_cv', lambda: pickle.dumps(v))
show('pickle_ctx', lambda: pickle.dumps(ctx))


def set_name():
    v.name = 'x'


show('readonly', set_name)

seen = []


def worker():
    seen.append((v.get(), w.get('none'), len(contextvars.copy_context())))


w.set('main')
thread = threading.Thread(target=worker)
thread.start()
thread.join()
show('thread', lambda: seen)


async def task(n):
    w.set(n)
    await asyncio.sleep(0)
    return w.get()


async def gather():
    w.set('outer')
    results = await asyncio.gather(task(1), task(2))
    return results, w.get()


show('asyncio', lambda: asyncio.run(gather()))
show('after_asyncio', lambda: w.get())
