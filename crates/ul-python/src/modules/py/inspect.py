"""inspect enxuto: predicados de tipo, `signature` e `getdoc` sobre os objetos da VM."""

import types
import sys

__all__ = ['isfunction', 'ismethod', 'isclass', 'ismodule', 'iscoroutine', 'iscoroutinefunction',
           'isgenerator', 'isgeneratorfunction', 'isasyncgen', 'isasyncgenfunction', 'isawaitable',
           'isbuiltin', 'isroutine', 'callable', 'getdoc', 'signature', 'Signature', 'Parameter',
           'getmembers', 'currentframe', 'unwrap']

callable = callable


def isfunction(obj):
    return type(obj) is type(isfunction)


def ismethod(obj):
    return type(obj).__name__ == 'method'


def isclass(obj):
    return isinstance(obj, type)


def ismodule(obj):
    return isinstance(obj, types.ModuleType)


def isbuiltin(obj):
    return type(obj).__name__ == 'builtin_function_or_method'


def isroutine(obj):
    return isbuiltin(obj) or isfunction(obj) or ismethod(obj)


def _unwrap_method(obj):
    return getattr(obj, '__func__', obj)


def iscoroutinefunction(obj):
    obj = _unwrap_method(obj)
    while hasattr(obj, '__wrapped__'):
        obj = obj.__wrapped__
    return bool(getattr(obj, '_is_coroutine_function', False)) or _code_flag(obj, 'coroutine')


def isasyncgenfunction(obj):
    return _code_flag(_unwrap_method(obj), 'asyncgen')


def isgeneratorfunction(obj):
    return _code_flag(_unwrap_method(obj), 'generator')


def _code_flag(obj, kind):
    code = getattr(obj, '__code__', None)
    if code is None:
        return False
    return kind in getattr(code, 'co_kinds', ())


def iscoroutine(obj):
    return type(obj).__name__ == 'coroutine'


def isgenerator(obj):
    return type(obj).__name__ == 'generator'


def isasyncgen(obj):
    return type(obj).__name__ == 'async_generator'


def isawaitable(obj):
    return iscoroutine(obj) or hasattr(obj, '__await__')


def unwrap(func, *, stop=None):
    while hasattr(func, '__wrapped__'):
        if stop is not None and stop(func):
            break
        func = func.__wrapped__
    return func


def getdoc(obj):
    doc = getattr(obj, '__doc__', None)
    if not isinstance(doc, str):
        return None
    lines = doc.expandtabs().split('\n')
    margin = None
    for line in lines[1:]:
        content = len(line.lstrip())
        if content:
            indent = len(line) - content
            margin = indent if margin is None else min(margin, indent)
    if margin is not None:
        lines[1:] = [l[margin:] for l in lines[1:]]
    while lines and not lines[-1]:
        lines.pop()
    while lines and not lines[0]:
        lines.pop(0)
    return '\n'.join(lines)


def getmembers(obj, predicate=None):
    out = []
    for name in dir(obj):
        try:
            value = getattr(obj, name)
        except AttributeError:
            continue
        if predicate is None or predicate(value):
            out.append((name, value))
    return sorted(out, key=lambda kv: kv[0])


def currentframe():
    return sys._getframe(1)


class _Empty:
    def __repr__(self):
        return '<class \'inspect._empty\'>'


_empty = _Empty()


class Parameter:
    POSITIONAL_ONLY = 0
    POSITIONAL_OR_KEYWORD = 1
    VAR_POSITIONAL = 2
    KEYWORD_ONLY = 3
    VAR_KEYWORD = 4
    empty = _empty

    def __init__(self, name, kind, *, default=_empty, annotation=_empty):
        self.name = name
        self.kind = kind
        self.default = default
        self.annotation = annotation

    def __str__(self):
        text = self.name
        if self.kind == 2:
            text = '*' + text
        elif self.kind == 4:
            text = '**' + text
        if self.default is not _empty:
            text += '=' + repr(self.default)
        return text

    def __repr__(self):
        return '<Parameter "%s">' % self


class Signature:
    empty = _empty

    def __init__(self, parameters=None, *, return_annotation=_empty):
        self.parameters = {p.name: p for p in (parameters or [])}
        self.return_annotation = return_annotation

    def __str__(self):
        parts = []
        params = list(self.parameters.values())
        star_done = False
        for i, p in enumerate(params):
            if p.kind == 3 and not star_done and not any(q.kind == 2 for q in params):
                parts.append('*')
                star_done = True
            parts.append(str(p))
            if p.kind == 0 and (i + 1 == len(params) or params[i + 1].kind != 0):
                parts.append('/')
        return '(' + ', '.join(parts) + ')'

    def __repr__(self):
        return '<Signature %s>' % self


def signature(obj):
    obj = _unwrap_method(obj)
    code = getattr(obj, '__code__', None)
    if code is None:
        raise ValueError('no signature found for %r' % (obj,))
    names = list(code.co_varnames[:code.co_argcount])
    defaults = tuple(getattr(obj, '__defaults__', None) or ())
    kwdefaults = getattr(obj, '__kwdefaults__', None) or {}
    posonly = getattr(code, 'co_posonlyargcount', 0)
    params = []
    first_default = len(names) - len(defaults)
    for i, name in enumerate(names):
        kind = 0 if i < posonly else 1
        default = defaults[i - first_default] if i >= first_default else _empty
        params.append(Parameter(name, kind, default=default))
    rest = list(code.co_varnames[code.co_argcount:])
    kwonly = rest[:code.co_kwonlyargcount]
    rest = rest[code.co_kwonlyargcount:]
    if code.co_flags_varargs:
        params.append(Parameter(rest.pop(0), 2))
    for name in kwonly:
        params.append(Parameter(name, 3, default=kwdefaults.get(name, _empty)))
    if code.co_flags_varkw and rest:
        params.append(Parameter(rest[0], 4))
    return Signature(params)
