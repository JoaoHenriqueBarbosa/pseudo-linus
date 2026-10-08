"""O que os geradores de tipos embutidos (`gen_builtin_type_*.py`, `gen_builtin_method_sigs.py`) compartilham:
o mapa dos tipos sem nome em `builtins` e a leitura da lista de tipos de `builtin-type-dir.tsv`.

Os geradores rodam com `python3 -I`, que tira o diretório do script de `sys.path`; cada um o põe de volta
antes de importar este módulo:

    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import gen_common
"""
import builtins
import socket
import sys
import types

NAMED = {
    'NoneType': type(None), 'NotImplementedType': type(NotImplemented), 'ellipsis': type(Ellipsis),
    'function': types.FunctionType, 'generator': types.GeneratorType, 'method': types.MethodType,
    'builtin_function_or_method': types.BuiltinFunctionType, 'coroutine': types.CoroutineType,
    'async_generator': types.AsyncGeneratorType, 'code': types.CodeType, 'socket': socket.socket,
    'dict_keys': type({}.keys()), 'dict_values': type({}.values()), 'dict_items': type({}.items()),
    'list_iterator': type(iter([])), 'tuple_iterator': type(iter(())), 'str_ascii_iterator': type(iter('a')),
    'str_iterator': type(iter('\xe9')), 'range_iterator': type(iter(range(1))),
    'dict_keyiterator': type(iter({})), 'dict_valueiterator': type(iter({}.values())),
    'dict_itemiterator': type(iter({}.items())), 'dict_reversekeyiterator': type(reversed({})),
    'set_iterator': type(iter(set())), 'list_reverseiterator': type(reversed([])),
    'bytes_iterator': type(iter(b'')), 'bytearray_iterator': type(iter(bytearray())),
    # O `tp_name` do C é `NoDefaultType`, sem módulo: um tipo de `builtins` que só o `typing` alcança.
    'NoDefaultType': type(__import__('typing').NoDefault),
    # Os tipos dos descritores (`types.GetSetDescriptorType` e companhia).
    'getset_descriptor': types.GetSetDescriptorType, 'member_descriptor': types.MemberDescriptorType,
    'method_descriptor': types.MethodDescriptorType, 'wrapper_descriptor': types.WrapperDescriptorType,
    'classmethod_descriptor': types.ClassMethodDescriptorType, 'method-wrapper': types.MethodWrapperType,
}


def type_names(path):
    """Os nomes de tipo da primeira coluna de `builtin-type-dir.tsv`."""
    with open(path, encoding='utf-8') as f:
        return [line.split('\t', 1)[0] for line in f if line.strip() and not line.startswith('#')]


# Módulos que o CPython escreve em C além dos embutidos no executável (`sys.builtin_module_names`).
EXTENSION_MODULES = ['_json', '_csv', '_struct', '_datetime', '_decimal', '_hashlib', '_bz2', '_lzma', '_ssl',
                     '_socket', 'select', 'math', 'cmath', 'zlib', 'binascii', 'array', 'unicodedata', '_random',
                     '_bisect', '_heapq', '_pickle', 'fcntl', 'termios', 'mmap', 'resource', 'grp',
                     '_posixsubprocess', '_contextvars', '_asyncio', '_queue', '_statistics', '_zoneinfo',
                     'readline', '_sqlite3', '_uuid']


def c_modules():
    """Os módulos em C importáveis do oráculo (os embutidos e os de `EXTENSION_MODULES`), em ordem de nome."""
    import importlib
    found = []
    for name in sorted(sys.builtin_module_names) + EXTENSION_MODULES:
        try:
            found.append(importlib.import_module(name))
        except ImportError:
            continue
    return found


def resolve(name):
    """O tipo de nome `name` (`NAMED` ou `builtins`), ou `None` quando não é um tipo."""
    t = NAMED.get(name) or getattr(builtins, name, None)
    return t if isinstance(t, type) else None
