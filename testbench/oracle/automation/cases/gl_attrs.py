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
