"""pdb mínimo: o sandbox não tem depurador interativo (`sys.settrace`). Existe para `doctest` e afins
importarem; `set_trace()` não faz nada além de avisar."""
import sys

__all__ = ['Pdb', 'set_trace', 'post_mortem', 'pm', 'run', 'runcall', 'runeval', 'Restart']


class Restart(Exception):
    pass


class Pdb:
    def __init__(self, completekey='tab', stdin=None, stdout=None, skip=None, nosigint=False, readrc=True):
        self.stdin = stdin
        self.stdout = stdout or sys.stdout

    def reset(self):
        pass

    def set_trace(self, frame=None):
        print('pdb: depuração interativa indisponível neste sandbox', file=self.stdout)

    def trace_dispatch(self, frame, event, arg):
        return None

    def set_continue(self):
        pass

    def set_quit(self):
        pass


def set_trace(*, header=None):
    Pdb().set_trace()


def post_mortem(t=None):
    pass


def pm():
    pass


def run(statement, globals=None, locals=None):
    exec(statement, globals if globals is not None else {}, locals)


def runeval(expression, globals=None, locals=None):
    return eval(expression, globals if globals is not None else {}, locals)


def runcall(func, *args, **kwds):
    return func(*args, **kwds)
