"""`_multiprocessing`: o `SemLock` que o `multiprocessing.synchronize` do CPython usa para `Lock`, `RLock`,
`Semaphore`, `BoundedSemaphore`, `Event` e `Condition`, e o que as filas e o `Pool` do `fork` pedem.

No Linux o CPython guarda o `sem_t` numa página compartilhada (`sem_open` seguido de `sem_unlink`, que o `fork`
herda) e espera com `sem_wait`. O sandbox não tem semáforo do kernel; o valor vive numa caixa de correio: um
`pipe` que sempre guarda uma única mensagem de 8 bytes. Ler a mensagem toma o valor (quem vier depois bloqueia
no `read`, que é exatamente a exclusão mútua da seção crítica), e escrever a devolve. O pipe é herdado pelo
filho do `fork`, então todos os processos que descendem de quem criou o lock enxergam o mesmo contador."""

import os
import struct
import time

import _thread

RECURSIVE_MUTEX = 0
SEMAPHORE = 1

# O `SEM_VALUE_MAX` do glibc (`INT_MAX`).
SEM_VALUE_MAX = 2147483647

flags = {'HAVE_SEM_OPEN': 1, 'HAVE_SEM_TIMEDWAIT': 1}

_VALUE = struct.Struct('<q')

# A espera por um valor positivo volta a olhar a caixa de correio em intervalos que crescem até este teto.
_FIRST_POLL = 0.0002
_MAX_POLL = 0.01


class SemLock:
    """Semaphore/mutex shared by the processes that inherit it."""

    SEM_VALUE_MAX = SEM_VALUE_MAX

    def __init__(self, kind, value, maxvalue, name, unlink):
        if kind not in (RECURSIVE_MUTEX, SEMAPHORE):
            raise ValueError('unrecognized kind')
        self._reader, self._writer = os.pipe()
        os.write(self._writer, _VALUE.pack(value))
        self._kind = kind
        self._maxvalue = maxvalue
        # Com `unlink` (o contexto `fork`) o nome já não existe no sistema: o objeto fica sem nome.
        self._name = None if unlink else name
        self._count_value = 0
        self._last_tid = 0

    @property
    def handle(self):
        return self._reader

    @property
    def kind(self):
        return self._kind

    @property
    def maxvalue(self):
        return self._maxvalue

    @property
    def name(self):
        return self._name

    def __del__(self):
        # O `sem_close` do `SemLock_dealloc`: só o descritor deste processo fecha, os dos outros seguem.
        for fd in (getattr(self, '_reader', -1), getattr(self, '_writer', -1)):
            if fd >= 0:
                try:
                    os.close(fd)
                except OSError:
                    pass

    def _take(self):
        """Toma o valor da caixa de correio (bloqueia enquanto outro o tem em mãos)."""
        return _VALUE.unpack(os.read(self._reader, 8))[0]

    def _put(self, value):
        os.write(self._writer, _VALUE.pack(value))

    def _is_mine(self):
        return self._count_value > 0 and _thread.get_ident() == self._last_tid

    def acquire(self, block=True, timeout=None):
        """Acquire the semaphore/lock."""
        if self._kind == RECURSIVE_MUTEX and self._is_mine():
            self._count_value += 1
            return True
        deadline = None
        if block and timeout is not None:
            deadline = time.monotonic() + max(timeout, 0)
        poll = _FIRST_POLL
        while True:
            value = self._take()
            if value > 0:
                self._put(value - 1)
                break
            self._put(value)
            if not block:
                return False
            wait = poll
            if deadline is not None:
                left = deadline - time.monotonic()
                if left <= 0:
                    return False
                wait = min(poll, left)
            time.sleep(wait)
            poll = min(poll * 2, _MAX_POLL)
        self._count_value += 1
        self._last_tid = _thread.get_ident()
        return True

    def release(self):
        """Release the semaphore/lock."""
        if self._kind == RECURSIVE_MUTEX:
            if not self._is_mine():
                raise AssertionError('attempt to release recursive lock not owned by thread')
            if self._count_value > 1:
                self._count_value -= 1
                return
        value = self._take()
        if value >= self._maxvalue:
            self._put(value)
            raise ValueError('semaphore or lock released too many times')
        self._put(value + 1)
        self._count_value -= 1

    def __enter__(self):
        """Enter the semaphore/lock."""
        return self.acquire()

    def __exit__(self, *args):
        """Exit the semaphore/lock."""
        return self.release()

    def _count(self):
        """Num of `acquire()`s minus num of `release()`s for this process."""
        return self._count_value

    def _is_zero(self):
        """Return whether semaphore has value zero."""
        return self._get_value() == 0

    def _get_value(self):
        """Get the value of the semaphore."""
        value = self._take()
        self._put(value)
        return value

    def _after_fork(self):
        """Rezero the net acquisition count after fork()."""
        self._count_value = 0

    @classmethod
    def _rebuild(cls, handle, kind, maxvalue, name):
        # O `sem_open(name)` de um semáforo nomeado: sem semáforos do kernel não há nome a abrir.
        raise FileNotFoundError(2, 'No such file or directory')


def sem_unlink(name):
    raise FileNotFoundError(2, 'No such file or directory')
