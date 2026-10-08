"""Secret Labs' Regular Expression Engine. Shim sobre o motor nativo `_re`: a compilação em bytecode do CPython
não é executada aqui; `compile` recompila o padrão de texto no motor nativo."""

import _re

MAGIC = 20230612
CODESIZE = 4
MAXREPEAT = 4294967295
MAXGROUPS = 2147483647
copyright = ' SRE 2.2.2 Copyright (c) 1997-2002 by Secret Labs AB '


def _simple(mapped, character):
    """O mapeamento simples (um ponto de código) de maiúsculas e minúsculas, como o `Py_UNICODE_TOLOWER`."""
    return ord(mapped) if len(mapped) == 1 else character


def getcodesize():
    return CODESIZE


def ascii_iscased(character, /):
    return character < 128 and chr(character).isalpha()


def unicode_iscased(character, /):
    text = chr(character)
    return character != _simple(text.lower(), character) or character != _simple(text.upper(), character)


def ascii_tolower(character, /):
    return character + 32 if 65 <= character <= 90 else character


def unicode_tolower(character, /):
    return _simple(chr(character).lower(), character)


def compile(pattern, flags, code, groups, groupindex, indexgroup):
    if pattern is None:
        raise TypeError("cannot compile a pre-parsed pattern: the native engine compiles from the pattern text")
    return _re.compile(pattern, flags)


def template(pattern, template):
    return tuple(template)
