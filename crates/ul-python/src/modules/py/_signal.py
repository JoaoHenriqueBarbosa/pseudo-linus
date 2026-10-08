"""Módulo `_signal`: números e tabela de tratadores do Linux, `raise_signal` e os temporizadores.

Não há entrega assíncrona de sinais ao programa Python: `signal.signal` registra o tratador e
`raise_signal` o chama de forma síncrona, como o CPython faz para o próprio processo. O `signal.py`
do Debian, por cima deste módulo, converte os números e os tratadores nos `IntEnum` públicos.
"""

import _os
from collections import namedtuple as _namedtuple

NSIG = 65

SIGHUP = 1
SIGINT = 2
SIGQUIT = 3
SIGILL = 4
SIGTRAP = 5
SIGABRT = 6
SIGIOT = 6
SIGBUS = 7
SIGFPE = 8
SIGKILL = 9
SIGUSR1 = 10
SIGSEGV = 11
SIGUSR2 = 12
SIGPIPE = 13
SIGALRM = 14
SIGTERM = 15
SIGSTKFLT = 16
SIGCHLD = 17
SIGCLD = 17
SIGCONT = 18
SIGSTOP = 19
SIGTSTP = 20
SIGTTIN = 21
SIGTTOU = 22
SIGURG = 23
SIGXCPU = 24
SIGXFSZ = 25
SIGVTALRM = 26
SIGPROF = 27
SIGWINCH = 28
SIGIO = 29
SIGPOLL = 29
SIGPWR = 30
SIGSYS = 31
SIGRTMIN = 34
SIGRTMAX = 64

SIG_DFL = 0
SIG_IGN = 1
SIG_BLOCK = 0
SIG_UNBLOCK = 1
SIG_SETMASK = 2

ITIMER_REAL = 0
ITIMER_VIRTUAL = 1
ITIMER_PROF = 2

# O erro de `setitimer` e `getitimer` (`EINVAL` com `which` ou tempo inválido): no CPython a classe se
# chama `signal.itimer_error` e o módulo a expõe como `ItimerError`.
ItimerError = type('itimer_error', (OSError,), {'__module__': 'signal'})

_siginfo_base = _namedtuple('struct_siginfo', 'si_signo si_code si_errno si_pid si_uid si_status si_band')


class struct_siginfo(_siginfo_base):
    """struct_siginfo: Result from sigwaitinfo or sigtimedwait.

This object may be accessed either as a tuple of
(si_signo, si_code, si_errno, si_pid, si_uid, si_status, si_band),
or via the attributes si_signo, si_code, and so on."""

    __slots__ = ()
    __module__ = 'signal'

    def __repr__(self):
        return 'signal.' + _siginfo_base.__repr__(self)


def default_int_handler(signalnum, frame, /):
    """The default handler for SIGINT installed by Python.

It raises KeyboardInterrupt."""
    raise KeyboardInterrupt


# Os tratadores por número: o que o interpretador instala na partida (o `default_int_handler` no SIGINT, e os
# sinais que ele ignora).
_handlers = {SIGINT: default_int_handler, SIGPIPE: SIG_IGN, SIGXFSZ: SIG_IGN}
_delivered = 0
_wakeup_fd = -1
_wakeup_warn = True
# `siginterrupt(sig, flag)`: o `SA_RESTART` guardado por sinal (só `signal.signal` o apaga, como o `sigaction`).
_restart = {}
# A máscara da thread (`pthread_sigmask`) e os sinais levantados enquanto bloqueados.
_blocked = set()
_pending = set()


def _int_arg(value):
    """O `int` do clinic: `TypeError` para o que não é inteiro, `OverflowError` fora da faixa do `int` de C."""
    if not isinstance(value, int):
        try:
            index = type(value).__index__
        except AttributeError:
            raise TypeError("'%s' object cannot be interpreted as an integer" % type(value).__name__) from None
        value = index(value)
    if not -2147483648 <= value <= 2147483647:
        raise OverflowError('Python int too large to convert to C int')
    # Um membro de `IntEnum` entra como o inteiro puro: as chaves das tabelas daqui são inteiros.
    return int(value)


def _signum(value):
    """O número de sinal válido de `signalnum`: `ValueError` fora de 1..NSIG-1."""
    value = _int_arg(value)
    if not 1 <= value < NSIG:
        raise ValueError('signal number out of range')
    return value


def _is_default(handler):
    return type(handler) is int and handler == SIG_DFL


def _is_ignore(handler):
    return type(handler) is int and handler == SIG_IGN


def _main_thread():
    import sys
    threading = sys.modules.get('threading')
    return threading is None or threading.current_thread() is threading.main_thread()


def signal(signalnum, handler, /):
    """Set the action for the given signal.

The action can be SIG_DFL, SIG_IGN, or a callable Python object.
The previous action is returned.  See getsignal() for possible return values.

*** IMPORTANT NOTICE ***
A signal handler function is called with two arguments:
the first is the signal number, the second is the interrupted stack frame."""
    signalnum = _int_arg(signalnum)
    if not (_is_default(handler) or _is_ignore(handler) or callable(handler)):
        raise TypeError('signal handler must be signal.SIG_IGN, signal.SIG_DFL, or a callable object')
    if not _main_thread():
        raise ValueError('signal only works in main thread of the main interpreter')
    if not 1 <= signalnum < NSIG:
        raise ValueError('signal number out of range')
    if signalnum in (SIGKILL, SIGSTOP):
        raise OSError(22, 'Invalid argument')
    old = _handlers.get(signalnum, SIG_DFL)
    if _is_ignore(handler):
        _os._sigaction(signalnum, 1)
    elif _is_default(handler):
        _os._sigaction(signalnum, 0)
    else:
        _os._sigaction(signalnum, 2)
    _handlers[signalnum] = handler
    _restart.pop(signalnum, None)
    return old


def getsignal(signalnum, /):
    """Return the current action for the given signal.

The return value can be:
  SIG_IGN -- if the signal is being ignored
  SIG_DFL -- if the default action for the signal is in effect
  None    -- if an unknown handler is in effect
  anything else -- the callable Python object used as a handler"""
    return _handlers.get(_signum(signalnum), SIG_DFL)


def _write_wakeup(signalnum):
    """O `trip_signal` do CPython: o byte com o número do sinal vai ao fd de `set_wakeup_fd` na chegada. Erro de
    escrita não sobe: o `EWOULDBLOCK` só aparece se `warn_on_full_buffer`, o resto sempre, ao `sys.unraisablehook`."""
    if _wakeup_fd == -1:
        return
    try:
        _os.write(_wakeup_fd, bytes([signalnum]))
    except OSError as e:
        if e.errno == 11 and not _wakeup_warn:
            return
        import sys
        import _unraisable
        msg = 'Exception ignored when trying to write to the signal wakeup fd'
        sys.unraisablehook(_unraisable.UnraisableHookArgs((type(e), e, None, msg, None)))


def _dispatch(signums):
    """Chamado pela VM com os sinais capturados que chegaram: roda o tratador de cada um."""
    global _delivered
    import sys
    try:
        # O quadro Python em execução na hora da chegada (o código embutido não conta como quadro).
        frame = sys._getframe(0)
    except (AttributeError, ValueError):
        frame = None
    # A chegada de todos (o handler em C) vem antes de qualquer tratador em Python.
    for n in signums:
        _write_wakeup(n)
    for n in signums:
        _delivered += 1
        if n in _blocked:
            _pending.add(n)
            continue
        handler = _handlers.get(n, SIG_DFL)
        if callable(handler):
            handler(n, frame)


def raise_signal(signalnum, /):
    """Send a signal to the executing process."""
    signalnum = _signum(signalnum)
    if signalnum in _blocked:
        _pending.add(signalnum)
        return
    handler = _handlers.get(signalnum, SIG_DFL)
    if _is_ignore(handler):
        return
    if callable(handler):
        _write_wakeup(signalnum)
        handler(signalnum, None)
        return
    if signalnum in (SIGCHLD, SIGCONT, SIGURG, SIGWINCH):
        return
    import sys
    sys.stdout.flush()
    sys.stderr.flush()
    raise SystemExit(128 + signalnum)


# O texto de `strsignal` no glibc 2.41, pelo número.
_DESCRIPTIONS = {
    1: 'Hangup', 2: 'Interrupt', 3: 'Quit', 4: 'Illegal instruction', 5: 'Trace/breakpoint trap',
    6: 'Aborted', 7: 'Bus error', 8: 'Floating point exception', 9: 'Killed',
    10: 'User defined signal 1', 11: 'Segmentation fault', 12: 'User defined signal 2',
    13: 'Broken pipe', 14: 'Alarm clock', 15: 'Terminated', 16: 'Stack fault', 17: 'Child exited',
    18: 'Continued', 19: 'Stopped (signal)', 20: 'Stopped', 21: 'Stopped (tty input)',
    22: 'Stopped (tty output)', 23: 'Urgent I/O condition', 24: 'CPU time limit exceeded',
    25: 'File size limit exceeded', 26: 'Virtual timer expired', 27: 'Profiling timer expired',
    28: 'Window changed', 29: 'I/O possible', 30: 'Power failure', 31: 'Bad system call',
}


def strsignal(signalnum, /):
    """Return the system description of the given signal.

Returns the description of signal *signalnum*, such as "Interrupt"
for :const:`SIGINT`. Returns :const:`None` if *signalnum* has no
description. Raises :exc:`ValueError` if *signalnum* is invalid."""
    signalnum = _signum(signalnum)
    text = _DESCRIPTIONS.get(signalnum)
    if text is not None:
        return text
    if SIGRTMIN <= signalnum <= SIGRTMAX:
        return 'Real-time signal %d' % (signalnum - SIGRTMIN)
    return 'Unknown signal %d' % signalnum


def valid_signals():
    """Return a set of valid signal numbers on this platform.

The signal numbers returned by this function can be safely passed to
functions like `pthread_sigmask`."""
    # O `sigfillset` do glibc deixa de fora os dois sinais que ele usa por dentro (32 e 33).
    return {n for n in range(1, NSIG) if n not in (32, 33)}


def alarm(seconds, /):
    """Arrange for SIGALRM to arrive after the given number of seconds."""
    return _os._alarm(_int_arg(seconds))


def _timeval(value):
    """O `timeval` de `_PyTime_ObjectToTimeval` com arredondamento para cima: `(segundos, microssegundos)`."""
    import math
    if isinstance(value, float):
        if value != value:
            raise ValueError('Invalid value NaN (not a number)')
        fraction, whole = math.modf(value)
        fraction = math.ceil(fraction * 1e6)
        if fraction >= 1000000:
            fraction -= 1000000
            whole += 1.0
        elif fraction < 0:
            fraction += 1000000
            whole -= 1.0
        if not -9223372036854775808.0 <= whole < 9223372036854775808.0:
            raise OverflowError('timestamp out of range for platform time_t')
        return int(whole), int(fraction)
    if not isinstance(value, int):
        try:
            index = type(value).__index__
        except AttributeError:
            raise TypeError("'%s' object cannot be interpreted as an integer" % type(value).__name__) from None
        value = index(value)
    if not -9223372036854775808 <= value < 9223372036854775808:
        raise OverflowError('timestamp out of range for platform time_t')
    return value, 0


def _itimer_result(raw):
    value_sec, value_usec, interval_sec, interval_usec = raw
    return value_sec + value_usec * 1e-6, interval_sec + interval_usec * 1e-6


def setitimer(which, seconds, interval=0.0, /):
    """Sets given itimer (one of ITIMER_REAL, ITIMER_VIRTUAL or ITIMER_PROF).

The timer will fire after value seconds and after that every interval seconds.
The itimer can be cleared by setting seconds to zero.

Returns old values as a tuple: (delay, interval)."""
    which = _int_arg(which)
    value_sec, value_usec = _timeval(seconds)
    interval_sec, interval_usec = _timeval(interval)
    try:
        raw = _os._setitimer(which, value_sec, value_usec, interval_sec, interval_usec)
    except OSError as e:
        raise ItimerError(e.errno, e.strerror) from None
    return _itimer_result(raw)


def getitimer(which, /):
    """Returns current value of given itimer."""
    try:
        raw = _os._getitimer(_int_arg(which))
    except OSError as e:
        raise ItimerError(e.errno, e.strerror) from None
    return _itimer_result(raw)


def pause():
    """Wait until a signal arrives."""
    # Espera chegar um sinal capturado; o tratador roda dentro do `sleep` (a VM entrega ao acordar).
    import time
    before = _delivered
    while _delivered == before:
        time.sleep(0.01)


def siginterrupt(signalnum, flag, /):
    """Change system call restart behaviour.

If flag is False, system calls will be restarted when interrupted by
signal sig, else system calls will be interrupted."""
    signalnum = _signum(signalnum)
    flag = _int_arg(flag)
    if signalnum in (SIGKILL, SIGSTOP):
        raise OSError(22, 'Invalid argument')
    _restart[signalnum] = not flag


def set_wakeup_fd(fd, /, *, warn_on_full_buffer=True):
    """Sets the fd to be written to (with the signal number) when a signal comes in.

A library can use this to wakeup select or poll.
The previous fd or -1 is returned.

The fd must be non-blocking."""
    global _wakeup_fd, _wakeup_warn
    fd = _int_arg(fd)
    if not _main_thread():
        raise ValueError('set_wakeup_fd only works in main thread of the main interpreter')
    if fd != -1 and _os.get_blocking(fd):
        raise ValueError('the fd %i must be in non-blocking mode' % fd)
    old = _wakeup_fd
    _wakeup_fd = fd
    _wakeup_warn = bool(warn_on_full_buffer)
    return old


def _sigset(mask):
    """O conjunto de números de um iterável de sinais, como o `iterable_to_sigset` do CPython."""
    out = set()
    for item in mask:
        n = _int_arg(item)
        if not 1 <= n < NSIG:
            raise ValueError('signal number %d out of range [1; %d]' % (n, NSIG - 1))
        out.add(n)
    return out


def pthread_sigmask(how, mask, /):
    """Fetch and/or change the signal mask of the calling thread."""
    how = _int_arg(how)
    wanted = _sigset(mask)
    wanted.discard(SIGKILL)
    wanted.discard(SIGSTOP)
    if how == SIG_BLOCK:
        new = _blocked | wanted
    elif how == SIG_UNBLOCK:
        new = _blocked - wanted
    elif how == SIG_SETMASK:
        new = wanted
    else:
        raise OSError(22, 'Invalid argument')
    old = set(_blocked)
    _blocked.clear()
    _blocked.update(new)
    # Os sinais que ficaram desbloqueados chegam agora.
    ready = sorted(n for n in _pending if n not in _blocked)
    for n in ready:
        _pending.discard(n)
    if ready:
        _dispatch(ready)
    return old


def sigpending():
    """Examine pending signals.

Returns a set of signal numbers that are pending for delivery to
the calling thread."""
    return set(_pending)


def _wait_pending(mask, timeout):
    """O menor sinal de `mask` pendente; espera até `timeout` segundos (`None` espera sem limite)."""
    import time
    deadline = None if timeout is None else time.monotonic() + timeout
    while True:
        ready = sorted(n for n in _pending if n in mask)
        if ready:
            _pending.discard(ready[0])
            return ready[0]
        if deadline is not None and time.monotonic() >= deadline:
            return None
        time.sleep(0.01)


def _siginfo(signalnum):
    """O `struct_siginfo` de um sinal levantado pelo próprio processo (`SI_TKILL`)."""
    import os
    return struct_siginfo(signalnum, -6, 0, os.getpid(), os.getuid(), 0, 0)


def sigwait(sigset, /):
    """Wait for a signal.

Suspend execution of the calling thread until the delivery of one of the
signals specified in the signal set sigset.  The function accepts the signal
and returns the signal number."""
    return _wait_pending(_sigset(sigset), None)


def sigwaitinfo(sigset, /):
    """Wait synchronously until one of the signals in *sigset* is delivered.

Returns a struct_siginfo containing information about the signal."""
    return _siginfo(_wait_pending(_sigset(sigset), None))


def sigtimedwait(sigset, timeout, /):
    """Like sigwaitinfo(), but with a timeout.

The timeout is specified in seconds, with floating-point numbers allowed."""
    mask = _sigset(sigset)
    if isinstance(timeout, float):
        if timeout != timeout:
            raise ValueError('Invalid value NaN (not a number)')
    elif not isinstance(timeout, int):
        raise TypeError("'%s' object cannot be interpreted as an integer or float" % type(timeout).__name__)
    if timeout < 0:
        raise ValueError('timeout must be non-negative')
    found = _wait_pending(mask, timeout)
    return None if found is None else _siginfo(found)


def pthread_kill(thread_id, signalnum, /):
    """Send a signal to a thread."""
    import sys
    import _thread
    if not isinstance(thread_id, int):
        thread_id = _int_arg(thread_id)
    signalnum = _int_arg(signalnum)
    threading = sys.modules.get('threading')
    alive = {t.ident for t in threading.enumerate()} if threading is not None else set()
    if thread_id != _thread.get_ident() and thread_id not in alive:
        raise OSError(3, 'No such process')
    if signalnum == 0:
        return
    if not 1 <= signalnum < NSIG:
        raise OSError(22, 'Invalid argument')
    raise_signal(signalnum)


def pidfd_send_signal(pidfd, signalnum, siginfo=None, flags=0, /):
    """Send a signal to a process referred to by a pid file descriptor."""
    pidfd = _int_arg(pidfd)
    signalnum = _int_arg(signalnum)
    flags = _int_arg(flags)
    if siginfo is not None:
        raise TypeError('siginfo must be None')
    if flags != 0 or not 0 <= signalnum < NSIG:
        raise OSError(22, 'Invalid argument')
    _os.fstat(pidfd)
    raise OSError(22, 'Invalid argument')
