"""This module provides access to some objects used or maintained by the
interpreter and to functions that interact strongly with the interpreter.

Dynamic objects:

argv -- command line arguments; argv[0] is the script pathname if known
path -- module search path; path[0] is the script directory, else ''
modules -- dictionary of loaded modules

displayhook -- called to show results in an interactive session
excepthook -- called to handle any uncaught exception other than SystemExit
  To customize printing in an interactive session or to install a custom
  top-level exception handler, assign other functions to replace these.

stdin -- standard input file object; used by input()
stdout -- standard output file object; used by print()
stderr -- standard error object; used for error messages
  By assigning other file objects (or objects that behave like files)
  to these, it is possible to redirect all of the interpreter's I/O.

last_exc - the last uncaught exception
  Only available in an interactive session after a
  traceback has been printed.
last_type -- type of last uncaught exception
last_value -- value of last uncaught exception
last_traceback -- traceback of last uncaught exception
  These three are the (deprecated) legacy representation of last_exc.

Static objects:

builtin_module_names -- tuple of module names built into this interpreter
copyright -- copyright notice pertaining to this interpreter
exec_prefix -- prefix used to find the machine-specific Python library
executable -- absolute path of the executable binary of the Python interpreter
float_info -- a named tuple with information about the float implementation.
float_repr_style -- string indicating the style of repr() output for floats
hash_info -- a named tuple with information about the hash algorithm.
hexversion -- version information encoded as a single integer
implementation -- Python implementation information.
int_info -- a named tuple with information about the int implementation.
maxsize -- the largest supported length of containers.
maxunicode -- the value of the largest Unicode code point
platform -- platform identifier
prefix -- prefix used to find the Python library
thread_info -- a named tuple with information about the thread implementation.
version -- the version of this interpreter as a string
version_info -- version information as a named tuple
__stdin__ -- the original stdin; don't touch!
__stdout__ -- the original stdout; don't touch!
__stderr__ -- the original stderr; don't touch!
__displayhook__ -- the original displayhook; don't touch!
__excepthook__ -- the original excepthook; don't touch!

Functions:

displayhook() -- print an object to the screen, and save it in builtins._
excepthook() -- print an exception and its traceback to sys.stderr
exception() -- return the current thread's active exception
exc_info() -- return information about the current thread's active exception
exit() -- exit the interpreter by raising SystemExit
getdlopenflags() -- returns flags to be used for dlopen() calls
getprofile() -- get the global profiling function
getrefcount() -- return the reference count for an object (plus one :-)
getrecursionlimit() -- return the max recursion depth for the interpreter
getsizeof() -- return the size of an object in bytes
gettrace() -- get the global debug tracing function
setdlopenflags() -- set the flags to be used for dlopen() calls
setprofile() -- set the global profiling function
setrecursionlimit() -- set the max recursion depth for the interpreter
settrace() -- set the global debug tracing function
"""

# O módulo sys do interpretador. No CPython ele é embutido: o namespace só tem o que o `sys` real
# tem, então toda a montagem acontece dentro de `_build`, que devolve os nomes públicos e some no
# fim (os auxiliares ficam presos em closures, fora do `dir(sys)`).


def _build():
    import _sys

    def plain(f, name=None):
        """Tira o `_build.<locals>` do nome qualificado de uma função ou classe."""
        f.__qualname__ = name or f.__name__
        if name:
            f.__name__ = name
        return f

    def structseq(name, fields, values, module='sys'):
        """Uma sequência nomeada somente leitura (`structseq` do CPython), do tipo `sys.<name>`."""

        class StructSeq:
            _fields = tuple(fields)
            n_fields = len(fields)
            n_sequence_fields = len(fields)
            n_unnamed_fields = 0

            def __init__(self, vals):
                object.__setattr__(self, '_values', tuple(vals))
                for f, v in zip(fields, vals):
                    self.__dict__[f] = v

            def __setattr__(self, key, value):
                raise AttributeError('readonly attribute')

            def __getitem__(self, i):
                return self._values[i]

            def __iter__(self):
                return iter(self._values)

            def __len__(self):
                return len(self._values)

            def __eq__(self, other):
                return self._values == tuple(other)

            def __lt__(self, other):
                return self._values < tuple(other)

            def __le__(self, other):
                return self._values <= tuple(other)

            def __gt__(self, other):
                return self._values > tuple(other)

            def __ge__(self, other):
                return self._values >= tuple(other)

            def __hash__(self):
                return hash(self._values)

            def __reduce__(self):
                return (type(self), (self._values,))

            def __repr__(self):
                return '%s.%s(%s)' % (module, name, ', '.join('%s=%r' % kv for kv in zip(fields, self._values)))

        # O tipo se chama `flags`, `version_info`... (o `tp_name` do CPython), não `StructSeq`.
        skip = ('__dict__', '__weakref__', '__module__', '__qualname__', '__doc__')
        attrs = {k: v for k, v in vars(StructSeq).items() if k not in skip}
        attrs['__module__'] = module
        return type(name, (), attrs)(values)

    class SimpleNamespace:
        def __init__(self, **kw):
            self.__dict__.update(kw)

        def __repr__(self):
            return 'namespace(%s)' % ', '.join('%s=%r' % kv for kv in self.__dict__.items())

        def __eq__(self, other):
            return isinstance(other, SimpleNamespace) and self.__dict__ == other.__dict__

    plain(SimpleNamespace)
    SimpleNamespace.__module__ = 'types'

    class Modules:
        """`sys.modules`: visão dos módulos carregados (leitura sempre atual; escritas ficam à parte)."""

        def __init__(self):
            self._extra = {}

        def _all(self):
            d = _sys._modules()
            d.update(self._extra)
            return d

        def __getitem__(self, key):
            return self._all()[key]

        def __setitem__(self, key, value):
            if isinstance(key, str) and _sys._set_module(key, value):
                self._extra.pop(key, None)
            else:
                self._extra[key] = value

        def __delitem__(self, key):
            if key in self._extra:
                del self._extra[key]
            elif not (isinstance(key, str) and _sys._pop_module(key)):
                raise KeyError(key)

        def __contains__(self, key):
            return key in self._all()

        def __iter__(self):
            return iter(self._all())

        def __len__(self):
            return len(self._all())

        def get(self, key, default=None):
            return self._all().get(key, default)

        def keys(self):
            return self._all().keys()

        def values(self):
            return self._all().values()

        def items(self):
            return self._all().items()

        def pop(self, key, *default):
            if key in self._extra:
                return self._extra.pop(key)
            d = self._all()
            if key in d:
                value = d[key]
                self.__delitem__(key)
                return value
            if default:
                return default[0]
            raise KeyError(key)

        def setdefault(self, key, default=None):
            d = self._all()
            if key in d:
                return d[key]
            self[key] = default
            return default

        def update(self, *args, **kwargs):
            for key, value in dict(*args, **kwargs).items():
                self[key] = value

        def copy(self):
            return dict(self._all())

        def __repr__(self):
            return repr(self._all())

    skip = ('__dict__', '__weakref__', '__module__', '__qualname__', '__doc__')
    # `sys.modules` é um `dict` para o programa: o tipo leva o nome do embutido.
    modules_attrs = {k: v for k, v in vars(Modules).items() if k not in skip}
    modules_attrs['__module__'] = 'builtins'
    Modules = type('dict', (), modules_attrs)

    state = {'audit': [], 'dlopenflags': 2, 'int_max_str_digits': 4300,
             'asyncgen_hooks': (None, None), 'coroutine_depth': 0, 'switchinterval': 0.005}

    ns = {}

    def public(f):
        ns[f.__name__] = plain(f)
        return f

    @public
    def exception():
        """Return the current exception."""
        return _sys.exc_info()[1]

    @public
    def displayhook(value):
        """Print an object to sys.stdout and also save it in builtins._"""
        if value is None:
            return
        import builtins
        builtins._ = None
        print(repr(value))
        builtins._ = value

    @public
    def excepthook(exctype, value, traceback):
        """Handle an exception by displaying it with a traceback on sys.stderr."""
        import traceback as tb
        tb.print_exception(exctype, value, traceback)

    @public
    def breakpointhook(*args, **kws):
        """This hook function is called by built-in breakpoint()."""
        # `sys_breakpointhook` do sysmodule.c: lê o PYTHONBREAKPOINT a cada chamada (a não ser com -E).
        import _os
        envar = None if _sys.cli_flags[7] else _os.getenv('PYTHONBREAKPOINT')
        if not envar:
            hookname = 'pdb.set_trace'
        elif envar == '0':
            return None
        else:
            hookname = envar
        modname, dot, attrname = hookname.rpartition('.')
        if not dot:
            modname = 'builtins'
        try:
            hook = getattr(__import__(modname, None, None, ['__name__']), attrname)
        except BaseException:
            import warnings
            warnings.warn('Ignoring unimportable $PYTHONBREAKPOINT: "%s"' % envar, RuntimeWarning, 2)
            return None
        return hook(*args, **kws)

    @public
    def unraisablehook(unraisable):
        """Handle an unraisable exception."""
        import traceback as tb
        msg = unraisable.err_msg or 'Exception ignored in'
        _sys.stderr.write('%s: %r\n' % (msg, unraisable.object))
        tb.print_exception(unraisable.exc_type, unraisable.exc_value, unraisable.exc_traceback)

    @public
    def addaudithook(hook):
        """Adds a new audit hook callback."""
        state['audit'].append(hook)

    @public
    def audit(event, *args):
        """Passes the event to any audit hooks that are attached."""
        for hook in state['audit']:
            hook(event, args)

    @public
    def getdefaultencoding():
        """Return the current default encoding used by the Unicode implementation."""
        return 'utf-8'

    @public
    def getfilesystemencoding():
        """Return the encoding used to convert Unicode filenames to OS filenames."""
        return 'utf-8'

    @public
    def getfilesystemencodeerrors():
        """Return the error mode used Unicode to OS filename conversion."""
        return 'surrogateescape'

    @public
    def intern(string):
        """``Intern'' the given string."""
        return string

    @public
    def is_finalizing():
        """Return True if Python is exiting."""
        return False

    @public
    def getswitchinterval():
        """Return the current thread switch interval; see sys.setswitchinterval()."""
        return state['switchinterval']

    @public
    def setswitchinterval(interval):
        """Set the ideal thread switching delay inside the Python interpreter."""
        if interval <= 0:
            raise ValueError('switch interval must be strictly positive')
        state['switchinterval'] = float(interval)
        _sys._gt_interval(state['switchinterval'])

    @public
    def getdlopenflags():
        """Return the current value of the flags that are used for dlopen calls."""
        return state['dlopenflags']

    @public
    def setdlopenflags(flags):
        """Set the flags used by the interpreter for dlopen calls."""
        state['dlopenflags'] = flags

    @public
    def get_int_max_str_digits():
        """Return the maximum string digits limit for non-binary int<->str conversions."""
        return state['int_max_str_digits']

    @public
    def set_int_max_str_digits(maxdigits):
        """Set the maximum string digits limit for non-binary int<->str conversions."""
        if maxdigits != 0 and maxdigits < 640:
            raise ValueError('maxdigits must be >= %d or 0 for unlimited' % 640)
        state['int_max_str_digits'] = maxdigits

    @public
    def get_coroutine_origin_tracking_depth():
        """Check status of origin tracking for coroutine objects in this thread."""
        return state['coroutine_depth']

    @public
    def set_coroutine_origin_tracking_depth(depth):
        """Enable or disable origin tracking for coroutine objects in this thread."""
        if depth < 0:
            raise ValueError('depth must be >= 0')
        state['coroutine_depth'] = depth

    asyncgen_hooks = structseq('asyncgen_hooks', ('firstiter', 'finalizer'), (None, None))
    hooks_type = type(asyncgen_hooks)

    @public
    def get_asyncgen_hooks():
        """Return the installed asynchronous generators hooks."""
        return hooks_type(state['asyncgen_hooks'])

    @public
    def set_asyncgen_hooks(firstiter=None, finalizer=None):
        """Set new asynchronous generators hooks."""
        state['asyncgen_hooks'] = (firstiter, finalizer)

    @public
    def call_tracing(func, args):
        """Call func(*args), while tracing is enabled."""
        return func(*args)

    @public
    def getallocatedblocks():
        """Return the number of memory blocks currently allocated."""
        return 16064

    @public
    def getunicodeinternedsize():
        """Return the number of elements of the unicode interned dictionary"""
        return 4365

    @public
    def is_stack_trampoline_active():
        """Return *True* if a stack profiler trampoline is active."""
        return False

    @public
    def activate_stack_trampoline(backend):
        """Activate stack profiler trampoline *backend*."""
        if backend != 'perf':
            raise ValueError('invalid backend: %s' % backend)

    @public
    def deactivate_stack_trampoline():
        """Deactivate the current stack profiler trampoline backend."""

    @public
    def _is_gil_enabled():
        """Return True if the GIL is currently enabled and False otherwise."""
        return True

    @public
    def _get_cpu_count_config():
        """Private function for getting PyConfig.cpu_count"""
        return -1

    @public
    def _is_interned(string):
        """Return True if the given string is "interned"."""
        return False

    @public
    def _clear_type_cache():
        """Clear the internal type lookup cache."""

    @public
    def _clear_internal_caches():
        """Clear all internal performance-related caches."""

    @public
    def _current_frames():
        """Return a dict mapping each thread's thread id to its current stack frame."""
        import threading
        return {threading.get_ident(): _sys._getframe(1)}

    @public
    def _current_exceptions():
        """Return a dict mapping each thread's identifier to its current raised exception."""
        import threading
        return {threading.get_ident(): _sys.exc_info()[1]}

    @public
    def _getframemodulename(depth=0):
        """Return the name of the module for a calling frame."""
        try:
            # Esta função conta como embutida (sem quadro próprio, como a do CPython): o quadro 0 do
            # `_getframe` já é o de quem a chamou.
            return _sys._getframe(depth).f_globals.get('__name__')
        except ValueError:
            return None

    @public
    def _debugmallocstats():
        """Print summary info to stderr about the state of pymalloc's structures."""

    @public
    def _setprofileallthreads(function):
        """Set the profiling function in all running threads belonging to the current interpreter."""
        _sys.setprofile(function)

    @public
    def _settraceallthreads(function):
        """Set the global debug tracing function in all running threads belonging to the current interpreter."""
        _sys.settrace(function)

    @public
    def _baserepl():
        """Private function for getting the base REPL"""
        import code
        code.interact()

    @public
    def getsizeof(obj, default=None):
        """Return the size of object in bytes."""
        if obj is None or isinstance(obj, bool):
            return 16 if obj is None else 28
        if isinstance(obj, int):
            return 24 + 4 * max(1, (abs(obj).bit_length() + 29) // 30) if obj else 24
        if isinstance(obj, float):
            return 24
        if isinstance(obj, str):
            return 40 + len(obj.encode('utf-8')) + (1 if obj.isascii() else 0) + (0 if obj.isascii() else 8)
        if isinstance(obj, (bytes, bytearray)):
            return 33 + len(obj)
        if isinstance(obj, tuple):
            return 40 + 8 * len(obj)
        if isinstance(obj, list):
            return 56 + 8 * len(obj)
        if isinstance(obj, (dict, set, frozenset)):
            return 64 + 32 * len(obj) if isinstance(obj, dict) else 200 + 16 * len(obj)
        return 48

    cli = list(_sys.cli_flags)
    cli[3] = _sys.optimize
    cli[13] = bool(cli[13])
    cli[16] = bool(cli[16])
    flags = structseq('flags', ('debug', 'inspect', 'interactive', 'optimize', 'dont_write_bytecode',
                                'no_user_site', 'no_site', 'ignore_environment', 'verbose', 'bytes_warning',
                                'quiet', 'hash_randomization', 'isolated', 'dev_mode', 'utf8_mode',
                                'warn_default_encoding', 'safe_path', 'int_max_str_digits'), cli)
    version_info = structseq('version_info', ('major', 'minor', 'micro', 'releaselevel', 'serial'),
                             (3, 13, 5, 'final', 0))
    hexversion = 51185136
    # `sys.path[0]` (o diretório do script, ou '' para -c e -m) só entra depois do `site`, como no
    # `pymain_run_python` do CPython: o interpretador o insere antes de rodar o programa.
    path = list(_sys.path)
    from _frozen_importlib import BuiltinImporter, FrozenImporter
    from _frozen_importlib_external import PathFinder

    def register_readline():
        import site
        hook = getattr(site, 'register_readline', None)
        if hook is not None and hook is not register_readline:
            hook()

    plain(register_readline).__module__ = 'site'

    # `sys.monitoring` (PEP 669): as ferramentas registradas e os eventos pedidos por cada uma.
    monitoring = type(_sys)('sys.monitoring')
    tools = {}
    tool_events = {}
    callbacks = {}
    mon = vars(monitoring)
    mon.update(DEBUGGER_ID=0, COVERAGE_ID=1, PROFILER_ID=2, OPTIMIZER_ID=5, MISSING=object(), DISABLE=object(),
               events=SimpleNamespace(PY_START=1, PY_RESUME=2, PY_RETURN=4, PY_YIELD=8, CALL=16, LINE=32,
                                      INSTRUCTION=64, JUMP=128, BRANCH=256, STOP_ITERATION=512, RAISE=1024,
                                      EXCEPTION_HANDLED=2048, PY_UNWIND=4096, PY_THROW=8192, RERAISE=16384,
                                      C_RETURN=32768, C_RAISE=65536, NO_EVENTS=0))

    def check_tool(tool_id):
        if not 0 <= tool_id < 6:
            raise ValueError('invalid tool %d (must be between 0 and 5)' % tool_id)

    def use_tool_id(tool_id, name):
        check_tool(tool_id)
        if not isinstance(name, str):
            raise ValueError('tool name must be a str')
        if tool_id in tools:
            raise ValueError('tool %d is already in use' % tool_id)
        tools[tool_id] = name

    def free_tool_id(tool_id):
        check_tool(tool_id)
        tools.pop(tool_id, None)
        tool_events.pop(tool_id, None)
        for key in [k for k in callbacks if k[0] == tool_id]:
            del callbacks[key]

    def get_tool(tool_id):
        check_tool(tool_id)
        return tools.get(tool_id)

    def register_callback(tool_id, event, func):
        check_tool(tool_id)
        old = callbacks.get((tool_id, event))
        callbacks[(tool_id, event)] = func
        return old

    def get_events(tool_id):
        check_tool(tool_id)
        return tool_events.get(tool_id, 0)

    def set_events(tool_id, event_set):
        check_tool(tool_id)
        if tool_id not in tools:
            raise ValueError('tool %d is not in use' % tool_id)
        tool_events[tool_id] = event_set

    def get_local_events(tool_id, code):
        check_tool(tool_id)
        return 0

    def set_local_events(tool_id, code, event_set):
        check_tool(tool_id)
        if tool_id not in tools:
            raise ValueError('tool %d is not in use' % tool_id)

    def restart_events():
        pass

    def _all_events():
        return {}

    for f in (use_tool_id, free_tool_id, get_tool, register_callback, get_events, set_events, get_local_events,
              set_local_events, restart_events, _all_events):
        plain(f).__module__ = 'sys.monitoring'
        mon[f.__name__] = f
    mon['__doc__'] = None

    ns.update(
        monitoring=monitoring,
        __interactivehook__=register_readline,
        argv=_sys.argv,
        orig_argv=['python3'] + _sys.argv,
        stdin=_sys.stdin,
        stdout=_sys.stdout,
        stderr=_sys.stderr,
        __stdin__=_sys.stdin,
        __stdout__=_sys.stdout,
        __stderr__=_sys.stderr,
        exit=_sys.exit,
        exc_info=_sys.exc_info,
        _getframe=_sys._getframe,
        getrefcount=_sys.getrefcount,
        getrecursionlimit=_sys.getrecursionlimit,
        setrecursionlimit=_sys.setrecursionlimit,
        settrace=_sys.settrace,
        gettrace=_sys.gettrace,
        setprofile=_sys.setprofile,
        getprofile=_sys.getprofile,
        version='3.13.5 (main, Aug 10 2026, 12:06:59) [GCC 14.2.0]',
        version_info=version_info,
        hexversion=hexversion,
        api_version=1013,
        copyright='Copyright (c) 2001-2024 Python Software Foundation.\nAll Rights Reserved.\n\n'
                  'Copyright (c) 2000 BeOpen.com.\nAll Rights Reserved.\n\n'
                  'Copyright (c) 1995-2001 Corporation for National Research Initiatives.\nAll Rights Reserved.\n\n'
                  'Copyright (c) 1991-1995 Stichting Mathematisch Centrum, Amsterdam.\nAll Rights Reserved.',
        platform='linux',
        abiflags='',
        executable=_sys.executable,
        _base_executable=_sys.base_executable,
        prefix=_sys.prefix,
        exec_prefix=_sys.prefix,
        base_prefix='/usr',
        base_exec_prefix='/usr',
        platlibdir='lib',
        pycache_prefix=None,
        _stdlib_dir='/usr/lib/python3.13',
        _home=None,
        _framework='',
        _git=('CPython', '', ''),
        _xoptions={},
        byteorder='little',
        maxsize=9223372036854775807,
        maxunicode=1114111,
        float_repr_style='short',
        path=path,
        path_hooks=[],
        path_importer_cache={},
        meta_path=[BuiltinImporter, FrozenImporter, PathFinder],
        warnoptions=_sys.warnoptions,
        dont_write_bytecode=True,
        modules=Modules(),
        flags=flags,
        hash_info=structseq('hash_info', ('width', 'modulus', 'inf', 'nan', 'imag', 'algorithm', 'hash_bits',
                                          'seed_bits', 'cutoff'),
                            (64, 2305843009213693951, 314159, 0, 1000003, 'siphash13', 64, 128, 0)),
        float_info=structseq('float_info', ('max', 'max_exp', 'max_10_exp', 'min', 'min_exp', 'min_10_exp', 'dig',
                                            'mant_dig', 'epsilon', 'radix', 'rounds'),
                             (1.7976931348623157e+308, 1024, 308, 2.2250738585072014e-308, -1021, -307, 15, 53,
                              2.220446049250313e-16, 2, 1)),
        int_info=structseq('int_info', ('bits_per_digit', 'sizeof_digit', 'default_max_str_digits',
                                        'str_digits_check_threshold'), (30, 4, 4300, 640)),
        thread_info=structseq('thread_info', ('name', 'lock', 'version'), ('pthread', 'semaphore', 'NPTL 2.41')),
        implementation=SimpleNamespace(name='cpython', cache_tag='cpython-313', version=version_info,
                                       hexversion=hexversion, _multiarch='x86_64-linux-gnu'),
        builtin_module_names=(
            '_abc', '_ast', '_bisect', '_blake2', '_codecs', '_collections', '_csv', '_datetime', '_elementtree',
            '_functools', '_heapq', '_imp', '_io', '_json', '_locale', '_md5', '_opcode', '_operator', '_pickle',
            '_posixsubprocess', '_random', '_sha1', '_sha2', '_sha3', '_signal', '_socket', '_sre', '_stat',
            '_statistics', '_string', '_struct', '_suggestions', '_symtable', '_sysconfig', '_thread', '_tokenize',
            '_tracemalloc', '_typing', '_warnings', '_weakref', 'array', 'atexit', 'binascii', 'builtins', 'cmath',
            'errno', 'faulthandler', 'fcntl', 'gc', 'grp', 'itertools', 'marshal', 'math', 'posix', 'pwd',
            'pyexpat', 'select', 'sys', 'syslog', 'time', 'unicodedata', 'zlib'),
        stdlib_module_names=frozenset('''
            __future__ _abc _aix_support _android_support _apple_support _ast _asyncio _bisect _blake2 _bz2
            _codecs _codecs_cn _codecs_hk _codecs_iso2022 _codecs_jp _codecs_kr _codecs_tw _collections
            _collections_abc _colorize _compat_pickle _compression _contextvars _csv _ctypes _curses
            _curses_panel _datetime _dbm _decimal _elementtree _frozen_importlib _frozen_importlib_external
            _functools _gdbm _hashlib _heapq _imp _interpchannels _interpqueues _interpreters _io _ios_support
            _json _locale _lsprof _lzma _markupbase _md5 _multibytecodec _multiprocessing _opcode
            _opcode_metadata _operator _osx_support _overlapped _pickle _posixshmem _posixsubprocess _py_abc
            _pydatetime _pydecimal _pyio _pylong _pyrepl _queue _random _scproxy _sha1 _sha2 _sha3 _signal
            _sitebuiltins _socket _sqlite3 _sre _ssl _stat _statistics _string _strptime _struct _suggestions
            _symtable _sysconfig _thread _threading_local _tkinter _tokenize _tracemalloc _typing _uuid
            _warnings _weakref _weakrefset _winapi _wmi _zoneinfo abc antigravity argparse array ast asyncio
            atexit base64 bdb binascii bisect builtins bz2 cProfile calendar cmath cmd code codeop
            collections colorsys compileall concurrent configparser contextlib contextvars copy copyreg csv
            ctypes curses dataclasses datetime dbm decimal difflib dis doctest email encodings ensurepip enum
            errno faulthandler fcntl filecmp fileinput fnmatch fractions ftplib functools gc genericpath getopt
            getpass gettext glob graphlib grp gzip hashlib heapq hmac html http idlelib imaplib importlib
            inspect io ipaddress itertools json keyword linecache locale logging lzma mailbox marshal math
            mimetypes mmap modulefinder msvcrt multiprocessing netrc nt ntpath nturl2path numbers opcode
            operator optparse os pathlib pdb pickle pickletools pkgutil platform plistlib poplib posix posixpath
            pprint profile pstats pty pwd py_compile pyclbr pydoc pydoc_data pyexpat queue quopri random re
            readline reprlib resource rlcompleter runpy sched secrets select selectors shelve shlex shutil
            signal site smtplib socket socketserver sqlite3 sre_compile sre_constants sre_parse ssl stat
            statistics string stringprep struct subprocess symtable sys sysconfig syslog tabnanny tarfile
            tempfile termios textwrap this threading time timeit tkinter token tokenize tomllib trace traceback
            tracemalloc tty turtle turtledemo types typing unicodedata unittest urllib uuid venv warnings wave
            weakref webbrowser winreg winsound wsgiref xml xmlrpc zipapp zipfile zipimport zlib zoneinfo
            '''.split()),
        __loader__=BuiltinImporter,
    )
    ns['__displayhook__'] = ns['displayhook']
    ns['__excepthook__'] = ns['excepthook']
    ns['__breakpointhook__'] = ns['breakpointhook']
    ns['__unraisablehook__'] = ns['unraisablehook']
    return ns


globals().update(_build())
del _build
__spec__ = __loader__.find_spec('sys')


def _install_path_hooks():
    # Depois do `sys` montado: o `zipimport` puxa o `os`, que lê `sys.platform`.
    from importlib.machinery import FileFinder, SourceFileLoader, SOURCE_SUFFIXES
    from zipimport import zipimporter
    path_hooks.extend([zipimporter, FileFinder.path_hook((SourceFileLoader, SOURCE_SUFFIXES))])


_install_path_hooks()
del _install_path_hooks
