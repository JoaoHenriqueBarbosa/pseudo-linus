"""re: o motor vem do módulo nativo `_re`; aqui ficam `Pattern` e `Match` como classes de verdade,
para `isinstance(x, re.Pattern)` e anotações como `re.Match[str]`."""

from _re import *
from _re import (compile, match, search, fullmatch, findall, finditer, sub, subn, split, escape, purge, error,
                 NOFLAG, I, IGNORECASE, L, LOCALE, M, MULTILINE, S, DOTALL, U, UNICODE, X, VERBOSE, A, ASCII,
                 DEBUG)


class _NativeType(type):
    """Metaclasse: `isinstance` olha o nome do tipo do objeto nativo."""

    def __instancecheck__(cls, obj):
        return type(obj).__name__ == cls.__name__


class Pattern(metaclass=_NativeType):
    def __class_getitem__(cls, item):
        return cls


class Match(metaclass=_NativeType):
    def __class_getitem__(cls, item):
        return cls
