"""Coletor de lixo: a VM usa contagem de referências, então não há ciclos a varrer."""

import _sys

# Os `__del__` não dependem do coletor: rodam quando a última referência cai. Objetos que só se
# alcançam em ciclo não têm referência final para cair e ficam sem finalizar até a saída do
# interpretador, que finaliza tudo o que ainda vive. `collect()` não detecta ciclos: não finaliza nem
# libera nada e devolve 0.

DEBUG_STATS = 1
DEBUG_COLLECTABLE = 2
DEBUG_UNCOLLECTABLE = 4
DEBUG_SAVEALL = 32
DEBUG_LEAK = 38

garbage = []
callbacks = []
_debug = 0
_sys._gc_bind(garbage, callbacks, 0)


def collect(generation=2):
    if not isinstance(generation, int):
        raise TypeError("'%s' object cannot be interpreted as an integer" % type(generation).__name__)
    return _sys._gc_collect(generation)


def enable():
    _sys._gc_config(True, None)


def disable():
    _sys._gc_config(False, None)


def isenabled():
    return _sys._gc_config(None, None)[0]


def get_count():
    return _sys._gc_count()


def get_threshold():
    return _sys._gc_config(None, None)[1]


def set_threshold(threshold0, threshold1=None, threshold2=None):
    current = get_threshold()
    _sys._gc_config(None, (threshold0,
                           current[1] if threshold1 is None else threshold1,
                           current[2] if threshold2 is None else threshold2))


def get_debug():
    return _debug


def set_debug(flags):
    global _debug
    _debug = flags
    _sys._gc_bind(None, None, flags)


def get_objects(generation=None):
    return _sys._gc_objects(generation)


def get_referrers(*objs):
    return _sys._gc_referrers(objs)


def get_referents(*objs):
    referents = []
    for obj in objs:
        referents.extend(_sys._referents(obj))
    return referents


def is_tracked(obj):
    return _sys._gc_is_tracked(obj)


def is_finalized(obj):
    return _sys._gc_is_finalized(obj)


def freeze():
    _sys._gc_freeze(True)


def unfreeze():
    _sys._gc_freeze(False)


def get_freeze_count():
    return _sys._gc_freeze(None)


def get_stats():
    return [{'collections': c, 'collected': f, 'uncollectable': 0} for c, f in _sys._gc_stats()]
