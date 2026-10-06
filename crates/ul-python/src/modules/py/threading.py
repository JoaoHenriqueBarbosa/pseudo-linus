"""threading: o interpretador tem um único fluxo de execução, então cada `Thread` roda até o fim dentro
de `start()`. Resultados e efeitos são os mesmos de um programa que espera as threads, mas não há
concorrência: quem bloquearia esperando outra thread (um `Event` nunca ligado, uma fila vazia
consumida por um trabalhador que roda antes do produtor) levanta `RuntimeError` em vez de travar."""

import _thread
import time as _time

__all__ = ['Thread', 'Lock', 'RLock', 'Event', 'Condition', 'Semaphore', 'BoundedSemaphore', 'Barrier', 'Timer',
           'local', 'current_thread', 'main_thread', 'active_count', 'enumerate', 'get_ident', 'get_native_id',
           'excepthook', 'ThreadError', 'TIMEOUT_MAX', 'settrace', 'setprofile', 'stack_size']

TIMEOUT_MAX = 9223372036.0
ThreadError = RuntimeError
get_ident = _thread.get_ident
get_native_id = _thread.get_ident
Lock = _thread.allocate_lock
_counter = [0]
_threads = []


def settrace(func):
    pass


def setprofile(func):
    pass


def stack_size(size=0):
    return 0


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


class Condition:
    def __init__(self, lock=None):
        self._lock = lock if lock is not None else RLock()
        self.acquire = self._lock.acquire
        self.release = self._lock.release
        self._waiters = 0

    def __enter__(self):
        return self._lock.__enter__()

    def __exit__(self, *args):
        return self._lock.__exit__(*args)

    def wait(self, timeout=None):
        if timeout is not None:
            _time.sleep(timeout)
            return False
        raise RuntimeError('deadlock: wait() sem outra thread para notificar')

    def wait_for(self, predicate, timeout=None):
        result = predicate()
        if not result:
            self.wait(timeout)
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
        if self._value > 0:
            self._value -= 1
            return True
        if not blocking:
            return False
        if timeout is not None:
            _time.sleep(timeout)
            return False
        raise RuntimeError('deadlock: semáforo esgotado sem outra thread para liberar')

    __enter__ = acquire

    def release(self, n=1):
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
        if timeout is not None:
            _time.sleep(timeout)
            return self._flag
        raise RuntimeError('deadlock: Event.wait() sem outra thread para ligar o evento')


class Barrier:
    def __init__(self, parties, action=None, timeout=None):
        self._parties = parties
        self._action = action
        self._count = 0
        self.broken = False

    @property
    def parties(self):
        return self._parties

    @property
    def n_waiting(self):
        return self._count

    def wait(self, timeout=None):
        self._count += 1
        index = self._count - 1
        if self._count == self._parties:
            if self._action:
                self._action()
            self._count = 0
        return index

    def reset(self):
        self._count = 0

    def abort(self):
        self.broken = True


class _local:
    """Dados locais da thread: com um fluxo só, são os dados do próprio objeto."""


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
        self._daemon = bool(daemon) if daemon is not None else False
        self._started = False
        self._finished = False
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
        self._daemon = bool(daemonic)

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
        previous = _state['current']
        _state['current'] = self
        _threads.append(self)
        try:
            try:
                self.run()
            except SystemExit:
                pass
            except BaseException as exc:
                import sys
                excepthook(_ExceptHookArgs(type(exc), exc, exc.__traceback__, self))
        finally:
            _state['current'] = previous
            self._finished = True
            _threads.remove(self)

    def join(self, timeout=None):
        if not self._started:
            raise RuntimeError('cannot join thread before it is started')

    def __repr__(self):
        status = 'initial' if not self._started else 'stopped' if self._finished else 'started'
        if self._daemon:
            status += ' daemon'
        if self._ident is not None:
            status += ' %s' % self._ident
        return '<%s(%s, %s)>' % (type(self).__name__, self._name, status)


class _MainThread(Thread):
    def __init__(self):
        Thread.__init__(self, name='MainThread')
        _counter[0] -= 1
        self._started = True
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
