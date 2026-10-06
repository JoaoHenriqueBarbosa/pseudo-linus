"""dis: só o que outros módulos da biblioteca padrão consultam (`inspect` usa `COMPILER_FLAG_NAMES` e `Positions`).

O interpretador não tem o bytecode do CPython, então o desassemblador em si não existe: as funções que o
exigem levantam `NotImplementedError` em vez de produzir uma listagem que não corresponde a nada."""

import collections

__all__ = ['COMPILER_FLAG_NAMES', 'Positions', 'dis', 'disassemble', 'distb', 'show_code', 'get_instructions',
           'findlinestarts', 'findlabels', 'code_info', 'Instruction', 'Bytecode']

COMPILER_FLAG_NAMES = {
    1: 'OPTIMIZED',
    2: 'NEWLOCALS',
    4: 'VARARGS',
    8: 'VARKEYWORDS',
    16: 'NESTED',
    32: 'GENERATOR',
    64: 'NOFREE',
    128: 'COROUTINE',
    256: 'ITERABLE_COROUTINE',
    512: 'ASYNC_GENERATOR',
}

Positions = collections.namedtuple('Positions', ['lineno', 'end_lineno', 'col_offset', 'end_col_offset'],
                                   defaults=[None] * 4)


def _unavailable(*args, **kwargs):
    raise NotImplementedError('bytecode disassembly is not available in this interpreter')


dis = disassemble = distb = show_code = get_instructions = findlinestarts = findlabels = code_info = _unavailable


class Instruction:
    def __init__(self, *args, **kwargs):
        _unavailable()


class Bytecode:
    def __init__(self, *args, **kwargs):
        _unavailable()
