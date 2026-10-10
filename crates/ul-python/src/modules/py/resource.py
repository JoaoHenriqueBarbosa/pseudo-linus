"""resource do sandbox: limites e consumo do processo (valores do sandbox, sem limites reais)."""

import time as _time

RLIMIT_CPU = 0
RLIMIT_FSIZE = 1
RLIMIT_DATA = 2
RLIMIT_STACK = 3
RLIMIT_CORE = 4
RLIMIT_RSS = 5
RLIMIT_NPROC = 6
RLIMIT_NOFILE = 7
RLIMIT_OFILE = 7
RLIMIT_MEMLOCK = 8
RLIMIT_AS = 9
RLIMIT_SIGPENDING = 11
RLIMIT_MSGQUEUE = 12
RLIMIT_NICE = 13
RLIMIT_RTPRIO = 14
RLIMIT_RTTIME = 15
RLIM_INFINITY = -1
RUSAGE_SELF = 0
RUSAGE_CHILDREN = -1
RUSAGE_THREAD = 1

error = OSError

# Os limites vivem no kernel (`RLIMIT_NPROC` e `RLIMIT_NOFILE` valem de verdade); o `_os` só os mostra e grava.
from _os import getrlimit, setrlimit


def prlimit(pid, resource, limits=None):
    import errno
    import operator
    import os
    pid = operator.index(pid)
    if pid != 0 and pid != os.getpid():
        if pid < 0:
            raise ProcessLookupError(errno.ESRCH, os.strerror(errno.ESRCH))
        os.kill(pid, 0)
        # O kernel do sandbox só tem os limites do processo que chama.
        raise PermissionError(errno.EPERM, os.strerror(errno.EPERM))
    old = getrlimit(resource)
    if limits is not None:
        setrlimit(resource, limits)
    return old


class struct_rusage(tuple):
    _fields = ('ru_utime', 'ru_stime', 'ru_maxrss', 'ru_ixrss', 'ru_idrss', 'ru_isrss', 'ru_minflt', 'ru_majflt',
               'ru_nswap', 'ru_inblock', 'ru_oublock', 'ru_msgsnd', 'ru_msgrcv', 'ru_nsignals', 'ru_nvcsw',
               'ru_nivcsw')

    def __new__(cls, values):
        return tuple.__new__(cls, values)

    def __getattr__(self, name):
        try:
            return self[self._fields.index(name)]
        except ValueError:
            raise AttributeError(name) from None

    def __repr__(self):
        return 'resource.struct_rusage(%s)' % ', '.join('%s=%r' % kv for kv in zip(self._fields, self))


def _rusage(utime, stime, maxrss):
    """O `struct_rusage` com o que o sandbox mede (CPU e pico de memória); o resto do `rusage` fica em zero."""
    return struct_rusage((utime, stime, maxrss) + (0,) * 13)


def getrusage(who):
    if who not in (RUSAGE_SELF, RUSAGE_CHILDREN, RUSAGE_THREAD):
        raise ValueError('invalid who parameter')
    if who == RUSAGE_CHILDREN:
        import _os
        return _rusage(*_os.child_rusage())
    return _rusage(_time.process_time(), 0.0, 16384)


def getpagesize():
    return 4096
