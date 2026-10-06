"""multiprocessing: o sandbox não cria processos do SO para Python, então cada `Process` roda como uma thread
(serial, como o `threading` daqui). A API e os resultados observáveis seguem o CPython (nomes `Process-N`,
`exitcode`, `Pool`, filas, `Value`, `Manager`); o que não existe é isolamento de memória entre "processos"."""

import os
import sys
import queue as _queue
import threading
import itertools
import traceback

__all__ = ['Process', 'Pool', 'Queue', 'SimpleQueue', 'JoinableQueue', 'Pipe', 'Lock', 'RLock', 'Semaphore',
           'BoundedSemaphore', 'Event', 'Condition', 'Barrier', 'Value', 'Array', 'Manager', 'cpu_count',
           'current_process', 'parent_process', 'active_children', 'freeze_support', 'set_start_method',
           'get_start_method', 'get_all_start_methods', 'get_context', 'TimeoutError', 'ProcessError',
           'AuthenticationError', 'BufferTooShort']


class ProcessError(Exception):
    pass


class BufferTooShort(ProcessError):
    pass


class TimeoutError(ProcessError):
    pass


class AuthenticationError(ProcessError):
    pass


_counter = itertools.count(1)
_children = set()
_local = threading.local()
_pid_state = []


def _next_pid():
    if not _pid_state:
        _pid_state.append(itertools.count(os.getpid() + 1))
    return next(_pid_state[0])


class Process:
    def __init__(self, group=None, target=None, name=None, args=(), kwargs=None, *, daemon=None):
        if group is not None:
            raise AssertionError('group argument must be None for now')
        self._target = target
        self._args = tuple(args)
        self._kwargs = dict(kwargs or {})
        self._identity = next(_counter)
        self.name = name or 'Process-%d' % self._identity
        self.daemon = bool(daemon)
        self._pid = None
        self._exitcode = None
        self._started = False
        self._closed = False
        self._thread = None
        self._parent = _current()

    def run(self):
        if self._target:
            self._target(*self._args, **self._kwargs)

    def _bootstrap(self):
        _local.process = self
        code = 0
        try:
            self.run()
        except SystemExit as e:
            if e.code is None:
                code = 0
            elif isinstance(e.code, int):
                code = e.code
            else:
                sys.stderr.write(str(e.code) + '\n')
                code = 1
        except BaseException:
            code = 1
            sys.stderr.write('Process %s:\n' % self.name)
            traceback.print_exc()
        finally:
            _local.process = None
            self._exitcode = code
            _children.discard(self)

    def start(self):
        if self._closed:
            raise ValueError('process object is closed')
        if self._started:
            raise AssertionError('cannot start a process twice')
        self._started = True
        self._pid = _next_pid()
        _children.add(self)
        self._thread = threading.Thread(target=self._bootstrap, name=self.name)
        self._thread.start()

    def terminate(self):
        pass

    def kill(self):
        pass

    def join(self, timeout=None):
        if not self._started:
            raise AssertionError('can only join a started process')
        if self._thread is not None:
            self._thread.join(timeout)

    def is_alive(self):
        if not self._started or self._exitcode is not None:
            return False
        return self._thread is not None and self._thread.is_alive()

    def close(self):
        if self.is_alive():
            raise ValueError('Cannot close a process while it is still running. You should first call join() or terminate().')
        self._closed = True

    @property
    def exitcode(self):
        return self._exitcode

    @property
    def pid(self):
        return self._pid

    ident = pid

    @property
    def sentinel(self):
        return self._pid

    @property
    def authkey(self):
        return b'0' * 32

    def __repr__(self):
        if not self._started:
            status = 'initial'
        elif self._exitcode is None:
            status = 'started'
        else:
            status = 'stopped exitcode=%d' % self._exitcode if self._exitcode >= 0 else 'stopped'
        pid = ' pid=%s' % self._pid if self._pid is not None else ''
        return '<%s name=%r%s parent=%s %s%s>' % (type(self).__name__, self.name, pid, os.getpid(), status,
                                                  ' daemon' if self.daemon else '')


class _MainProcess(Process):
    def __init__(self):
        self._identity = 0
        self.name = 'MainProcess'
        self.daemon = False
        self._pid = os.getpid()
        self._exitcode = None
        self._started = True
        self._closed = False
        self._thread = None
        self._parent = None
        self._target = None
        self._args = ()
        self._kwargs = {}

    def is_alive(self):
        return True


_main_process = None


def _current():
    global _main_process
    cur = getattr(_local, 'process', None)
    if cur is not None:
        return cur
    if _main_process is None:
        _main_process = _MainProcess()
    return _main_process


def current_process():
    return _current()


def parent_process():
    return _current()._parent


def active_children():
    return [p for p in list(_children) if p.is_alive()]


def cpu_count():
    n = os.cpu_count()
    if n is None:
        raise NotImplementedError('cannot determine number of cpus')
    return n


def freeze_support():
    pass


_start_method = 'fork'


def set_start_method(method, force=False):
    global _start_method
    if method not in get_all_start_methods():
        raise ValueError('cannot find context for %r' % method)
    _start_method = method


def get_start_method(allow_none=False):
    return _start_method


def get_all_start_methods():
    return ['fork', 'spawn', 'forkserver']


# --- filas e pipes -----------------------------------------------------------------------------------------

class Queue(_queue.Queue):
    def __init__(self, maxsize=0, *, ctx=None):
        super().__init__(maxsize)
        self._closed_flag = False

    def close(self):
        self._closed_flag = True

    def join_thread(self):
        pass

    def cancel_join_thread(self):
        pass

    def put(self, obj, block=True, timeout=None):
        if self._closed_flag:
            raise ValueError('Queue %r is closed' % self)
        super().put(obj, block, timeout)

    def get(self, block=True, timeout=None):
        return super().get(block, timeout)


class JoinableQueue(Queue):
    pass


class SimpleQueue:
    def __init__(self, *, ctx=None):
        self._q = _queue.Queue()

    def put(self, obj):
        self._q.put(obj)

    def get(self):
        return self._q.get()

    def empty(self):
        return self._q.empty()

    def close(self):
        pass


class Connection:
    def __init__(self, inbox, outbox):
        self._in = inbox
        self._out = outbox
        self._closed = False

    def send(self, obj):
        if self._closed:
            raise OSError('handle is closed')
        self._out.put(obj)

    def recv(self):
        if self._closed:
            raise OSError('handle is closed')
        return self._in.get()

    def send_bytes(self, buf, offset=0, size=None):
        data = bytes(buf)[offset:] if size is None else bytes(buf)[offset:offset + size]
        self._out.put(data)

    def recv_bytes(self, maxlength=None):
        return self._in.get()

    def poll(self, timeout=0.0):
        if timeout:
            try:
                item = self._in.get(timeout=timeout)
            except _queue.Empty:
                return False
            self._in.put(item)
            return True
        return not self._in.empty()

    def close(self):
        self._closed = True

    @property
    def closed(self):
        return self._closed

    def fileno(self):
        return -1

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


def Pipe(duplex=True):
    a, b = _queue.Queue(), _queue.Queue()
    if duplex:
        return Connection(a, b), Connection(b, a)
    return Connection(a, None), Connection(None, a)


# --- sincronização -----------------------------------------------------------------------------------------

def Lock():
    return threading.Lock()


def RLock():
    return threading.RLock()


def Semaphore(value=1):
    return threading.Semaphore(value)


def BoundedSemaphore(value=1):
    return threading.BoundedSemaphore(value)


def Event():
    return threading.Event()


def Condition(lock=None):
    return threading.Condition(lock)


def Barrier(parties, action=None, timeout=None):
    return threading.Barrier(parties, action, timeout)


# --- memória compartilhada (compartilhada de fato: é a mesma memória) --------------------------------------

_TYPECODES = {'c': bytes, 'b': int, 'B': int, 'h': int, 'H': int, 'i': int, 'I': int, 'l': int, 'L': int,
              'q': int, 'Q': int, 'f': float, 'd': float, 'u': str}


class _Synchronized:
    def __init__(self, value, lock):
        self._value = value
        self._lock = lock

    def get_lock(self):
        return self._lock

    def acquire(self, *a, **k):
        return self._lock.acquire(*a, **k)

    def release(self):
        return self._lock.release()

    def __enter__(self):
        self._lock.acquire()
        return self

    def __exit__(self, *exc):
        self._lock.release()

    @property
    def value(self):
        return self._value

    @value.setter
    def value(self, v):
        self._value = v


def Value(typecode_or_type, *args, lock=True):
    default = 0
    if typecode_or_type in ('f', 'd'):
        default = 0.0
    elif typecode_or_type in ('c', 'u'):
        default = b'\0' if typecode_or_type == 'c' else '\0'
    v = args[0] if args else default
    if lock is True or lock is None:
        lock = threading.RLock()
    elif lock is False:
        lock = _NoLock()
    return _Synchronized(v, lock)


class _NoLock:
    def acquire(self, *a, **k):
        return True

    def release(self):
        pass


class _SynchronizedArray(_Synchronized):
    def __getitem__(self, i):
        return self._value[i]

    def __setitem__(self, i, v):
        self._value[i] = v

    def __len__(self):
        return len(self._value)

    def __iter__(self):
        return iter(self._value)

    def __getslice__(self, a, b):
        return self._value[a:b]


def Array(typecode_or_type, size_or_initializer, *, lock=True):
    if isinstance(size_or_initializer, int):
        zero = 0.0 if typecode_or_type in ('f', 'd') else 0
        data = [zero] * size_or_initializer
    else:
        data = list(size_or_initializer)
    if lock is True or lock is None:
        lock = threading.RLock()
    elif lock is False:
        lock = _NoLock()
    return _SynchronizedArray(data, lock)


# --- Pool ---------------------------------------------------------------------------------------------------

class AsyncResult:
    def __init__(self, future, callback=None, error_callback=None):
        self._future = future
        self._callback = callback
        self._error_callback = error_callback
        future.add_done_callback(self._done)

    def _done(self, fut):
        exc = fut.exception()
        if exc is None:
            if self._callback:
                self._callback(fut.result())
        elif self._error_callback:
            self._error_callback(exc)

    def get(self, timeout=None):
        import concurrent.futures as cf
        try:
            return self._future.result(timeout)
        except cf.TimeoutError:
            raise TimeoutError from None

    def wait(self, timeout=None):
        import concurrent.futures as cf
        try:
            self._future.result(timeout)
        except cf.TimeoutError:
            pass
        except BaseException:
            pass

    def ready(self):
        return self._future.done()

    def successful(self):
        if not self.ready():
            raise ValueError('%r not ready' % self)
        return self._future.exception() is None


class MapResult(AsyncResult):
    def __init__(self, futures, callback=None, error_callback=None):
        self._futures = futures
        self._callback = callback
        self._error_callback = error_callback
        self._reported = False

    def _collect(self, timeout=None):
        import concurrent.futures as cf
        try:
            out = [f.result(timeout) for f in self._futures]
        except cf.TimeoutError:
            raise TimeoutError from None
        except BaseException as e:
            if self._error_callback and not self._reported:
                self._reported = True
                self._error_callback(e)
            raise
        if self._callback and not self._reported:
            self._reported = True
            self._callback(out)
        return out

    def get(self, timeout=None):
        return self._collect(timeout)

    def wait(self, timeout=None):
        try:
            self._collect(timeout)
        except BaseException:
            pass

    def ready(self):
        return all(f.done() for f in self._futures)

    def successful(self):
        if not self.ready():
            raise ValueError('%r not ready' % self)
        return all(f.exception() is None for f in self._futures)


class Pool:
    def __init__(self, processes=None, initializer=None, initargs=(), maxtasksperchild=None, context=None):
        import concurrent.futures as cf
        if processes is None:
            processes = cpu_count()
        if processes < 1:
            raise ValueError('Number of processes must be at least 1')
        self._processes = processes
        self._state = 'RUN'
        self._initializer = initializer
        self._initargs = initargs
        self._executor = cf.ThreadPoolExecutor(max_workers=processes)
        self._initialized = False

    def _check(self):
        if self._state != 'RUN':
            raise ValueError('Pool not running')
        if not self._initialized:
            self._initialized = True
            if self._initializer:
                self._initializer(*self._initargs)

    def apply(self, func, args=(), kwds={}):
        return self.apply_async(func, args, kwds).get()

    def apply_async(self, func, args=(), kwds={}, callback=None, error_callback=None):
        self._check()
        return AsyncResult(self._executor.submit(func, *args, **kwds), callback, error_callback)

    def map(self, func, iterable, chunksize=None):
        return self.map_async(func, iterable, chunksize).get()

    def map_async(self, func, iterable, chunksize=None, callback=None, error_callback=None):
        self._check()
        futures = [self._executor.submit(func, x) for x in iterable]
        return MapResult(futures, callback, error_callback)

    def starmap(self, func, iterable, chunksize=None):
        return self.starmap_async(func, iterable, chunksize).get()

    def starmap_async(self, func, iterable, chunksize=None, callback=None, error_callback=None):
        self._check()
        futures = [self._executor.submit(func, *args) for args in iterable]
        return MapResult(futures, callback, error_callback)

    def imap(self, func, iterable, chunksize=1):
        self._check()
        futures = [self._executor.submit(func, x) for x in iterable]
        return (f.result() for f in futures)

    def imap_unordered(self, func, iterable, chunksize=1):
        return self.imap(func, iterable, chunksize)

    def close(self):
        self._state = 'CLOSE'

    def terminate(self):
        self._state = 'TERMINATE'
        self._executor.shutdown(wait=False)

    def join(self):
        if self._state == 'RUN':
            raise ValueError('Pool is still running')
        self._executor.shutdown(wait=True)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.terminate()


# --- Manager ------------------------------------------------------------------------------------------------

class _Namespace:
    def __init__(self, **kwds):
        self.__dict__.update(kwds)

    def __repr__(self):
        items = ['%s=%r' % kv for kv in sorted(self.__dict__.items()) if not kv[0].startswith('_')]
        return 'Namespace(%s)' % ', '.join(items)


class SyncManager:
    def __init__(self, *args, **kwargs):
        self._started = False

    def start(self, initializer=None, initargs=()):
        self._started = True

    def shutdown(self):
        self._started = False

    def connect(self):
        pass

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, *exc):
        self.shutdown()

    def dict(self, *args, **kwargs):
        return dict(*args, **kwargs)

    def list(self, seq=()):
        return list(seq)

    def Queue(self, maxsize=0):
        return Queue(maxsize)

    def JoinableQueue(self, maxsize=0):
        return JoinableQueue(maxsize)

    def Lock(self):
        return Lock()

    def RLock(self):
        return RLock()

    def Event(self):
        return Event()

    def Semaphore(self, value=1):
        return Semaphore(value)

    def Condition(self, lock=None):
        return Condition(lock)

    def Barrier(self, parties, action=None, timeout=None):
        return Barrier(parties, action, timeout)

    def Value(self, typecode, value, lock=True):
        return Value(typecode, value, lock=lock)

    def Array(self, typecode, sequence, lock=True):
        return Array(typecode, sequence, lock=lock)

    def Namespace(self, **kwds):
        return _Namespace(**kwds)

    def Pool(self, *args, **kwargs):
        return Pool(*args, **kwargs)


def Manager():
    m = SyncManager()
    m.start()
    return m


# --- contexto ------------------------------------------------------------------------------------------------

class _Context:
    def __init__(self, method):
        self._method = method

    def get_start_method(self, allow_none=False):
        return self._method

    def __getattr__(self, name):
        return globals()[name]


def get_context(method=None):
    if method is not None and method not in get_all_start_methods():
        raise ValueError('cannot find context for %r' % method)
    return _Context(method or _start_method)
