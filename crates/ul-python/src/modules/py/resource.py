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

_limits = {
    RLIMIT_CPU: (RLIM_INFINITY, RLIM_INFINITY),
    RLIMIT_FSIZE: (RLIM_INFINITY, RLIM_INFINITY),
    RLIMIT_DATA: (RLIM_INFINITY, RLIM_INFINITY),
    RLIMIT_STACK: (8388608, RLIM_INFINITY),
    RLIMIT_CORE: (RLIM_INFINITY, RLIM_INFINITY),
    RLIMIT_RSS: (RLIM_INFINITY, RLIM_INFINITY),
    RLIMIT_NPROC: (RLIM_INFINITY, RLIM_INFINITY),
    RLIMIT_NOFILE: (1048576, 1048576),
    RLIMIT_MEMLOCK: (8388608, 8388608),
    RLIMIT_AS: (RLIM_INFINITY, RLIM_INFINITY),
    # O 10 é o `RLIMIT_LOCKS` do kernel: o glibc 2.41 não o exporta, e o `resource` do Debian não tem o nome.
    10: (RLIM_INFINITY, RLIM_INFINITY),
    RLIMIT_SIGPENDING: (RLIM_INFINITY, RLIM_INFINITY),
    RLIMIT_MSGQUEUE: (819200, 819200),
    RLIMIT_NICE: (0, 0),
    RLIMIT_RTPRIO: (0, 0),
    RLIMIT_RTTIME: (RLIM_INFINITY, RLIM_INFINITY),
}


def getrlimit(resource):
    if resource not in _limits:
        raise ValueError('invalid resource specified')
    return _limits[resource]


def setrlimit(resource, limits):
    if resource not in _limits:
        raise ValueError('invalid resource specified')
    soft, hard = limits
    cur_soft, cur_hard = _limits[resource]
    if cur_hard != RLIM_INFINITY and (hard == RLIM_INFINITY or hard > cur_hard):
        raise ValueError('not allowed to raise maximum limit')
    if hard != RLIM_INFINITY and (soft == RLIM_INFINITY or soft > hard):
        raise ValueError('current limit exceeds maximum limit')
    _limits[resource] = (soft, hard)


def prlimit(pid, resource, limits=None):
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
