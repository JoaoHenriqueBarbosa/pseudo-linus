"""_lsprof sobre o coletor nativo `_prof`: o mesmo contrato do CPython (`Profiler`, `profiler_entry`,
`profiler_subentry`), medindo as funções escritas em Python. Funções nativas não aparecem."""

import _prof
import sys


class _Code:
    """Faz as vezes do objeto de código que o `cProfile.label` espera."""

    def __init__(self, filename, lineno, name):
        self.co_filename = filename
        self.co_firstlineno = lineno
        self.co_name = name


class profiler_subentry:
    def __init__(self, code, callcount, reccallcount, totaltime, inlinetime):
        self.code = code
        self.callcount = callcount
        self.reccallcount = reccallcount
        self.totaltime = totaltime
        self.inlinetime = inlinetime

    def __repr__(self):
        return ("profiler_subentry(code=%r, callcount=%r, reccallcount=%r, totaltime=%r, inlinetime=%r)"
                % (self.code, self.callcount, self.reccallcount, self.totaltime, self.inlinetime))


class profiler_entry(profiler_subentry):
    def __init__(self, code, callcount, reccallcount, totaltime, inlinetime, calls):
        super().__init__(code, callcount, reccallcount, totaltime, inlinetime)
        self.calls = calls


class Profiler:
    def __init__(self, timer=None, timeunit=0.0, subcalls=True, builtins=True):
        self._timer = timer
        self._timeunit = timeunit
        _prof.clear()

    def enable(self, subcalls=True, builtins=True):
        _prof.start()

    def disable(self):
        _prof.stop()

    def clear(self):
        _prof.clear()

    def getstats(self):
        codes = {}
        script = sys.argv[0] if sys.argv and sys.argv[0] else '<string>'

        def code_for(filename, lineno, name):
            key = (filename or script, lineno, name)
            c = codes.get(key)
            if c is None:
                c = codes[key] = _Code(*key)
            return c

        out = []
        for row in _prof.dump():
            subs = [profiler_subentry(code_for(*s[:3]), *s[3:]) for s in row[7]]
            out.append(profiler_entry(code_for(*row[:3]), *row[3:7], subs))
        return out
