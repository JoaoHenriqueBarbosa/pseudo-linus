"""Módulo `_asyncio` do CPython (em C lá): `Future` e `Task`.

O corpo é o `_PyFuture`/`_PyTask` do `futures.py` e do `tasks.py` do Debian, executados pelo mesmo
interpretador; o que o programa observa é o que o C mostra: `asyncio.Future.__module__` é `_asyncio`, a classe
`Task` herda da `Future` do módulo, e os quadros dos métodos não aparecem em traceback (ver `C_ACCELERATED` em
`vm.rs`). Os nomes saem sob demanda porque `futures.py` importa este módulo no fim dele, quando `tasks.py` ainda
nem existe, e `tasks.py` o importa no fim dele, quando `_PyTask` acaba de nascer."""

import sys

__all__ = ('Future', 'Task')


def _future():
    py_future = sys.modules['asyncio.futures']._PyFuture

    class Future(py_future):
        __module__ = '_asyncio'
        __qualname__ = 'Future'
        __doc__ = py_future.__doc__

    return Future


def _task(future):
    py_task = sys.modules['asyncio.tasks']._PyTask

    class Task(py_task, future):
        __module__ = '_asyncio'
        __qualname__ = 'Task'
        __doc__ = py_task.__doc__

    return Task


def __getattr__(name):
    if name == 'Future':
        value = globals()['Future'] = _future()
    elif name == 'Task':
        future = globals().get('Future') or __getattr__('Future')
        value = globals()['Task'] = _task(future)
    else:
        raise AttributeError(f"module '_asyncio' has no attribute '{name}'")
    return value
