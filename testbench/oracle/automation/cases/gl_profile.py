"""sys.setprofile com troca de greenlet: o switch é uma chamada de C (c_call e c_return com o quadro de quem chama) e o
código do greenlet gera call e return de verdade; o que o shim faz por dentro não gera evento."""

import sys

import greenlet


class Profiler:
    def __init__(self):
        self.actions = []

    def __call__(self, frame, event, arg):
        self.actions.append((event, frame.f_code.co_name))

    def __enter__(self):
        self.old = sys.setprofile(self)
        return self.actions

    def __exit__(self, *args):
        sys.setprofile(self.old)


def callback():
    return 42


def run():
    return callback()


class X(greenlet.greenlet):
    def run(self):
        return callback()


def trace_switch(glet):
    with Profiler() as actions:
        glet.switch()
    return actions


print(trace_switch(greenlet.greenlet(run)))
print(trace_switch(X()))
main = greenlet.getcurrent()


def g1_run():
    tracer.__enter__()
    callback()
    g2.switch()
    callback()
    return 42


def g2_run():
    callback()
    tracer.__exit__()
    callback()
    g1.switch()


tracer = Profiler()
g1 = greenlet.greenlet(g1_run)
g2 = greenlet.greenlet(g2_run)
print(g1.switch())
callback()
print(tracer.actions)
