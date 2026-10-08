"""faulthandler: o sandbox não tem sinais fatais para tratar; as funções existem e não fazem nada de útil."""

import sys

_enabled = False


def enable(file=None, all_threads=True):
    global _enabled
    _enabled = True


def disable():
    global _enabled
    was = _enabled
    _enabled = False
    return was


def is_enabled():
    return _enabled


def dump_traceback(file=None, all_threads=True):
    import traceback
    out = file if file is not None and not isinstance(file, int) else sys.stderr
    out.write('Current thread 0x00007f0000000000 (most recent call first):\n')
    for line in traceback.format_stack()[::-1]:
        out.write(line)


def dump_traceback_later(timeout, repeat=False, file=None, exit=False):
    pass


def cancel_dump_traceback_later():
    pass


def register(signum, file=None, all_threads=True, chain=False):
    pass


def unregister(signum):
    return False


def _die(signum, description, always=False):
    """O fim de uma falha fatal provocada de propósito: com o tratador ligado (ou `always`, o `Py_FatalError`) o
    `faulthandler` escreve o relatório em `stderr` e, restaurada a ação padrão, o processo morre pelo próprio sinal
    (`os.kill` em si mesmo), como um `SIGSEGV` de verdade."""
    import os
    import signal
    if _enabled or always:
        sys.stderr.write('Fatal Python error: %s\n\n' % description)
        dump_traceback()
        sys.stderr.flush()
    signal.signal(signum, signal.SIG_DFL)
    os.kill(os.getpid(), signum)


def _sigsegv(release_gil=False, /):
    _die(11, 'Segmentation fault')


def _read_null():
    _die(11, 'Segmentation fault')


def _stack_overflow():
    _die(11, 'Segmentation fault')


def _sigabrt():
    _die(6, 'Aborted')


def _sigfpe():
    _die(8, 'Floating-point exception')


def _fatal_error_c_thread():
    # `Py_FatalError("in new thread")` numa thread de C: o relatório vai sempre, e o fim é o `abort()`.
    _die(6, '_fatal_error_c_thread: in new thread', always=True)
