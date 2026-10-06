"""copy do sandbox (Python embutido): cópia rasa e profunda."""

__all__ = ['Error', 'copy', 'deepcopy']


class Error(Exception):
    pass


error = Error

_ATOMIC = (type(None), int, float, bool, str, bytes, range, type, complex if False else int)


def _is_atomic(x):
    if x is None or isinstance(x, (int, float, bool, str, bytes, range, type)):
        return type(x).__name__ in ('NoneType', 'int', 'float', 'bool', 'str', 'bytes', 'range') or isinstance(x, type)
    return type(x).__name__ in ('function', 'builtin_function_or_method', 'module', 'method')


def _payload_new(cls):
    """Instância vazia de `cls` já com o valor embutido vazio (subclasse de list, dict...)."""
    for base in (list, dict, set):
        if issubclass(cls, base):
            return base.__new__(cls)
    return object.__new__(cls)


def _copy_state(x, y, transform):
    for key, value in vars(x).items():
        object.__setattr__(y, key, transform(value))


def copy(x):
    cls = type(x)
    if _is_atomic(x):
        return x
    if cls is list:
        return x[:]
    if cls is dict:
        return x.copy()
    if cls is set:
        return x.copy()
    if cls is frozenset:
        return x
    if cls is tuple:
        return x
    hook = getattr(cls, '__copy__', None)
    if hook is not None:
        return hook(x)
    if isinstance(x, tuple):
        return x
    if isinstance(x, (list, dict, set)):
        y = _payload_new(cls)
        if isinstance(x, dict):
            y.update(x)
        elif isinstance(x, list):
            y.extend(x)
        else:
            y.update(x)
        _copy_state(x, y, lambda v: v)
        return y
    if isinstance(x, (str, int, float)):
        return x
    y = object.__new__(cls)
    _copy_state(x, y, lambda v: v)
    return y


def deepcopy(x, memo=None):
    if memo is None:
        memo = {}
    key = id(x)
    if key in memo:
        return memo[key]
    cls = type(x)
    if _is_atomic(x):
        return x
    hook = getattr(cls, '__deepcopy__', None)
    if hook is not None:
        y = hook(x, memo)
        memo[key] = y
        return y
    if cls is list:
        y = []
        memo[key] = y
        for item in x:
            y.append(deepcopy(item, memo))
        return y
    if cls is dict:
        y = {}
        memo[key] = y
        for k, v in x.items():
            y[deepcopy(k, memo)] = deepcopy(v, memo)
        return y
    if cls is set:
        y = set()
        memo[key] = y
        for item in x:
            y.add(deepcopy(item, memo))
        return y
    if cls is tuple:
        y = tuple(deepcopy(item, memo) for item in x)
        memo[key] = y
        return y
    if cls is frozenset:
        y = frozenset(deepcopy(item, memo) for item in x)
        memo[key] = y
        return y
    if isinstance(x, tuple):
        items = [deepcopy(item, memo) for item in x]
        if hasattr(cls, '_make'):
            y = cls._make(items)
        else:
            y = tuple.__new__(cls, items)
        memo[key] = y
        return y
    if isinstance(x, (list, dict, set)):
        y = _payload_new(cls)
        memo[key] = y
        if isinstance(x, dict):
            for k, v in x.items():
                y[deepcopy(k, memo)] = deepcopy(v, memo)
        elif isinstance(x, list):
            for item in x:
                y.append(deepcopy(item, memo))
        else:
            for item in x:
                y.add(deepcopy(item, memo))
        _copy_state(x, y, lambda v: deepcopy(v, memo))
        return y
    if isinstance(x, (str, int, float)):
        return x
    y = object.__new__(cls)
    memo[key] = y
    _copy_state(x, y, lambda v: deepcopy(v, memo))
    return y


def replace(obj, /, **changes):
    if not hasattr(obj, '__replace__'):
        raise TypeError('replace() does not support %s objects' % type(obj).__name__)
    return obj.__replace__(**changes)
