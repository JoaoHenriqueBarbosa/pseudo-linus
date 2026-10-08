"""atexit: funções chamadas na saída normal do interpretador, na ordem inversa do registro."""

_exithandlers = []


def register(func, *args, **kwargs):
    if not callable(func):
        raise TypeError('the first argument must be callable')
    _exithandlers.append((func, args, kwargs))
    return func


def unregister(func):
    _exithandlers[:] = [h for h in _exithandlers if h[0] != func]


def _clear():
    del _exithandlers[:]


def _ncallbacks():
    return len(_exithandlers)


def _run_exitfuncs():
    import sys
    # O `Py_FinalizeEx` espera as threads (`threading._shutdown`) antes de chamar as funções do `atexit`.
    threading = sys.modules.get('threading')
    if threading is not None:
        threading._shutdown()
    while _exithandlers:
        func, args, kwargs = _exithandlers.pop()
        try:
            func(*args, **kwargs)
        except SystemExit:
            pass
        except BaseException as exc:
            sys.stderr.write('Exception ignored in atexit callback %r:\n' % (func,))
            import traceback
            traceback.print_exception(type(exc), exc, exc.__traceback__)
