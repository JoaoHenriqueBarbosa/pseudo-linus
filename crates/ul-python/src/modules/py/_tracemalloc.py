"""Casca do _tracemalloc: o interpretador não tem gancho de alocação, então o
rastreamento liga e desliga, mas nenhum bloco é contabilizado."""

_tracing = False
_limit = 1


def is_tracing():
    return _tracing


def start(nframe=1):
    global _tracing, _limit
    if not 1 <= nframe <= 65535:
        raise ValueError("the number of frames must be in range [1; 65535]")
    _tracing = True
    _limit = nframe


def stop():
    global _tracing
    _tracing = False


def clear_traces():
    pass


def get_traceback_limit():
    return _limit


def get_traced_memory():
    return (0, 0)


def reset_peak():
    pass


def get_tracemalloc_memory():
    return 0


def _get_traces():
    return []


def _get_object_traceback(obj):
    return None
