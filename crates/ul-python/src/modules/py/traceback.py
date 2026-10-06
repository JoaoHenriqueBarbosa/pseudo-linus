"""traceback: formatação de tracebacks sobre `e.__traceback__` (sem as marcas de coluna `^^^^`)."""

import sys

__all__ = ['extract_stack', 'extract_tb', 'format_exception', 'format_exception_only', 'format_list',
           'format_stack', 'format_tb', 'print_exc', 'format_exc', 'print_exception', 'print_last',
           'print_stack', 'print_tb', 'clear_frames', 'FrameSummary', 'StackSummary',
           'TracebackException', 'walk_stack', 'walk_tb']

_sentinel = object()
_source_cache = {}


def _source_line(filename, lineno):
    if filename.startswith('<'):
        return None
    lines = _source_cache.get(filename)
    if lines is None:
        try:
            with open(filename, encoding='utf-8') as f:
                lines = f.read().splitlines()
        except (OSError, ValueError, UnicodeDecodeError):
            lines = []
        _source_cache[filename] = lines
    if 1 <= lineno <= len(lines):
        return lines[lineno - 1].strip()
    return None


def walk_tb(tb):
    while tb is not None:
        yield tb.tb_frame, tb.tb_lineno
        tb = tb.tb_next


def walk_stack(f):
    return iter(())


class FrameSummary:
    __slots__ = ('filename', 'lineno', 'name', '_line', 'locals', 'end_lineno', 'colno', 'end_colno')

    def __init__(self, filename, lineno, name, *, lookup_line=True, locals=None, line=None,
                 end_lineno=None, colno=None, end_colno=None):
        self.filename = filename
        self.lineno = lineno
        self.name = name
        self._line = line
        if lookup_line:
            self.line
        self.locals = {k: repr(v) for k, v in locals.items()} if locals else None
        self.end_lineno = end_lineno
        self.colno = colno
        self.end_colno = end_colno

    def __eq__(self, other):
        if isinstance(other, FrameSummary):
            return (self.filename == other.filename and self.lineno == other.lineno and
                    self.name == other.name and self.locals == other.locals)
        if isinstance(other, tuple):
            return (self.filename, self.lineno, self.name, self.line) == other
        return NotImplemented

    def __getitem__(self, pos):
        return (self.filename, self.lineno, self.name, self.line)[pos]

    def __iter__(self):
        return iter([self.filename, self.lineno, self.name, self.line])

    def __repr__(self):
        return '<FrameSummary file {filename}, line {lineno} in {name}>'.format(
            filename=self.filename, lineno=self.lineno, name=self.name)

    def __len__(self):
        return 4

    @property
    def line(self):
        if self._line is None:
            if self.lineno is None:
                return None
            self._line = _source_line(self.filename, self.lineno) or ''
        return self._line.strip()


class StackSummary(list):
    @classmethod
    def extract(klass, frame_gen, *, limit=None, lookup_lines=True, capture_locals=False):
        result = klass()
        for f, lineno in frame_gen:
            co = f.f_code
            result.append(FrameSummary(co.co_filename, lineno, co.co_name, lookup_line=lookup_lines))
        if limit is not None:
            if limit >= 0:
                result[:] = result[:limit]
            else:
                result[:] = result[limit:]
        return result

    @classmethod
    def from_list(klass, a_list):
        result = StackSummary()
        for frame in a_list:
            if isinstance(frame, FrameSummary):
                result.append(frame)
            else:
                filename, lineno, name, line = frame
                result.append(FrameSummary(filename, lineno, name, line=line))
        return result

    def format_frame_summary(self, frame_summary):
        row = ['  File "{}", line {}, in {}\n'.format(frame_summary.filename, frame_summary.lineno,
                                                       frame_summary.name)]
        if frame_summary.line:
            row.append('    {}\n'.format(frame_summary.line.strip()))
        return ''.join(row)

    def format(self):
        result = []
        last_file = None
        last_line = None
        last_name = None
        count = 0
        for frame_summary in self:
            formatted_frame = self.format_frame_summary(frame_summary)
            if formatted_frame is None:
                continue
            if (last_file is None or last_file != frame_summary.filename or
                    last_line is None or last_line != frame_summary.lineno or
                    last_name is None or last_name != frame_summary.name):
                if count > 3:
                    count -= 3
                    result.append('  [Previous line repeated {count} more time{s}]\n'.format(
                        count=count, s='s' if count > 1 else ''))
                last_file = frame_summary.filename
                last_line = frame_summary.lineno
                last_name = frame_summary.name
                count = 0
            count += 1
            if count > 3:
                continue
            result.append(formatted_frame)
        if count > 3:
            count -= 3
            result.append('  [Previous line repeated {count} more time{s}]\n'.format(
                count=count, s='s' if count > 1 else ''))
        return result


def extract_tb(tb, limit=None):
    return StackSummary.extract(walk_tb(tb), limit=limit)


def extract_stack(f=None, limit=None):
    return StackSummary()


def format_list(extracted_list):
    return StackSummary.from_list(extracted_list).format()


def format_tb(tb, limit=None):
    return extract_tb(tb, limit=limit).format()


def print_list(extracted_list, file=None):
    if file is None:
        file = sys.stderr
    for item in StackSummary.from_list(extracted_list).format():
        print(item, file=file, end='')


def print_tb(tb, limit=None, file=None):
    print_list(extract_tb(tb, limit=limit), file=file)


def format_stack(f=None, limit=None):
    return []


def print_stack(f=None, limit=None, file=None):
    pass


def _type_name(exc_type):
    module = getattr(exc_type, '__module__', None)
    name = getattr(exc_type, '__qualname__', None) or exc_type.__name__
    if module not in ('__main__', 'builtins', None):
        return module + '.' + name
    return name


def _format_final_line(exc_type, value):
    name = _type_name(exc_type)
    try:
        text = str(value)
    except Exception:
        text = '<exception str() failed>'
    if value is None or not text:
        return name + '\n'
    return '%s: %s\n' % (name, text)


def format_exception_only(exc, /, value=_sentinel, *, show_group=False):
    if value is _sentinel:
        value = exc
        exc = type(value)
    if exc is None:
        return ['None\n']
    return [_format_final_line(exc, value)]


def _parse_args(exc, value, tb):
    if value is _sentinel:
        if tb is not _sentinel:
            raise ValueError('Both or neither of value and tb must be given')
        value = exc
        tb = getattr(value, '__traceback__', None)
        exc = type(value)
    elif tb is _sentinel:
        raise ValueError('Both or neither of value and tb must be given')
    return exc, value, tb


def format_exception(exc, /, value=_sentinel, tb=_sentinel, limit=None, chain=True, **kwargs):
    exc, value, tb = _parse_args(exc, value, tb)
    out = []
    if tb is not None:
        out.append('Traceback (most recent call last):\n')
        out.extend(format_tb(tb, limit))
    out.extend(format_exception_only(exc, value))
    return out


def print_exception(exc, /, value=_sentinel, tb=_sentinel, limit=None, file=None, chain=True, **kwargs):
    if file is None:
        file = sys.stderr
    for line in format_exception(exc, value, tb, limit=limit, chain=chain):
        print(line, file=file, end='')


def format_exc(limit=None, chain=True):
    t, v, tb = sys.exc_info()
    if v is None:
        return 'NoneType: None\n'
    return ''.join(format_exception(t, v, tb, limit=limit, chain=chain))


def print_exc(limit=None, file=None, chain=True):
    print_exception(*sys.exc_info(), limit=limit, file=file, chain=chain)


def print_last(limit=None, file=None, chain=True):
    raise ValueError('no last exception')


def clear_frames(tb):
    pass


class TracebackException:
    def __init__(self, exc_type, exc_value, exc_traceback, *, limit=None, lookup_lines=True,
                 capture_locals=False, compact=False, max_group_width=15, max_group_depth=10):
        self.exc_type = exc_type
        self._value = exc_value
        self.stack = StackSummary.extract(walk_tb(exc_traceback), limit=limit, lookup_lines=lookup_lines)
        self.__cause__ = None
        self.__context__ = None
        self.__suppress_context__ = False
        self._str = str(exc_value) if exc_value is not None else ''

    @classmethod
    def from_exception(cls, exc, *args, **kwargs):
        return cls(type(exc), exc, exc.__traceback__, *args, **kwargs)

    @property
    def exc_type_str(self):
        return _type_name(self.exc_type)

    def format_exception_only(self):
        yield _format_final_line(self.exc_type, self._value)

    def format(self, *, chain=True):
        if self.stack:
            yield 'Traceback (most recent call last):\n'
            for line in self.stack.format():
                yield line
        for line in self.format_exception_only():
            yield line

    def print(self, *, file=None, chain=True):
        if file is None:
            file = sys.stderr
        for line in self.format(chain=chain):
            print(line, file=file, end='')
