"""Implementação do módulo `select` do sandbox (o `select.py` só reexporta os nomes públicos, para o `dir(select)`
ficar como o do CPython). `select()`, `poll()` e `epoll` sobre o kernel do sandbox: `poll(2)` e `epoll(7)` valem
para qualquer descritor (pipe, terminal, arquivo, socket), porque todo socket do Python é um fd do kernel, então não
há estado em processo para consultar. O `epoll` é um fd de verdade (`anon_inode:[eventpoll]`)."""

import _os
import _sys
import errno as _errno

error = OSError

POLLIN = 1
POLLPRI = 2
POLLOUT = 4
POLLERR = 8
POLLHUP = 16
POLLNVAL = 32
POLLRDNORM = 64
POLLRDBAND = 128
POLLWRNORM = 256
POLLWRBAND = 512
POLLMSG = 1024
POLLRDHUP = 8192

EPOLLIN = 1
EPOLLPRI = 2
EPOLLOUT = 4
EPOLLERR = 8
EPOLLHUP = 16
EPOLLRDNORM = 64
EPOLLRDBAND = 128
EPOLLWRNORM = 256
EPOLLWRBAND = 512
EPOLLMSG = 1024
EPOLLRDHUP = 8192
EPOLLEXCLUSIVE = 1 << 28
EPOLLONESHOT = 1 << 30
EPOLLET = 1 << 31
EPOLL_CLOEXEC = 524288

PIPE_BUF = 4096

_FD_SETSIZE = 1024
_TIME_MAX = 1 << 63
_INT_MAX = (1 << 31) - 1
_MISSING = object()
_CTL_ADD = 1
_CTL_DEL = 2
_CTL_MOD = 3


# ---- conversão de argumentos como o Argument Clinic do CPython ----

def _int(value):
    if type(value) is int:
        return value
    try:
        index = value.__index__
    except AttributeError:
        raise TypeError("'%s' object cannot be interpreted as an integer" % type(value).__name__) from None
    return index()


def _cint(value):
    value = _int(value)
    if not -_INT_MAX - 1 <= value <= _INT_MAX:
        raise OverflowError('Python int too large to convert to C int')
    return value


def _uint(value):
    return _int(value) & 0xFFFFFFFF


def _ushort(value):
    return _int(value) & 0xFFFF


def _fileno(obj):
    """O `PyObject_AsFileDescriptor` do conversor `fildes`."""
    if isinstance(obj, int):
        fd = obj
    elif hasattr(obj, 'fileno'):
        fd = obj.fileno()
        if not isinstance(fd, int):
            raise TypeError('fileno() returned a non-integer')
    else:
        raise TypeError('argument must be an int, or have a fileno() method.')
    if fd < 0:
        raise ValueError('file descriptor cannot be a negative integer (%d)' % fd)
    return fd


def _unpack(fname, names, required, args, kwargs, defaults=()):
    """Os argumentos de um método com palavras-chave (`_PyArg_UnpackKeywords`): `names` na ordem, os `required`
    primeiros obrigatórios e `defaults` para os demais."""
    total = len(names)
    given = len(args)
    if given > total:
        raise TypeError('%s() takes %s %d positional argument%s (%d given)' % (
            fname, 'at most' if required < total else 'exactly', total, '' if total == 1 else 's', given))
    for name in kwargs:
        if name not in names:
            raise TypeError("'%s' is an invalid keyword argument for %s()" % (name, fname))
    for index in range(given):
        if names[index] in kwargs:
            raise TypeError("argument for %s() given by name ('%s') and position (%d)" % (fname, names[index], index + 1))
    values = list(args)
    for index in range(given, total):
        name = names[index]
        if name in kwargs:
            values.append(kwargs[name])
        elif index < required:
            raise TypeError("%s() missing required argument '%s' (pos %d)" % (fname, name, index + 1))
        else:
            values.append(defaults[index - required])
    return values


def _positional(fname, args, low, high):
    """Os argumentos só posicionais de um método (`_PyArg_CheckPositional`), completados com `_MISSING`."""
    given = len(args)
    if given < low or given > high:
        want = low if given < low else high
        raise TypeError('%s expected %s%d argument%s, got %d' % (
            fname, '' if low == high else ('at least ' if given < low else 'at most '), want, '' if want == 1 else 's', given))
    return list(args) + [_MISSING] * (high - given)


def _only(fname, args):
    """O único argumento de um método `METH_O`."""
    if len(args) != 1:
        raise TypeError('%s() takes exactly one argument (%d given)' % (fname, len(args)))
    return args[0]


def _timeout_ms(obj, scale):
    """O prazo em milissegundos, arredondado para cima (`-1` é sem limite). `scale` converte a unidade do argumento
    em nanossegundos: 10**9 para segundos (`epoll.poll`), 10**6 para milissegundos (`poll.poll`)."""
    if obj is None:
        return -1
    if isinstance(obj, float):
        if obj != obj:
            raise ValueError('Invalid value NaN (not a number)')
        scaled = obj * scale
        if not -9.223372036854776e18 <= scaled < 9.223372036854776e18:
            raise OverflowError('timestamp too large to convert to C PyTime_t')
        ns = int(scaled)
        if ns < scaled:
            ns += 1
    else:
        try:
            index = obj.__index__
        except AttributeError:
            raise TypeError('timeout must be an integer or None') from None
        value = index()
        if not -_TIME_MAX <= value < _TIME_MAX:
            raise OverflowError('timestamp out of range for platform time_t')
        ns = value * scale
        if not -_TIME_MAX <= ns < _TIME_MAX:
            raise OverflowError('timestamp too large to convert to C PyTime_t')
    ms = -((-ns) // 1000000)
    if not -_INT_MAX - 1 <= ms <= _INT_MAX:
        raise OverflowError('timeout is too large')
    return ms


# ---- espera cooperativa ----

def _await(cond, entries, timeout, what):
    """Espera `cond()`. A thread suspende (`_net.wait_fds`): as outras threads e serviços cooperativos rodam, e
    quando ninguém mais pode rodar o `poll(2)` do kernel vigia os descritores e acorda quando outro processo
    escrever. Sem descritores e sem prazo a espera é eterna, como a do `select(2)`. Uma thread que esperasse
    em fatias sem ceder a vez prenderia todas as outras (o `Pool` do `multiprocessing` espera assim)."""
    import time
    import _net
    deadline = None if timeout is None else time.monotonic() + timeout
    while True:
        left = None if deadline is None else max(deadline - time.monotonic(), 0.0)
        if _net.wait_fds(entries.items(), cond, left, what):
            return True
        if deadline is not None and time.monotonic() >= deadline:
            return False


def _seconds(timeout):
    """O prazo em segundos como o `_PyTime_FromSecondsObject` com arredondamento para cima, ou `None`."""
    if timeout is None:
        return None
    if isinstance(timeout, float):
        if timeout != timeout:
            raise ValueError('Invalid value NaN (not a number)')
        scaled = timeout * 1e9
        if not -9.223372036854776e18 <= scaled < 9.223372036854776e18:
            raise OverflowError('timestamp too large to convert to C PyTime_t')
        ns = int(scaled)
        if ns < scaled:
            ns += 1
    else:
        try:
            sec = timeout.__index__()
        except AttributeError:
            raise TypeError("'%s' object cannot be interpreted as an integer" % type(timeout).__name__) from None
        if not -_TIME_MAX <= sec < _TIME_MAX:
            raise OverflowError('timestamp out of range for platform time_t')
        ns = sec * 1000000000
        if not -_TIME_MAX <= ns < _TIME_MAX:
            raise OverflowError('timestamp too large to convert to C PyTime_t')
    if ns < 0:
        raise ValueError('timeout must be non-negative')
    return ns / 1e9


# ---- select() ----

def _bad_fd():
    return OSError(_errno.EBADF, 'Bad file descriptor')


def _kernel_revents(entries, timeout):
    """Os `revents` do kernel por fd; fd inválido (`POLLNVAL`) é EBADF, como o `select(2)`."""
    revents = _os.poll(list(entries.items()), timeout)
    out = dict(zip(entries, revents))
    if any(r & POLLNVAL for r in out.values()):
        raise _bad_fd()
    return out


def _split(rlist, wlist, xlist):
    lists = []
    for seq in (rlist, wlist, xlist):
        try:
            seq = list(seq)
        except TypeError:
            raise TypeError('arguments 1-3 must be sequences') from None
        lists.append(seq)
    fds = []
    for seq in lists:
        row = []
        for o in seq:
            fd = _fileno(o)
            if fd >= _FD_SETSIZE:
                raise ValueError('filedescriptor out of range in select()')
            row.append(fd)
        fds.append(row)
    return lists, fds


def select(rlist, wlist, xlist, timeout=None):
    timeout = _seconds(timeout)
    (rlist, wlist, xlist), (rfds, wfds, xfds) = _split(rlist, wlist, xlist)
    entries = {}
    for fds, events in ((rfds, POLLIN), (wfds, POLLOUT), (xfds, POLLPRI)):
        for fd in fds:
            entries[fd] = entries.get(fd, 0) | events
    result = []

    def scan():
        revents = _kernel_revents(entries, 0.0) if entries else {}
        out = []
        for objs, fds, mask in (
                (rlist, rfds, POLLIN | POLLHUP | POLLERR),
                (wlist, wfds, POLLOUT | POLLERR),
                (xlist, xfds, POLLPRI)):
            out.append([o for o, fd in zip(objs, fds) if revents[fd] & mask])
        if any(out):
            result[:] = [out]
            return True
        return False

    if timeout == 0.0:
        scan()
    else:
        _await(scan, entries, timeout, 'select()')
    return tuple(result[0]) if result else ([], [], [])


# ---- poll() ----

class poll:
    # O objeto de `select.poll()`: o `poll(2)` sobre os descritores registrados, na ordem do registro.

    __module__ = 'select'

    def register(self, *args, **kwargs):
        fd, mask = _unpack('register', ('fd', 'eventmask'), 1, args, kwargs, (POLLIN | POLLPRI | POLLOUT,))
        fd = _fileno(fd)
        self._fds[fd] = _ushort(mask)

    def modify(self, *args, **kwargs):
        fd, mask = _unpack('modify', ('fd', 'eventmask'), 2, args, kwargs)
        fd = _fileno(fd)
        mask = _ushort(mask)
        if fd not in self._fds:
            raise OSError(_errno.ENOENT, 'No such file or directory')
        self._fds[fd] = mask

    def unregister(self, *args):
        fd = _fileno(_only('select.poll.unregister', args))
        try:
            del self._fds[fd]
        except KeyError:
            raise KeyError(fd) from None

    def poll(self, *args):
        (timeout_obj,) = _positional('poll', args, 0, 1)
        ms = _timeout_ms(None if timeout_obj is _MISSING else timeout_obj, 1000000)
        if self._busy:
            raise RuntimeError('concurrent poll() invocation')
        entries = dict(self._fds)
        ready = []

        def scan():
            revents = _os.poll(list(entries.items()), 0.0)
            ready[:] = [(fd, rev) for fd, rev in zip(entries, revents) if rev]
            return bool(ready)

        self._busy = True
        try:
            if ms == 0:
                scan()
            else:
                _await(scan, entries, None if ms < 0 else ms / 1000, 'poll()')
        finally:
            self._busy = False
        return list(ready)


def _new_poll_object():
    obj = object.__new__(_PollObject)
    obj._fds = {}
    obj._busy = False
    return obj


_PollObject = poll


def poll(*args):
    if args:
        raise TypeError('select.poll() takes no arguments (%d given)' % len(args))
    return _new_poll_object()


# ---- epoll ----

class _FdGuard:
    """Dono do fd de um `epoll`: fecha na coleta do objeto, como o dealloc do CPython."""

    def __init__(self, fd):
        self.fd = fd

    def close(self):
        fd = self.fd
        self.fd = -1
        if fd >= 0:
            _os.close(fd)

    def __del__(self):
        try:
            self.close()
        except Exception:
            pass


class epoll:
    """select.epoll(sizehint=-1, flags=0)

Returns an epolling object

sizehint must be a positive integer or -1 for the default size. The
sizehint is used to optimize internal data structures. It doesn't limit
the maximum number of monitored events."""

    __module__ = 'select'

    def __new__(cls, *args, **kwargs):
        # O `tp_new` do CPython conta só os posicionais nesta mensagem (medido no Debian 13).
        if len(args) > 2:
            raise TypeError('epoll() takes at most 2 arguments (%d given)' % len(args))
        sizehint, flags = _unpack('epoll', ('sizehint', 'flags'), 0, args, kwargs, (-1, 0))
        sizehint = _cint(sizehint)
        flags = _cint(flags)
        if sizehint != -1 and sizehint <= 0:
            raise ValueError('negative sizehint')
        if flags and flags != EPOLL_CLOEXEC:
            raise OSError('invalid flags')
        obj = object.__new__(cls)
        obj._guard = _FdGuard(_os.epoll_create())
        return obj

    @classmethod
    def fromfd(cls, *args):
        fd = _cint(_only('select.epoll.fromfd', args))
        obj = object.__new__(cls)
        obj._guard = _FdGuard(fd)
        return obj

    def _fd(self):
        fd = self._guard.fd
        if fd < 0:
            raise ValueError('I/O operation on closed epoll object')
        return fd

    def _ctl(self, op, fd, events):
        _os.epoll_ctl(self._fd(), op, fd, events)

    @property
    def closed(self):
        return self._guard.fd < 0

    def close(self):
        self._guard.close()

    def fileno(self):
        return self._fd()

    def register(self, *args, **kwargs):
        fd, mask = _unpack('register', ('fd', 'eventmask'), 1, args, kwargs, (EPOLLIN | EPOLLPRI | EPOLLOUT,))
        self._ctl(_CTL_ADD, _fileno(fd), _uint(mask))

    def modify(self, *args, **kwargs):
        fd, mask = _unpack('modify', ('fd', 'eventmask'), 2, args, kwargs)
        self._ctl(_CTL_MOD, _fileno(fd), _uint(mask))

    def unregister(self, *args, **kwargs):
        (fd,) = _unpack('unregister', ('fd',), 1, args, kwargs)
        self._ctl(_CTL_DEL, _fileno(fd), 0)

    def poll(self, *args, **kwargs):
        timeout_obj, maxevents = _unpack('poll', ('timeout', 'maxevents'), 0, args, kwargs, (None, -1))
        maxevents = _cint(maxevents)
        ms = _timeout_ms(timeout_obj, 1000000000)
        if maxevents == -1:
            maxevents = _FD_SETSIZE - 1
        elif maxevents < 1:
            raise ValueError('maxevents must be greater than 0, got %d' % maxevents)
        epfd = self._fd()
        ready = []

        def scan():
            ready[:] = _os.epoll_wait(epfd, maxevents, 0.0)
            return bool(ready)

        if ms == 0:
            scan()
        else:
            _await(scan, {epfd: POLLIN}, None if ms < 0 else ms / 1000, 'epoll.poll()')
        return list(ready)

    def __enter__(self):
        self._fd()
        return self

    def __exit__(self, exc_type=None, exc_value=None, exc_tb=None):
        self.close()


# No CPython `select()` e `poll()` são funções embutidas (C): guardadas num atributo de classe não viram método.
select = _sys._builtin(select)
poll = _sys._builtin(poll)
select.__module__ = 'select'
poll.__module__ = 'select'
