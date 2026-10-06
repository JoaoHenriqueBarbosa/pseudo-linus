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
