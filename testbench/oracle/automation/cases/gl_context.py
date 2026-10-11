"""gr_context do greenlet: o contexto do contextvars troca a cada switch, um greenlet novo nasce com contexto vazio, o
setter aceita Context ou None, e o greenlet suspenso guarda o contexto dele."""
import contextvars
import re

import greenlet

var = contextvars.ContextVar('var', default='default')
main = greenlet.getcurrent()


def show(name, call):
    try:
        print(name, re.sub(r'0x[0-9a-f]+', '0x?', str(call())))
    except Exception as exc:
        print(name, type(exc).__name__, re.sub(r'0x[0-9a-f]+', '0x?', str(exc)))


def ctx_body():
    print('ctx_in', var.get())
    var.set('child')
    main.switch()
    print('ctx_back', var.get())


var.set('main')
child = greenlet.greenlet(ctx_body)
show('new_context', lambda: child.gr_context)
child.switch()
print('ctx_main', var.get())
print('ctx_gr_context', type(child.gr_context).__name__, child.gr_context.get(var))
child.switch()
show('dead_context', lambda: child.gr_context)

fresh = greenlet.greenlet(ctx_body)
fresh.gr_context = contextvars.Context()
fresh.switch()
print('ctx_main2', var.get())
fresh.switch()

show('main_is_none', lambda: main.gr_context is None)
show('main_type', lambda: type(main.gr_context).__name__)
show('bad_int', lambda: setattr(main, 'gr_context', 1))
show('bad_dict', lambda: setattr(child, 'gr_context', {}))

shared = contextvars.copy_context()


def reader():
    return var.get()


def body_shared():
    print('shared_in', var.get())
    var.set('in_shared')
    main.switch()


g1 = greenlet.greenlet(body_shared)
g1.gr_context = shared
g1.switch()
print('shared_main', var.get())
print('shared_ctx', shared.get(var))
g1.switch()

g2 = greenlet.greenlet(lambda: var.get())
g2.gr_context = None
print('none_ctx', g2.switch())

copy = contextvars.copy_context()
g3 = greenlet.greenlet(lambda: (var.get(), copy.run(var.get)))
g3.gr_context = copy
print('copy_ctx', g3.switch())

token_box = []


def body_token():
    token_box.append(var.set('tok'))
    main.switch()
    var.reset(token_box[0])
    return var.get()


g4 = greenlet.greenlet(body_token)
g4.switch()
show('token_in_main', lambda: var.reset(token_box[0]))
print('token_back', g4.switch())

show('set_on_current', lambda: setattr(main, 'gr_context', contextvars.Context()))
show('current_len', lambda: len(main.gr_context))
show('main_var_after', lambda: var.get())
