"""UnswitchableGreenlet: o slp_switch que falha de propósito (erro de SystemError na primeira troca e na troca para um
greenlet rodando, e o abort fatal do slp_switch), como nos testes do greenlet 3.5.6."""

import greenlet

U = greenlet._greenlet.UnswitchableGreenlet


def t(label, f):
    try:
        print(label, repr(f()))
    except BaseException as e:
        print(label, 'EXC', type(e).__name__, e)


g = U(lambda: 1)
g.force_switch_error = True
t('first', g.switch)
g.force_switch_error = False
t('after', g.switch)
runs = []


def func():
    runs.append(1)
    greenlet.getcurrent().parent.switch()
    runs.append(2)
    greenlet.getcurrent().parent.switch()
    runs.append(3)


h = U(func)
h.switch()
h.switch()
h.force_switch_error = True
t('running', h.switch)
h.force_switch_error = False
h.switch()
print(runs)
