"""Atributos do greenlet que a suíte do 3.5.6 exercita: `run=None` explícito, o `__init__` do tipo chamado por um
`__init__` de subclasse, o `__dict__` (apagar e trocar), o `gr_context` que não se apaga."""

import greenlet

G = greenlet.greenlet


def t(label, f):
    try:
        print(label, repr(f()))
    except BaseException as e:
        print(label, 'EXC', type(e).__name__, e)


class A:
    pass


a = A()


def del_a_dict():
    del a.__dict__


def set_a_dict():
    a.__dict__ = 42


t('A del dict', del_a_dict)
t('A set dict', set_a_dict)
g = G()
t('fresh run', lambda: g.run)


def del_run():
    del g.run


t('del run fresh', del_run)
g2 = G(lambda: 1)


def del_run_set():
    del g2.run


t('del run set', del_run_set)
t('after del', lambda: g2.run)
t('run=None switch', lambda: G(run=None).switch())
g3 = G(run=None)
t('g3.run', lambda: g3.run)
g3.run = 5
t('g3.run=5', lambda: g3.run)
t('dict', lambda: G(run=None).__dict__)
g4 = G(lambda: None)
g4.x = 1
t('dict x', lambda: g4.__dict__)


def del_dict():
    del g4.__dict__


def set_dict():
    g4.__dict__ = 42


t('del dict', del_dict)
t('set dict', set_dict)
def del_context():
    del g4.gr_context


t('ctx del', del_context)


class S(G):
    def __init__(self):
        G.__init__(self, run=self.run)

    def run(self):
        return 7


t('sub type', lambda: type(S()).__name__)
t('sub switch', lambda: S().switch())


import contextvars
import threading

var = contextvars.ContextVar('var')
print('get default no ctx', contextvars.ContextVar('d', default=3).get(), greenlet.getcurrent().gr_context)
cg = G(lambda: (greenlet.getcurrent().gr_context, var.get(None)))
print('new greenlet ctx', cg.switch())

running = threading.Event()
release = threading.Event()
holder = []


def in_thread():
    gr = G(lambda: (running.set(), release.wait(10)))
    holder.append(gr)
    gr.switch()


th = threading.Thread(target=in_thread)
th.start()
running.wait(10)
other = holder[0]
t('ctx get other thread', lambda: other.gr_context)


def set_ctx():
    other.gr_context = None


t('ctx set other thread', set_ctx)
release.set()
th.join()

dead = G(lambda: 1)
dead.switch()
t('dead throw', lambda: type(dead.throw()).__name__)
t('dead throw twice', lambda: type(dead.throw()).__name__)
t('self throw exit', lambda: type(greenlet.getcurrent().throw()).__name__)
t('throw bad tb', lambda: greenlet.getcurrent().throw(Exception, Exception(), object()))


def _reparent_cases():
    import time
    another = []

    def worker():
        g = G(lambda: None)
        another.append(g)
        g.switch()

    th = threading.Thread(target=worker)
    th.start()
    th.join(10)
    time.sleep(0.2)

    def direct():
        g = G(lambda: None)
        g.parent = another[0]
        return 'set'

    t('reparent to dead of exited thread', direct)

    class convoluted(G):
        def __getattribute__(self, name):
            if name == 'run':
                self.parent = another[0]
            return G.__getattribute__(self, name)

    t('switch after reparent, thread exited', lambda: convoluted(lambda: None).switch())
    del another[:]
    another2 = []
    alive = threading.Event()
    stop = threading.Event()

    def worker2():
        g = G(lambda: None)
        another2.append(g)
        g.switch()
        alive.set()
        stop.wait(10)

    th2 = threading.Thread(target=worker2)
    th2.start()
    alive.wait(10)
    another[:] = another2
    out = []
    try:
        convoluted(lambda: None).switch()
    except greenlet.error as e:
        out.append(str(e).split('\n')[0])
        out.append('Current:' in str(e) and 'Expected:' in str(e))
    print('switch after reparent, thread alive', out)
    stop.set()
    th2.join(10)
    del another[:]


_reparent_cases()


def _parent_restored():
    hub = G(lambda: None)
    main = greenlet.getcurrent()
    result = []

    def worker():
        try:
            main.switch()
        except greenlet.GreenletExit:
            result.append(greenlet.getcurrent().parent is main)
            result.append(greenlet.getcurrent())
            hub.switch()

    g = G(worker, parent=hub)
    g.switch()
    del g
    print('parent during kill is main', result[0], 'restored to hub', result[1].parent is hub)
    del result[:]


_parent_restored()
