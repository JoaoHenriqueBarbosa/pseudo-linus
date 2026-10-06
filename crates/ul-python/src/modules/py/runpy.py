"""runpy enxuto: `run_path` e `run_module` executam o código num namespace novo e devolvem seus globals."""
import sys
import types

__all__ = ["run_module", "run_path"]


def _exec_file(path, init_globals, run_name, mod_name=None, package=None):
    with open(path, 'rb') as f:
        data = f.read()
    code = compile(data, path, 'exec')
    ns = {}
    if init_globals:
        ns.update(init_globals)
    ns.update({'__name__': run_name, '__file__': path, '__cached__': None, '__doc__': None,
               '__loader__': None, '__package__': package, '__spec__': None})
    exec(code, ns)
    return ns


def run_path(path_name, init_globals=None, run_name=None):
    import os
    if run_name is None:
        run_name = "<run_path>"
    path_name = os.fspath(path_name)
    if os.path.isdir(path_name):
        main = os.path.join(path_name, '__main__.py')
        if not os.path.isfile(main):
            raise ImportError("can't find '__main__' module in %r" % path_name)
        old = list(sys.path)
        sys.path.insert(0, path_name)
        try:
            return _exec_file(main, init_globals, run_name)
        finally:
            sys.path[:] = old
    return _exec_file(path_name, init_globals, run_name)


def run_module(mod_name, init_globals=None, run_name=None, alter_sys=False):
    import importlib.util
    spec = importlib.util.find_spec(mod_name)
    if spec is None:
        raise ImportError("No module named %s" % mod_name)
    path = spec.origin
    if spec.submodule_search_locations:
        import os
        path = os.path.join(list(spec.submodule_search_locations)[0], '__main__.py')
    if path is None or not path.endswith('.py'):
        raise ImportError("No code object available for %s" % mod_name)
    package = mod_name.rpartition('.')[0] if not spec.submodule_search_locations else mod_name
    return _exec_file(path, init_globals, run_name or mod_name, mod_name, package)
