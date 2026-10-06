"""threading cooperativo: o interpretador tem um único fluxo de execução, então as threads não rodam em
paralelo. `start()` só enfileira a thread; quem bloqueia (`join`, `Event.wait`, `Condition.wait`, uma fila vazia,
`time.sleep` na thread principal) roda as threads pendentes, na ordem em que foram iniciadas, até o que esperava
poder seguir. Uma thread que bloqueia sem ninguém para destravá-la fica estacionada para sempre (como uma thread
que espera uma fila que nunca mais recebe nada); na thread principal isso vira `RuntimeError` em vez de travar."""

import _thread
import time as _time

__all__ = ['Thread', 'Lock', 'RLock', 'Event', 'Condition', 'Semaphore', 'BoundedSemaphore', 'Barrier', 'Timer',
           'local', 'current_thread', 'main_thread', 'active_count', 'enumerate', 'get_ident', 'get_native_id',
           'excepthook', 'ThreadError', 'TIMEOUT_MAX', 'settrace', 'setprofile', 'stack_size',
           'BrokenBarrierError']

TIMEOUT_MAX = 9223372036.0
ThreadError = RuntimeError
get_ident = _thread.get_ident
get_native_id = _thread.get_ident
Lock = _thread.allocate_lock
_counter = [0]
_threads = []
_pending = []


class BrokenBarrierError(RuntimeError):
    pass


class _Parked(BaseException):
    """Desenrola a pilha de uma thread que bloqueou sem ninguém para destravá-la (ou que passou do prazo
    do `sleep` da principal: `sliced`, e então a thread conta como encerrada)."""

    def __init__(self, sliced=False):
        self.sliced = sliced


def settrace(func):
    pass


def setprofile(func):
    pass


def stack_size(size=0):
    return 0


def _run_one():
    """Roda a thread pendente mais antiga até acabar (ou estacionar). `False` se não havia nenhuma."""
    if not _pending:
        return False
    _pending.pop(0)._bootstrap()
    return True


def _run_all():
    while _run_one():
        pass


def _no_progress(what):
    """Bloqueio que nenhuma thread pendente pode resolver."""
    if _state['current'] is _main:
        raise RuntimeError('deadlock: %s sem outra thread para destravar' % what)
    raise _Parked()


def _wait_for(cond, timeout, what):
    """Roda threads pendentes até `cond()` ficar verdadeira. Com `timeout`, dorme o que faltar e devolve `cond()`."""
    deadline = None if timeout is None else _time.monotonic() + max(timeout, 0)
    while not cond():
        if _run_one():
            continue
        if deadline is None:
            _no_progress(what)
        left = deadline - _time.monotonic()
        if left > 0:
            _time.sleep(left)
        return cond()
    return True


class RLock:
    def __init__(self):
        self._count = 0

    def acquire(self, blocking=True, timeout=-1):
        self._count += 1
        return True

    __enter__ = acquire

    def release(self):
        if self._count == 0:
            raise RuntimeError('cannot release un-acquired lock')
        self._count -= 1

    def __exit__(self, *exc):
        self.release()

    def _is_owned(self):
        return self._count > 0

    def _release_save(self):
        count, self._count = self._count, 0
        return count

    def _acquire_restore(self, saved):
        self._count = saved

    def __repr__(self):
        return '<%s.RLock object owner=%s count=%d>' % (self.__class__.__module__, 0, self._count)


class Condition:
    def __init__(self, lock=None):
        self._lock = lock if lock is not None else RLock()
        self.acquire = self._lock.acquire
        self.release = self._lock.release
        self._notified = 0

    def __enter__(self):
        return self._lock.__enter__()

    def __exit__(self, *args):
        return self._lock.__exit__(*args)

    def _release_save(self):
        if hasattr(self._lock, '_release_save'):
            return self._lock._release_save()
        self._lock.release()
        return None

    def _acquire_restore(self, saved):
        if hasattr(self._lock, '_acquire_restore'):
            self._lock._acquire_restore(saved)
        else:
            self._lock.acquire()

    def wait(self, timeout=None):
        # Solta o lock, deixa uma thread pendente rodar (é ela que vai notificar) e retoma: o retorno pode ser
        # "espúrio", e quem usa `Condition` já confere a condição em laço.
        saved = self._release_save()
        try:
            if _run_one():
                return True
            if timeout is not None:
                _time.sleep(timeout)
                return False
            _no_progress('Condition.wait()')
        finally:
            self._acquire_restore(saved)

    def wait_for(self, predicate, timeout=None):
        endtime = None
        waittime = timeout
        result = predicate()
        while not result:
            if waittime is not None:
                if endtime is None:
                    endtime = _time.monotonic() + waittime
                else:
                    waittime = endtime - _time.monotonic()
                    if waittime <= 0:
                        break
            self.wait(waittime)
            result = predicate()
        return result

    def notify(self, n=1):
        pass

    def notify_all(self):
        pass

    notifyAll = notify_all


class Semaphore:
    def __init__(self, value=1):
        if value < 0:
            raise ValueError('semaphore initial value must be >= 0')
        self._value = value

    def acquire(self, blocking=True, timeout=None):
        if not blocking and timeout is not None:
            raise ValueError("can't specify timeout for non-blocking acquire")
        if self._value <= 0:
            if not blocking:
                return False
            if not _wait_for(lambda: self._value > 0, timeout, 'semáforo esgotado'):
                return False
        self._value -= 1
        return True

    __enter__ = acquire

    def release(self, n=1):
        if n < 1:
            raise ValueError('n must be one or more')
        self._value += n

    def __exit__(self, t, v, tb):
        self.release()


class BoundedSemaphore(Semaphore):
    def __init__(self, value=1):
        super().__init__(value)
        self._initial_value = value

    def release(self, n=1):
        if self._value + n > self._initial_value:
            raise ValueError('Semaphore released too many times')
        super().release(n)


class Event:
    def __init__(self):
        self._flag = False

    def is_set(self):
        return self._flag

    isSet = is_set

    def set(self):
        self._flag = True

    def clear(self):
        self._flag = False

    def wait(self, timeout=None):
        if self._flag:
            return True
        return _wait_for(lambda: self._flag, timeout, 'Event.wait()') and self._flag


class Barrier:
    def __init__(self, parties, action=None, timeout=None):
        self._parties = parties
        self._action = action
        self._timeout = timeout
        self._count = 0
        self._generation = 0
        self._broken = False

    @property
    def parties(self):
        return self._parties

    @property
    def n_waiting(self):
        return self._count

    @property
    def broken(self):
        return self._broken

    def wait(self, timeout=None):
        if self._broken:
            raise BrokenBarrierError
        generation = self._generation
        index = self._count
        self._count += 1
        if self._count == self._parties:
            if self._action:
                self._action()
            self._count = 0
            self._generation += 1
            return index
        wait = timeout if timeout is not None else self._timeout
        if not _wait_for(lambda: self._generation != generation or self._broken, wait, 'Barrier.wait()'):
            self._broken = True
            raise BrokenBarrierError
        if self._broken:
            raise BrokenBarrierError
        return index

    def reset(self):
        self._count = 0
        self._generation += 1

    def abort(self):
        self._broken = True


class _local:
    """Dados locais da thread: cada thread enxerga o seu próprio conjunto de atributos."""

    def __new__(cls, *args, **kw):
        if (args or kw) and cls.__init__ is object.__init__:
            raise TypeError('Initialization arguments are not supported')
        self = object.__new__(cls)
        object.__setattr__(self, '_local__args', (args, kw))
        object.__setattr__(self, '_local__dicts', {})
        return self

    def _local__dict(self):
        dicts = object.__getattribute__(self, '_local__dicts')
        ident = _thread.get_ident()
        if ident not in dicts:
            dicts[ident] = {}
            args, kw = object.__getattribute__(self, '_local__args')
            if type(self).__init__ is not object.__init__:
                type(self).__init__(self, *args, **kw)
        return dicts[ident]

    def __getattribute__(self, name):
        if name.startswith('_local__') or name == '__class__':
            return object.__getattribute__(self, name)
        d = _local._local__dict(self)
        if name in d:
            return d[name]
        return object.__getattribute__(self, name)

    def __getattr__(self, name):
        d = _local._local__dict(self)
        if name in d:
            return d[name]
        raise AttributeError("'%s' object has no attribute '%s'" % (type(self).__name__, name))

    def __setattr__(self, name, value):
        if name == '__dict__':
            raise AttributeError("'%s' object attribute '__dict__' is read-only" % type(self).__name__)
        _local._local__dict(self)[name] = value

    def __delattr__(self, name):
        d = _local._local__dict(self)
        try:
            del d[name]
        except KeyError:
            raise AttributeError(name) from None


local = _local


def excepthook(args):
    import sys
    sys.stderr.write('Exception in thread %s:\n' % (args.thread.name if args.thread else None))
    import traceback
    traceback.print_exception(args.exc_type, args.exc_value, args.exc_traceback)


class _ExceptHookArgs:
    def __init__(self, exc_type, exc_value, exc_traceback, thread):
        self.exc_type = exc_type
        self.exc_value = exc_value
        self.exc_traceback = exc_traceback
        self.thread = thread


class Thread:
    def __init__(self, group=None, target=None, name=None, args=(), kwargs=None, *, daemon=None):
        _counter[0] += 1
        self._target = target
        self._args = args
        self._kwargs = kwargs if kwargs is not None else {}
        self._name = str(name) if name else 'Thread-%d' % _counter[0]
        if daemon is not None:
            self._daemon = bool(daemon)
        else:
            self._daemon = _state['current']._daemon
        self._started = False
        self._finished = False
        self._parked = False
        self._ident = None

    @property
    def name(self):
        return self._name

    @name.setter
    def name(self, value):
        self._name = str(value)

    @property
    def daemon(self):
        return self._daemon

    @daemon.setter
    def daemon(self, value):
        if self._started:
            raise RuntimeError('cannot set daemon status of active thread')
        self._daemon = bool(value)

    @property
    def ident(self):
        return self._ident

    native_id = ident

    def is_alive(self):
        return self._started and not self._finished

    def isDaemon(self):
        return self._daemon

    def setDaemon(self, daemonic):
        self.daemon = daemonic

    def getName(self):
        return self._name

    def setName(self, name):
        self._name = str(name)

    def run(self):
        try:
            if self._target is not None:
                self._target(*self._args, **self._kwargs)
        finally:
            del self._target, self._args, self._kwargs

    def start(self):
        if self._started:
            raise RuntimeError('threads can only be started once')
        self._started = True
        self._ident = _counter[0] + 1000
        _threads.append(self)
        _pending.append(self)

    def _bootstrap(self):
        previous = _state['current']
        _state['current'] = self
        _thread._idents.append(self._ident)
        try:
            try:
                self.run()
            except SystemExit:
                pass
            except _Parked as parked:
                self._parked = not parked.sliced
            except BaseException as exc:
                hook = excepthook
                hook(_ExceptHookArgs(type(exc), exc, exc.__traceback__, self))
        finally:
            _thread._idents.pop()
            _state['current'] = previous
            if not self._parked:
                self._finished = True
                if self in _threads:
                    _threads.remove(self)

    def join(self, timeout=None):
        if not self._started:
            raise RuntimeError('cannot join thread before it is started')
        if self is _state['current']:
            raise RuntimeError('cannot join current thread')
        _wait_for(lambda: self._finished, timeout, 'Thread.join()')

    def __repr__(self):
        status = 'initial' if not self._started else 'stopped' if self._finished else 'started'
        if self._daemon:
            status += ' daemon'
        if self._ident is not None:
            status += ' %s' % self._ident
        return '<%s(%s, %s)>' % (type(self).__name__, self._name, status)


class _MainThread(Thread):
    def __init__(self):
        self._target = None
        self._args = ()
        self._kwargs = {}
        self._name = 'MainThread'
        self._daemon = False
        self._started = True
        self._finished = False
        self._parked = False
        self._ident = _thread.get_ident()


class Timer(Thread):
    def __init__(self, interval, function, args=None, kwargs=None):
        Thread.__init__(self)
        self.interval = interval
        self.function = function
        self.args = args if args is not None else []
        self.kwargs = kwargs if kwargs is not None else {}
        self.finished = Event()

    def cancel(self):
        self.finished.set()

    def run(self):
        if not self.finished.is_set():
            _time.sleep(self.interval)
            if not self.finished.is_set():
                self.function(*self.args, **self.kwargs)
        self.finished.set()


_main = _MainThread()
_state = {'current': _main}


def current_thread():
    return _state['current']


currentThread = current_thread


def main_thread():
    return _main


def active_count():
    return 1 + len(_threads)


activeCount = active_count


def enumerate():
    return [_main] + list(_threads)


def _before_sleep(secs):
    """Gancho do `time.sleep`: devolve quanto ainda falta dormir de verdade.

    Na thread principal, o `sleep` deixa as threads pendentes rodarem durante aquele tempo (elas dormem de verdade,
    então o que gastaram é descontado). Uma thread que ainda quer dormir depois de esgotado o prazo da principal
    é desenrolada (como se a principal já tivesse acordado e seguido adiante)."""
    global _deadline
    if _state['current'] is _main:
        if not _pending:
            return secs
        start = _time.monotonic()
        saved = _deadline
        _deadline = start + secs
        try:
            _run_all()
        finally:
            _deadline = saved
        return max(0.0, secs - (_time.monotonic() - start))
    if _deadline is not None:
        left = _deadline - _time.monotonic()
        if left <= 0:
            # Passou do prazo da principal: uma espera isolada termina, mas quem continua dormindo em laço
            # (um trabalhador esperando um evento de parada) é desenrolado depois de duas esperas a mais.
            me = _state['current']
            me._overrun = getattr(me, '_overrun', 0) + 1
            if me._overrun > 2:
                raise _Parked(True)
        return secs
    return secs


_deadline = None


def _shutdown():
    """Na saída do interpretador, as threads que ainda não rodaram (e não são daemon) rodam até o fim."""
    for t in list(_pending):
        if t._daemon:
            _pending.remove(t)
    _run_all()


import atexit as _atexit

_atexit.register(_shutdown)
_time._sleep_hooks.append(_before_sleep)
