"""_thread mínimo: o interpretador roda um fluxo só (as threads de `threading` rodam uma de cada vez, aninhadas),
então os travões nunca bloqueiam de verdade."""

error = RuntimeError

# Pilha de identificadores: o da thread que está rodando é o último (`threading` empilha ao rodar uma thread).
_idents = [1]


def get_ident():
    return _idents[-1]


class LockType:
    def __init__(self):
        self._locked = False

    def acquire(self, blocking=True, timeout=-1):
        if self._locked and blocking and timeout < 0:
            raise RuntimeError('deadlock: lock already held by the only thread')
        if self._locked:
            return False
        self._locked = True
        return True

    def release(self):
        if not self._locked:
            raise RuntimeError('release unlocked lock')
        self._locked = False

    def locked(self):
        return self._locked

    def __enter__(self):
        self.acquire()
        return True

    def __exit__(self, *exc):
        self.release()


def allocate_lock():
    return LockType()
