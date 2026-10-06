"""select do sandbox: prontidão dos sockets em loopback (`_socket`). Descritores que não são sockets (arquivos,
terminal) contam como sempre prontos, como arquivos comuns no Linux. Sem `poll`/`epoll`, como numa plataforma
que não os tem: o `selectors` cai em `select()`."""

import _socket
import _sys

error = OSError

POLLIN = 1
POLLPRI = 2
POLLOUT = 4
POLLERR = 8
POLLHUP = 16
POLLNVAL = 32


def _fileno(obj):
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


def _readable(fd):
    st = _socket._fds.get(fd)
    if st is None:
        return True
    if st.listener is not None:
        return st.listener.readable()
    if st.endpoint is not None:
        return st.endpoint.readable()
    if st.dgram is not None:
        return st.dgram.readable()
    return False


def select(rlist, wlist, xlist, timeout=None):
    rlist, wlist, xlist = list(rlist), list(wlist), list(xlist)
    rfds = [_fileno(o) for o in rlist]
    wfds = [_fileno(o) for o in wlist]
    for o in xlist:
        _fileno(o)
    if timeout is not None:
        timeout = float(timeout)
        if timeout < 0:
            raise ValueError('timeout must be non-negative')

    def ready():
        return any(_readable(fd) for fd in rfds) or any(not _closed(fd) for fd in wfds)

    if not (rlist or wlist or xlist) and timeout is None:
        raise ValueError('select() without descriptors and no timeout would block forever')
    if not ready():
        _socket._wait(ready, timeout if timeout is not None else None, 'select()')
    return ([o for o, fd in zip(rlist, rfds) if _readable(fd)],
            [o for o, fd in zip(wlist, wfds) if not _closed(fd)],
            [])


def _closed(fd):
    st = _socket._fds.get(fd)
    return st is not None and st.closed


_sys._builtin(select)
