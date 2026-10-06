"""select do sandbox: sem descritores de rede. `select()` só espera o tempo pedido e devolve listas
vazias (nada fica pronto); `poll`/`epoll` não existem, como numa plataforma sem eles."""
import time as _time

error = OSError

POLLIN = 1
POLLPRI = 2
POLLOUT = 4
POLLERR = 8
POLLHUP = 16
POLLNVAL = 32


def select(rlist, wlist, xlist, timeout=None):
    for group in (rlist, wlist, xlist):
        for fd in group:
            if not isinstance(fd, int) and not hasattr(fd, 'fileno'):
                raise TypeError('argument must be an int, or have a fileno() method.')
    if timeout is None:
        if not (rlist or wlist or xlist):
            raise ValueError('select() without descriptors and no timeout would block forever')
        # Sem fontes de evento no sandbox, esperar para sempre não tem saída.
        raise OSError(4, 'Interrupted system call')
    if timeout < 0:
        raise ValueError('timeout must be non-negative')
    _time.sleep(timeout)
    return [], [], []
