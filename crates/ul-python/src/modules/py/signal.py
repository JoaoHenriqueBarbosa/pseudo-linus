"""Módulo `signal`: números e nomes do Linux, tabela de tratadores e `raise_signal`.

Não há entrega assíncrona de sinais ao programa Python: `signal.signal` registra o tratador e
`raise_signal` o chama de forma síncrona, como o CPython faz para o próprio processo.
"""

import enum as _enum
import _os

_NAMES = [
    ('SIGHUP', 1), ('SIGINT', 2), ('SIGQUIT', 3), ('SIGILL', 4), ('SIGTRAP', 5), ('SIGABRT', 6),
    ('SIGBUS', 7), ('SIGFPE', 8), ('SIGKILL', 9), ('SIGUSR1', 10), ('SIGSEGV', 11), ('SIGUSR2', 12),
    ('SIGPIPE', 13), ('SIGALRM', 14), ('SIGTERM', 15), ('SIGSTKFLT', 16), ('SIGCHLD', 17),
    ('SIGCONT', 18), ('SIGSTOP', 19), ('SIGTSTP', 20), ('SIGTTIN', 21), ('SIGTTOU', 22),
    ('SIGURG', 23), ('SIGXCPU', 24), ('SIGXFSZ', 25), ('SIGVTALRM', 26), ('SIGPROF', 27),
    ('SIGWINCH', 28), ('SIGIO', 29), ('SIGPWR', 30), ('SIGSYS', 31),
]

Signals = _enum.IntEnum('Signals', _NAMES + [('SIGIOT', 6), ('SIGPOLL', 29), ('SIGCLD', 17)])
_SIGNUMS = frozenset(v for _, v in _NAMES)


class Handlers(_enum.IntEnum):
    SIG_DFL = 0
    SIG_IGN = 1


SIG_DFL = Handlers.SIG_DFL
SIG_IGN = Handlers.SIG_IGN
SIG_BLOCK = 0
SIG_UNBLOCK = 1
SIG_SETMASK = 2
NSIG = 65

SIGHUP = Signals.SIGHUP
SIGINT = Signals.SIGINT
SIGQUIT = Signals.SIGQUIT
SIGILL = Signals.SIGILL
SIGTRAP = Signals.SIGTRAP
SIGABRT = Signals.SIGABRT
SIGBUS = Signals.SIGBUS
SIGFPE = Signals.SIGFPE
SIGKILL = Signals.SIGKILL
SIGUSR1 = Signals.SIGUSR1
SIGSEGV = Signals.SIGSEGV
SIGUSR2 = Signals.SIGUSR2
SIGPIPE = Signals.SIGPIPE
SIGALRM = Signals.SIGALRM
SIGTERM = Signals.SIGTERM
SIGSTKFLT = Signals.SIGSTKFLT
SIGCHLD = Signals.SIGCHLD
SIGCONT = Signals.SIGCONT
SIGSTOP = Signals.SIGSTOP
SIGTSTP = Signals.SIGTSTP
SIGTTIN = Signals.SIGTTIN
SIGTTOU = Signals.SIGTTOU
SIGURG = Signals.SIGURG
SIGXCPU = Signals.SIGXCPU
SIGXFSZ = Signals.SIGXFSZ
SIGVTALRM = Signals.SIGVTALRM
SIGPROF = Signals.SIGPROF
SIGWINCH = Signals.SIGWINCH
SIGIO = Signals.SIGIO
SIGPWR = Signals.SIGPWR
SIGSYS = Signals.SIGSYS
SIGIOT = Signals.SIGABRT
SIGPOLL = Signals.SIGIO
SIGCLD = Signals.SIGCHLD
SIGRTMIN = 34
SIGRTMAX = 64


def default_int_handler(signalnum, frame):
    raise KeyboardInterrupt


_handlers = {Signals.SIGINT: default_int_handler}


def _check(signalnum):
    if not isinstance(signalnum, int):
        raise TypeError("an integer is required (got type %s)" % type(signalnum).__name__)
    if not 1 <= signalnum < NSIG:
        raise ValueError("signal number out of range")


def signal(signalnum, handler):
    _check(signalnum)
    if signalnum in (Signals.SIGKILL, Signals.SIGSTOP):
        raise OSError(22, 'Invalid argument')
    if not callable(handler) and handler not in (SIG_IGN, SIG_DFL):
        raise TypeError('signal handler must be signal.SIG_IGN, signal.SIG_DFL, or a callable object')
    old = getsignal(signalnum)
    if handler == SIG_IGN and not callable(handler):
        _os._sigaction(int(signalnum), 1)
    elif handler == SIG_DFL and not callable(handler):
        _os._sigaction(int(signalnum), 0)
    else:
        _os._sigaction(int(signalnum), 2)
    _handlers[signalnum] = handler
    return old


_delivered = 0


def _dispatch(signums):
    """Chamado pela VM com os sinais capturados que chegaram: roda o tratador de cada um."""
    global _delivered
    import sys
    try:
        frame = sys._getframe(1)
    except (AttributeError, ValueError):
        frame = None
    for n in signums:
        _delivered += 1
        handler = _handlers.get(n, SIG_DFL)
        if callable(handler):
            handler(n, frame)


def getsignal(signalnum):
    _check(signalnum)
    return _handlers.get(signalnum, SIG_DFL)


def raise_signal(signalnum):
    _check(signalnum)
    handler = getsignal(signalnum)
    if handler is SIG_IGN or handler == 1 and not callable(handler):
        return
    if callable(handler):
        handler(signalnum, None)
        return
    if signalnum in (Signals.SIGCHLD, Signals.SIGCONT, Signals.SIGURG, Signals.SIGWINCH):
        return
    import sys
    sys.stdout.flush()
    sys.stderr.flush()
    raise SystemExit(128 + int(signalnum))


def strsignal(signalnum):
    _check(signalnum)
    texts = {
        1: 'Hangup', 2: 'Interrupt', 3: 'Quit', 4: 'Illegal instruction', 5: 'Trace/breakpoint trap',
        6: 'Aborted', 7: 'Bus error', 8: 'Floating point exception', 9: 'Killed',
        10: 'User defined signal 1', 11: 'Segmentation fault', 12: 'User defined signal 2',
        13: 'Broken pipe', 14: 'Alarm clock', 15: 'Terminated', 17: 'Child exited',
        18: 'Continued', 19: 'Stopped (signal)', 20: 'Stopped',
    }
    return texts.get(int(signalnum))


def valid_signals():
    return {Signals(n) if n in [v for _, v in _NAMES] else n for n in range(1, NSIG)}


def alarm(seconds):
    """Agenda um SIGALRM para daqui a `seconds` (0 cancela). Devolve os segundos que faltavam do alarme anterior."""
    if not isinstance(seconds, int):
        raise TypeError("'%s' object cannot be interpreted as an integer" % type(seconds).__name__)
    return _os._alarm(seconds)


def pause():
    # Espera chegar um sinal capturado; o tratador roda dentro do `sleep` (a VM entrega ao acordar).
    import time
    before = _delivered
    while _delivered == before:
        time.sleep(0.01)


def siginterrupt(signalnum, flag):
    _check(signalnum)


def set_wakeup_fd(fd, *, warn_on_full_buffer=True):
    return -1


def pthread_sigmask(how, mask):
    return set()


def sigpending():
    return set()
