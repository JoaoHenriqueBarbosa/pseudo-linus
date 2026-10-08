"""_thread: as threads são verdes (pilhas de quadros da VM que o laço de instruções troca, ver `_gsched`). Quem
precisa esperar um travão pede ao escalonador do `threading` (`_hooks['wait']`): a thread fica suspensa e as
outras rodam até a condição valer, como no CPython com a GIL."""

import _os
import _weakref

error = RuntimeError
TIMEOUT_MAX = 9223372036.0

# Os identificadores imitam o `pthread_self` do glibc: a thread principal e, abaixo dela, uma pilha de 8 MiB
# por thread criada. O `slot` de cada um serve ao `get_native_id` (o tid do kernel é o pid mais o slot, e a
# principal tem tid igual ao pid).
_MAIN_IDENT = 0x7ffff7d8a740
_THREAD_TOP = 0x7ffff75ff6c0
_STACK_STEP = 0x800000

# Pilha de identificadores: o da thread que está rodando é o último (`threading` empilha ao rodar uma thread).
_idents = [_MAIN_IDENT]
_slots = {}
_next_slot = [0]
# Pilhas já criadas e as liberadas por threads que acabaram: o glibc reaproveita a mais recente na próxima
# thread (o `pthread_self` se repete, o tid do kernel não).
_stacks = [0]
_free_idents = []
_quarantine = []
_locals = []

# O `threading` registra aqui o que o `_thread` não sabe fazer sozinho: `wait` (espera cooperativa), `start`
# (criar uma thread) e `count` (quantas threads vivas além da principal).
_hooks = {}

# Com `_busy[0]` ligado a fatia de tempo que vence não troca de thread (ver `_gsched.preempt`): o travão e o
# escalonador rodam inteiros, como as operações de C do CPython sob a GIL.
_busy = [False]


class _Atomic:
    """Seção atômica: dentro do `with` a vez não passa a outra thread por preempção. Quem entra guarda o valor
    anterior e o devolve ao sair, então as seções se aninham, e uma que bloqueia (a espera de um travão) deixa
    o escalonador trocar de thread de forma explícita."""

    __slots__ = ('prev',)

    def __enter__(self):
        self.prev = _busy[0]
        _busy[0] = True

    def __exit__(self, *exc):
        _busy[0] = self.prev


def _new_ident():
    _next_slot[0] += 1
    if _free_idents:
        ident = _free_idents.pop()
    else:
        ident = _THREAD_TOP - _stacks[0] * _STACK_STEP
        _stacks[0] += 1
    _slots[ident] = _next_slot[0]
    return ident


def _release_quarantine():
    """Passou tempo de verdade (um `sleep`): as threads que terminaram já saíram, e as pilhas delas servem de novo."""
    _free_idents.extend(_quarantine)
    del _quarantine[:]


def _release_ident(ident):
    """A thread de `ident` acabou: a pilha volta para reuso e os dados `threading.local` dela se perdem.
    A pilha de uma thread que acaba de terminar ainda está sendo desmontada quando a próxima nasce, então
    só a da thread anterior entra para reuso (quarentena de uma)."""
    if _quarantine:
        _free_idents.append(_quarantine.pop())
    _quarantine.append(ident)
    for ref in list(_locals):
        local = ref()
        if local is None:
            _locals.remove(ref)
        else:
            object.__getattribute__(local, '_local__dicts').pop(ident, None)


def get_ident():
    return _idents[-1]


def get_native_id():
    ident = _idents[-1]
    return _os.getpid() + _slots.get(ident, 0)


def _block(cond, timeout, what):
    """Espera `cond()` valer (`timeout` em segundos, `None` sem prazo). Devolve `cond()` ao fim."""
    wait = _hooks.get('wait')
    if wait is None:
        if cond():
            return True
        import threading
        wait = _hooks['wait']
    return wait(cond, timeout, what)


def _lock_args(blocking, timeout):
    """O `lock_acquire_parse_args` do `_threadmodule.c`: valida e devolve o prazo (`None` sem prazo)."""
    if not blocking and timeout != -1:
        raise ValueError("can't specify a timeout for a non-blocking call")
    # `_PyTime_FromSecondsObject` (nanossegundos em 64 bits) vem antes do teto do `PyThread_acquire_lock_timed`.
    if abs(timeout) >= 9223372036.854775807:
        raise OverflowError('timestamp out of range for platform time_t')
    if timeout < 0 and timeout != -1:
        raise ValueError('timeout value must be a non-negative number')
    if timeout > TIMEOUT_MAX:
        raise OverflowError('timeout value is too large')
    return None if timeout == -1 else timeout


class lock:
    """O `_thread.lock` do 3.13: é ele que `threading.Lock` aponta, então pode ficar como atributo de
    classe (`lock_class = threading.Lock` do werkzeug) sem virar método ligado."""

    def __init__(self, *args, **kwargs):
        if args or kwargs:
            raise TypeError(f'lock expected 0 arguments, got {len(args) + len(kwargs)}')
        self._locked = False

    def __init_subclass__(cls, **kwargs):
        raise TypeError("type '_thread.lock' is not an acceptable base type")

    def __repr__(self):
        state = 'locked' if self._locked else 'unlocked'
        return f'<{state} _thread.lock object at {id(self):#x}>'

    def acquire(self, blocking=True, timeout=-1):
        wait = _lock_args(blocking, timeout)
        with _Atomic():
            if self._locked:
                if not blocking:
                    return False
                if not _block(lambda: not self._locked, wait, 'lock.acquire()'):
                    return False
            self._locked = True
            return True

    def release(self):
        with _Atomic():
            if not self._locked:
                raise RuntimeError('release unlocked lock')
            self._locked = False

    def locked(self):
        return self._locked

    def _at_fork_reinit(self):
        self._locked = False

    def __enter__(self):
        self.acquire()
        return True

    def __exit__(self, *exc):
        self.release()

    acquire_lock = acquire
    release_lock = release
    locked_lock = locked


LockType = lock


def allocate_lock():
    return lock()


allocate = allocate_lock


class RLock:
    """O `_thread.RLock` do 3.13: reentrante, com dono (o `get_ident` de quem o pegou) e contagem."""

    def __init__(self, *args, **kwargs):
        self._owner = 0
        self._count = 0

    def acquire(self, blocking=True, timeout=-1):
        wait = _lock_args(blocking, timeout)
        me = get_ident()
        with _Atomic():
            if self._count > 0 and self._owner == me:
                self._count += 1
                return True
            if self._count > 0:
                if not blocking:
                    return False
                if not _block(lambda: self._count == 0, wait, 'RLock.acquire()'):
                    return False
            self._owner = me
            self._count = 1
            return True

    def release(self):
        with _Atomic():
            if self._count == 0 or self._owner != get_ident():
                raise RuntimeError('cannot release un-acquired lock')
            self._count -= 1
            if self._count == 0:
                self._owner = 0

    def __enter__(self):
        return self.acquire()

    def __exit__(self, *exc):
        self.release()

    def _is_owned(self):
        return self._count > 0 and self._owner == get_ident()

    def _recursion_count(self):
        return self._count if self._owner == get_ident() else 0

    def _release_save(self):
        with _Atomic():
            if self._count == 0:
                raise RuntimeError('cannot release un-acquired lock')
            saved = (self._count, self._owner)
            self._count = 0
            self._owner = 0
            return saved

    def _acquire_restore(self, saved):
        count, owner = saved
        with _Atomic():
            if self._count > 0:
                _block(lambda: self._count == 0, None, 'RLock.acquire()')
            self._owner = owner
            self._count = count

    def _at_fork_reinit(self):
        self._owner = 0
        self._count = 0

    def __repr__(self):
        state = 'locked' if self._count > 0 else 'unlocked'
        return f'<{state} {type(self).__module__}.{type(self).__qualname__} object owner={self._owner} count={self._count} at {id(self):#x}>'


class _ExceptHookArgs(tuple):
    """`_thread._ExceptHookArgs`: estrutura (exc_type, exc_value, exc_traceback, thread) que o `excepthook` recebe."""

    __slots__ = ()
    n_fields = 4
    n_sequence_fields = 4
    n_unnamed_fields = 0

    def __new__(cls, args):
        args = tuple(args)
        if len(args) != 4:
            raise TypeError(f'_thread._ExceptHookArgs() takes a 4-sequence ({len(args)}-sequence given)')
        return tuple.__new__(cls, args)

    exc_type = property(lambda self: self[0])
    exc_value = property(lambda self: self[1])
    exc_traceback = property(lambda self: self[2])
    thread = property(lambda self: self[3])

    def __repr__(self):
        return ('_thread._ExceptHookArgs(exc_type=%r, exc_value=%r, exc_traceback=%r, thread=%r)' % tuple(self))


_ExceptHookArgs.__module__ = '_thread'
_ExceptHookArgs.__name__ = '_ExceptHookArgs'


def _excepthook(args):
    """O `thread_excepthook` do `_threadmodule.c`: `SystemExit` é ignorado; o resto vai para `sys.stderr`."""
    import sys
    if not isinstance(args, _ExceptHookArgs):
        raise TypeError('_thread.excepthook argument type must be ExceptHookArgs')
    exc_type, exc_value, exc_tb, thread = args
    if exc_type is SystemExit:
        return
    file = sys.stderr
    if file is None and thread is not None:
        file = getattr(thread, '_stderr', None)
    if file is None:
        return
    name = thread.name if thread is not None else get_ident()
    file.write('Exception in thread %s:\n' % (name,))
    import traceback
    traceback.print_exception(exc_type, exc_value, exc_tb, file=file)
    file.flush()


_stack = [0]


def stack_size(size=0):
    """Tamanho da pilha das próximas threads: devolve o anterior; 0 é o padrão do sistema."""
    if not isinstance(size, int):
        raise TypeError("'%s' object cannot be interpreted as an integer" % type(size).__name__)
    if size < 0:
        raise ValueError('size must be 0 or a positive value')
    if size != 0 and size < 32768:
        raise ValueError('size not valid: %d bytes' % size)
    old = _stack[0]
    _stack[0] = size
    return old


def start_new_thread(function, args, kwargs=None):
    if not callable(function):
        raise TypeError('first arg must be callable')
    if not isinstance(args, tuple):
        raise TypeError('2nd arg must be a tuple')
    if kwargs is not None and not isinstance(kwargs, dict):
        raise TypeError('optional 3rd arg must be a dictionary')
    import threading
    return _hooks['start'](function, args, kwargs if kwargs is not None else {})


start_new = start_new_thread


def exit():
    raise SystemExit


exit_thread = exit


def _count():
    """Quantas threads vivas há além da principal."""
    count = _hooks.get('count')
    return count() if count is not None else 0


def interrupt_main(signum=2):
    """Simula a chegada de `signum` na thread principal: roda o tratador Python registrado (o do SIGINT padrão
    levanta `KeyboardInterrupt`); sem tratador Python (SIG_DFL, SIG_IGN) não faz nada."""
    import _signal as signal
    if not isinstance(signum, int):
        raise TypeError("'%s' object cannot be interpreted as an integer" % type(signum).__name__)
    if not 0 < signum < signal.NSIG:
        raise ValueError('signal number out of range')
    # A chegada passa pelo mesmo caminho de um sinal capturado: escreve no fd de `set_wakeup_fd` e roda o
    # tratador Python; sem tratador (SIG_DFL, SIG_IGN) não há o que fazer.
    signal._dispatch([signum])


class _local:
    """Dados locais da thread: cada thread enxerga o seu próprio conjunto de atributos."""

    def __new__(cls, *args, **kw):
        if (args or kw) and cls.__init__ is object.__init__:
            raise TypeError('Initialization arguments are not supported')
        self = object.__new__(cls)
        object.__setattr__(self, '_local__args', (args, kw))
        object.__setattr__(self, '_local__dicts', {})
        _locals.append(_weakref.ref(self))
        return self

    def _local__dict(self):
        dicts = object.__getattribute__(self, '_local__dicts')
        ident = get_ident()
        if ident not in dicts:
            dicts[ident] = {}
            args, kw = object.__getattribute__(self, '_local__args')
            if type(self).__init__ is not object.__init__:
                type(self).__init__(self, *args, **kw)
        return dicts[ident]

    def _local__type_name(self):
        """O `tp_name`: o tipo estático é qualificado (`_thread._local`), o de uma subclasse não."""
        cls = type(self)
        return '_thread._local' if cls is _local else cls.__name__

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
        raise AttributeError("'%s' object has no attribute '%s'" % (_local._local__type_name(self), name))

    def __setattr__(self, name, value):
        if name == '__dict__':
            raise AttributeError("'%s' object attribute '__dict__' is read-only" % _local._local__type_name(self))
        _local._local__dict(self)[name] = value

    def __delattr__(self, name):
        d = _local._local__dict(self)
        try:
            del d[name]
        except KeyError:
            raise AttributeError("'%s' object has no attribute '%s'" % (_local._local__type_name(self), name)) from None
