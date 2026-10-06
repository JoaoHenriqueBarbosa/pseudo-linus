"""re: o motor vem do módulo nativo `_re`; aqui ficam `Pattern` e `Match` como classes de verdade,
para `isinstance(x, re.Pattern)` e anotações como `re.Match[str]`."""

from _re import *
from _re import (compile, match, search, fullmatch, findall, finditer, sub, subn, split, escape, purge, error)
import enum as _enum

PatternError = error


class RegexFlag(_enum.IntFlag):
    """As flags do `re` como membros de `IntFlag` (os motores nativos recebem o inteiro)."""
    NOFLAG = 0
    ASCII = A = 256
    IGNORECASE = I = 2
    LOCALE = L = 4
    UNICODE = U = 32
    MULTILINE = M = 8
    DOTALL = S = 16
    VERBOSE = X = 64
    DEBUG = 128

    def __repr__(self):
        if self._name_ is not None and '|' not in self._name_:
            return 're.' + self._name_
        return '|'.join(n if n.isdigit() else 're.' + n for n in self._members_in())

    __str__ = __repr__


NOFLAG = RegexFlag.NOFLAG
A = ASCII = RegexFlag.ASCII
I = IGNORECASE = RegexFlag.IGNORECASE
L = LOCALE = RegexFlag.LOCALE
U = UNICODE = RegexFlag.UNICODE
M = MULTILINE = RegexFlag.MULTILINE
S = DOTALL = RegexFlag.DOTALL
X = VERBOSE = RegexFlag.VERBOSE
DEBUG = RegexFlag.DEBUG


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
