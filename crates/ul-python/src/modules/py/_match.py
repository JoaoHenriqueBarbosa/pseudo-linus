"""Auxiliares de runtime da instrução `match`, chamados pelo código que o compilador gera."""

_SELF_MATCH = (bool, bytearray, bytes, dict, float, frozenset, int, list, set, str, tuple)
_MISSING = object()


def _match_seq(x, n, star):
    """O sujeito é uma sequência (não texto) com `n` itens, ou ao menos `n - 1` se há estrela."""
    if isinstance(x, (str, bytes, bytearray)):
        return False
    if not isinstance(x, (list, tuple, range)):
        from collections.abc import Sequence
        if not isinstance(x, Sequence):
            return False
    size = len(x)
    if star:
        return size >= n - 1
    return size == n


def _match_item(x, i):
    return x[i]


def _match_star(x, before, after):
    size = len(x)
    return [x[i] for i in range(before, size - after)]


def _match_map(x):
    if isinstance(x, dict):
        return True
    from collections.abc import Mapping
    return isinstance(x, Mapping)


def _match_vals(x, keys):
    """Os valores das chaves, em tupla, ou None se alguma faltar."""
    out = []
    for k in keys:
        v = x.get(k, _MISSING)
        if v is _MISSING:
            return None
        out.append(v)
    return tuple(out)


def _match_rest(x, keys):
    rest = dict(x)
    for k in keys:
        rest.pop(k, None)
    return rest


def _match_class(x, cls, npos, kw):
    """Os atributos pedidos pelo padrão de classe, em tupla, ou None se não casa."""
    if not isinstance(cls, type):
        raise TypeError('called match pattern must be a class')
    if not isinstance(x, cls):
        return None
    out = []
    if npos:
        if cls in _SELF_MATCH:
            if npos > 1:
                raise TypeError('%s() accepts 1 positional sub-pattern (%d given)' % (cls.__name__, npos))
            out.append(x)
        else:
            args = getattr(cls, '__match_args__', ())
            if not isinstance(args, tuple):
                raise TypeError('%s.__match_args__ must be a tuple (got %s)' % (cls.__name__, type(args).__name__))
            if npos > len(args):
                raise TypeError('%s() accepts %d positional sub-pattern%s (%d given)'
                                % (cls.__name__, len(args), '' if len(args) == 1 else 's', npos))
            for name in args[:npos]:
                v = getattr(x, name, _MISSING)
                if v is _MISSING:
                    return None
                out.append(v)
    for name in kw:
        v = getattr(x, name, _MISSING)
        if v is _MISSING:
            return None
        out.append(v)
    return tuple(out)
