"""sqlite3: a interface DB-API 2.0 sobre o motor do SQLite do sandbox (`_sqlite3`)."""

import collections.abc
import datetime
import os
import time

import _sqlite3

paramstyle = 'qmark'
threadsafety = 3
apilevel = '2.0'
version = '2.6.0'
version_info = (2, 6, 0)
sqlite_version = _sqlite3.sqlite_version
sqlite_version_info = tuple(int(p) for p in sqlite_version.split('.'))

PARSE_DECLTYPES = 1
PARSE_COLNAMES = 2

SQLITE_OK = 0
SQLITE_DENY = 1
SQLITE_IGNORE = 2
LEGACY_TRANSACTION_CONTROL = -1

__all__ = [
    'Binary', 'Connection', 'Cursor', 'DataError', 'DatabaseError', 'Date', 'DateFromTicks', 'Error',
    'IntegrityError', 'InterfaceError', 'InternalError', 'NotSupportedError', 'OperationalError',
    'PARSE_COLNAMES', 'PARSE_DECLTYPES', 'ProgrammingError', 'Row', 'Time', 'TimeFromTicks',
    'Timestamp', 'TimestampFromTicks', 'Warning', 'adapt', 'adapters', 'apilevel', 'complete_statement',
    'connect', 'converters', 'enable_callback_tracebacks', 'paramstyle', 'register_adapter',
    'register_converter', 'sqlite_version', 'sqlite_version_info', 'threadsafety', 'version',
    'version_info']


class Warning(Exception):
    pass


class Error(Exception):
    pass


class InterfaceError(Error):
    pass


class DatabaseError(Error):
    pass


class DataError(DatabaseError):
    pass


class OperationalError(DatabaseError):
    pass


class IntegrityError(DatabaseError):
    pass


class InternalError(DatabaseError):
    pass


class ProgrammingError(DatabaseError):
    pass


class NotSupportedError(DatabaseError):
    pass


_ERRORS = {
    'Warning': Warning, 'Error': Error, 'InterfaceError': InterfaceError, 'DatabaseError': DatabaseError,
    'DataError': DataError, 'OperationalError': OperationalError, 'IntegrityError': IntegrityError,
    'InternalError': InternalError, 'ProgrammingError': ProgrammingError,
    'NotSupportedError': NotSupportedError, 'MemoryError': MemoryError, 'OverflowError': OverflowError,
}

Binary = memoryview
Date = datetime.date
Time = datetime.time
Timestamp = datetime.datetime


def DateFromTicks(ticks):
    return Date(*time.localtime(ticks)[:3])


def TimeFromTicks(ticks):
    return Time(*time.localtime(ticks)[3:6])


def TimestampFromTicks(ticks):
    return Timestamp(*time.localtime(ticks)[:6])


adapters = {}
converters = {}


def register_adapter(typ, adapter):
    adapters[(typ, PrepareProtocol)] = adapter


def register_converter(typename, converter):
    converters[typename.upper()] = converter


class PrepareProtocol:
    pass


def adapt(obj, proto=PrepareProtocol, alt=None):
    fn = adapters.get((type(obj), proto))
    if fn is not None:
        return fn(obj)
    conform = getattr(obj, '__conform__', None)
    if conform is not None:
        out = conform(proto)
        if out is not None:
            return out
    if alt is not None:
        return alt
    raise ProgrammingError('can not adapt type %r' % type(obj).__name__)


def enable_callback_tracebacks(flag):
    pass


def _adapt_date(val):
    return val.isoformat()


def _adapt_datetime(val):
    return val.isoformat(' ')


def _convert_date(val):
    return datetime.date(*map(int, val.split(b'-')))


def _convert_timestamp(val):
    datepart, timepart = val.split(b' ')
    year, month, day = map(int, datepart.split(b'-'))
    timepart_full = timepart.split(b'.')
    hours, minutes, seconds = map(int, timepart_full[0].split(b':'))
    if len(timepart_full) == 2:
        microseconds = int('{:0<6.6}'.format(timepart_full[1].decode()))
    else:
        microseconds = 0
    return datetime.datetime(year, month, day, hours, minutes, seconds, microseconds)


register_adapter(datetime.date, _adapt_date)
register_adapter(datetime.datetime, _adapt_datetime)
register_converter('date', _convert_date)
register_converter('timestamp', _convert_timestamp)


def _raise(res):
    """Levanta a exceção que o `_sqlite3` descreveu como `(tipo, mensagem)`."""
    if isinstance(res, tuple) and res and isinstance(res[0], str):
        raise _ERRORS.get(res[0], DatabaseError)(res[1])


def _first_word(sql):
    i, n = 0, len(sql)
    while i < n:
        if sql[i].isspace():
            i += 1
        elif sql.startswith('--', i):
            j = sql.find('\n', i)
            i = n if j < 0 else j + 1
        elif sql.startswith('/*', i):
            j = sql.find('*/', i + 2)
            i = n if j < 0 else j + 2
        else:
            break
    j = i
    while j < n and (sql[j].isalpha() or sql[j] == '_'):
        j += 1
    return sql[i:j].upper()


_DML = ('INSERT', 'UPDATE', 'DELETE', 'REPLACE')


def complete_statement(statement):
    """Aproximação de `sqlite3_complete`: termina em `;` fora de aspas e comentários."""
    if not isinstance(statement, str):
        raise TypeError('complete_statement() argument 1 must be str, not %s' % type(statement).__name__)
    s = statement
    i, n = 0, len(s)
    last = ''
    while i < n:
        c = s[i]
        if c in '\'"`':
            j = i + 1
            while j < n:
                if s[j] == c:
                    if j + 1 < n and s[j + 1] == c:
                        j += 2
                        continue
                    break
                j += 1
            else:
                return False
            i, last = j + 1, 'q'
        elif c == '[':
            j = s.find(']', i)
            if j < 0:
                return False
            i, last = j + 1, 'q'
        elif s.startswith('--', i):
            j = s.find('\n', i)
            i = n if j < 0 else j + 1
        elif s.startswith('/*', i):
            j = s.find('*/', i + 2)
            if j < 0:
                return False
            i = j + 2
        elif c.isspace():
            i += 1
        else:
            last = c
            i += 1
    if last != ';':
        return False
    words = s.upper().replace(';', ' ; ').split()
    if len(words) > 1 and words[0] == 'CREATE' and 'TRIGGER' in words[:4]:
        return len(words) >= 3 and words[-2] == 'END'
    return True


def _decl_name(decl):
    if decl is None:
        return None
    word = decl.split(' ')[0].split('(')[0]
    return word.upper()


class Row:

    def __init__(self, cursor, data):
        if not isinstance(cursor, Cursor):
            raise TypeError('argument 1 must be sqlite3.Cursor, not %s' % type(cursor).__name__)
        if not isinstance(data, tuple):
            raise TypeError('argument 2 must be tuple, not %s' % type(data).__name__)
        self._data = data
        self._keys = tuple(d[0] for d in (cursor.description or ()))

    def keys(self):
        return list(self._keys)

    def __len__(self):
        return len(self._data)

    def __iter__(self):
        return iter(self._data)

    def __getitem__(self, key):
        if isinstance(key, str):
            low = key.lower()
            for i, k in enumerate(self._keys):
                if k.lower() == low:
                    return self._data[i]
            raise IndexError('No item with that key')
        return self._data[key]

    def __eq__(self, other):
        if not isinstance(other, Row):
            return NotImplemented
        return self._keys == other._keys and self._data == other._data

    def __ne__(self, other):
        r = self.__eq__(other)
        return r if r is NotImplemented else not r

    def __hash__(self):
        return hash(self._keys) ^ hash(self._data)


class Cursor:

    def __init__(self, connection):
        if not isinstance(connection, Connection):
            raise TypeError('argument 1 must be sqlite3.Connection, not %s' % type(connection).__name__)
        self.connection = connection
        self.arraysize = 1
        self.row_factory = None
        self.lastrowid = None
        self.rowcount = -1
        self.description = None
        self._rows = []
        self._pos = 0
        self._closed = False
        self._decls = ()

    def _check(self):
        if self._closed:
            raise ProgrammingError('Cannot operate on a closed cursor.')
        if self.connection._closed:
            raise ProgrammingError('Cannot operate on a closed database.')

    def _convert_params(self, parameters):
        if isinstance(parameters, dict):
            return {k: _adapt_value(v) for k, v in parameters.items()}
        if isinstance(parameters, collections.abc.Mapping):
            return {k: _adapt_value(parameters[k]) for k in parameters.keys()}
        if isinstance(parameters, (str, bytes, bytearray)):
            raise ProgrammingError('parameters are of unsupported type')
        try:
            items = tuple(parameters)
        except TypeError:
            raise ProgrammingError('parameters are of unsupported type') from None
        return tuple(_adapt_value(v) for v in items)

    def _run(self, sql, parameters):
        conn = self.connection
        first = _first_word(sql)
        if first in _DML and conn._isolation_level is not None and not conn.in_transaction:
            conn._begin()
        res = conn._h.run(sql, self._convert_params(parameters))
        _raise(res)
        return first, res

    def _set_result(self, first, res, accumulate=False):
        _, names, decls, rows, changes, rowid = res
        conn = self.connection
        if names:
            fixed = []
            self._colconv = []
            for i, name in enumerate(names):
                conv = None
                if conn._detect_types & PARSE_COLNAMES and '[' in name:
                    idx = name.index('[')
                    end = name.find(']', idx)
                    if end > idx:
                        conv = converters.get(name[idx + 1:end].upper())
                        name = name[:idx - 1] if idx > 0 else name[:idx]
                if conv is None and conn._detect_types & PARSE_DECLTYPES:
                    conv = converters.get(_decl_name(decls[i]))
                fixed.append(name)
                self._colconv.append(conv)
            self.description = tuple((n, None, None, None, None, None, None) for n in fixed)
            self._rows = rows
            self._pos = 0
            self.rowcount = -1
        else:
            self.description = None
            self._rows = []
            self._pos = 0
            if first in _DML:
                self.rowcount = (self.rowcount if accumulate and self.rowcount >= 0 else 0) + changes
            elif not accumulate:
                self.rowcount = -1
        if first in ('INSERT', 'REPLACE'):
            self.lastrowid = rowid

    def execute(self, sql, parameters=()):
        self._check()
        if not isinstance(sql, str):
            raise TypeError('execute() argument 1 must be str, not %s' % type(sql).__name__)
        first, res = self._run(sql, parameters)
        self._set_result(first, res)
        return self

    def executemany(self, sql, seq_of_parameters):
        self._check()
        if not isinstance(sql, str):
            raise TypeError('executemany() argument 1 must be str, not %s' % type(sql).__name__)
        self.rowcount = 0
        self._rows = []
        self._pos = 0
        self.description = None
        for parameters in seq_of_parameters:
            first, res = self._run(sql, parameters)
            if res[1]:
                raise ProgrammingError('executemany() can only execute DML statements.')
            self._set_result(first, res, accumulate=True)
        return self

    def executescript(self, sql_script):
        self._check()
        if not isinstance(sql_script, str):
            raise TypeError('executescript() argument 1 must be str, not %s' % type(sql_script).__name__)
        conn = self.connection
        if conn._isolation_level is not None and conn.in_transaction:
            conn.commit()
        _raise(conn._h.script(sql_script))
        self.description = None
        self._rows = []
        return self

    def _make_row(self, values):
        conv = self._colconv if self.description else ()
        out = []
        text_factory = self.connection.text_factory
        for i, v in enumerate(values):
            c = conv[i] if i < len(conv) else None
            if c is not None and v is not None:
                raw = v if isinstance(v, bytes) else str(v).encode('utf-8')
                v = c(raw)
            elif isinstance(v, str) and text_factory is not str:
                v = text_factory(v.encode('utf-8'))
            out.append(v)
        row = tuple(out)
        factory = self.row_factory
        if factory is not None:
            return factory(self, row)
        return row

    def fetchone(self):
        self._check()
        if self._pos >= len(self._rows):
            return None
        row = self._rows[self._pos]
        self._pos += 1
        return self._make_row(row)

    def fetchmany(self, size=None):
        self._check()
        if size is None:
            size = self.arraysize
        out = []
        while len(out) < size:
            row = self.fetchone()
            if row is None:
                break
            out.append(row)
        return out

    def fetchall(self):
        self._check()
        out = []
        while True:
            row = self.fetchone()
            if row is None:
                return out
            out.append(row)

    def __iter__(self):
        return self

    def __next__(self):
        row = self.fetchone()
        if row is None:
            raise StopIteration
        return row

    def close(self):
        if self.connection._closed:
            raise ProgrammingError('Cannot operate on a closed database.')
        self._closed = True

    def setinputsizes(self, sizes):
        pass

    def setoutputsize(self, size, column=None):
        pass


def _adapt_value(v):
    t = type(v)
    if t in (int, float, str, bytes, bool, type(None)):
        return v
    fn = adapters.get((t, PrepareProtocol))
    if fn is not None:
        return fn(v)
    conform = getattr(v, '__conform__', None)
    if conform is not None:
        out = conform(PrepareProtocol)
        if out is not None:
            return out
    return v


class Connection:
    Warning = Warning
    Error = Error
    InterfaceError = InterfaceError
    DatabaseError = DatabaseError
    DataError = DataError
    OperationalError = OperationalError
    IntegrityError = IntegrityError
    InternalError = InternalError
    ProgrammingError = ProgrammingError
    NotSupportedError = NotSupportedError

    def __init__(self, database, timeout=5.0, detect_types=0, isolation_level='', check_same_thread=True,
                 factory=None, cached_statements=128, uri=False, *, autocommit=LEGACY_TRANSACTION_CONTROL):
        if isinstance(database, bytes):
            database = database.decode('utf-8')
        path = os.fspath(database)
        if isinstance(path, bytes):
            path = path.decode('utf-8')
        h = _sqlite3.connect(path, bool(uri))
        _raise(h)
        self._h = h
        self._closed = False
        self._detect_types = detect_types
        self._isolation_level = None
        self.isolation_level = isolation_level
        self.row_factory = None
        self.text_factory = str
        self.autocommit = autocommit

    @property
    def isolation_level(self):
        return self._isolation_level

    @isolation_level.setter
    def isolation_level(self, level):
        if level is None:
            if not self._closed and self.in_transaction:
                self.commit()
            self._isolation_level = None
            return
        if not isinstance(level, str):
            raise TypeError('isolation_level must be str or None')
        if level == '':
            self._isolation_level = ''
            return
        norm = level.upper()
        if norm not in ('DEFERRED', 'IMMEDIATE', 'EXCLUSIVE'):
            raise ValueError('isolation_level string must be \'\', \'DEFERRED\', \'IMMEDIATE\', or \'EXCLUSIVE\'')
        self._isolation_level = level

    def _check(self):
        if self._closed:
            raise ProgrammingError('Cannot operate on a closed database.')

    @property
    def in_transaction(self):
        self._check()
        return not self._h.is_autocommit()

    @property
    def total_changes(self):
        self._check()
        return self._h.total_changes()

    def _begin(self):
        level = self._isolation_level
        sql = 'BEGIN' if not level else 'BEGIN ' + level
        _raise(self._h.run(sql, ()))

    def cursor(self, factory=Cursor):
        self._check()
        cur = factory(self)
        if self.row_factory is not None:
            cur.row_factory = self.row_factory
        return cur

    def execute(self, sql, parameters=()):
        return self.cursor().execute(sql, parameters)

    def executemany(self, sql, seq_of_parameters):
        return self.cursor().executemany(sql, seq_of_parameters)

    def executescript(self, sql_script):
        return self.cursor().executescript(sql_script)

    def commit(self):
        self._check()
        if not self._h.is_autocommit():
            _raise(self._h.run('COMMIT', ()))

    def rollback(self):
        self._check()
        if not self._h.is_autocommit():
            _raise(self._h.run('ROLLBACK', ()))

    def close(self):
        if self._closed:
            return
        self._closed = True
        self._h.close()

    def __enter__(self):
        self._check()
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        if exc_type is None:
            self.commit()
        else:
            try:
                self.rollback()
            except Error:
                pass
        return False

    def create_function(self, name, narg, func, *, deterministic=False):
        self._check()
        if func is None:
            raise NotSupportedError('removing a function is not supported')
        _raise(self._h.create_function(name, narg, func, deterministic))

    def create_aggregate(self, name, n_arg, aggregate_class):
        self._check()
        _raise(self._h.create_aggregate(name, n_arg, aggregate_class))

    def create_collation(self, name, callable):
        self._check()
        if callable is None:
            raise NotSupportedError('removing a collation is not supported')
        _raise(self._h.create_collation(name, callable))

    def create_window_function(self, name, num_params, aggregate_class):
        raise NotSupportedError('window functions are not supported')

    def set_authorizer(self, authorizer_callback):
        self._check()

    def set_progress_handler(self, progress_handler, n):
        self._check()

    def set_trace_callback(self, trace_callback):
        self._check()

    def enable_load_extension(self, enabled):
        raise NotSupportedError('loadable extension support is not enabled')

    def load_extension(self, path, *, entrypoint=None):
        raise NotSupportedError('loadable extension support is not enabled')

    def interrupt(self):
        self._check()

    def iterdump(self, *, filter=None):
        from sqlite3.dump import _iterdump
        return _iterdump(self, filter=filter)

    def backup(self, target, *, pages=-1, progress=None, name='main', sleep=0.250):
        if not isinstance(target, Connection):
            raise TypeError('target must be sqlite3.Connection, not %s' % type(target).__name__)
        if target is self:
            raise ValueError('target cannot be the same connection instance')
        if target.in_transaction:
            raise OperationalError('target is in transaction')
        target.executescript('\n'.join(self.iterdump()))
        if progress is not None:
            progress(0, 0, 0)

    def __call__(self, sql):
        raise NotSupportedError('Connection objects are not callable')


def connect(database, timeout=5.0, detect_types=0, isolation_level='', check_same_thread=True,
            factory=Connection, cached_statements=128, uri=False, *, autocommit=LEGACY_TRANSACTION_CONTROL):
    return factory(database, timeout, detect_types, isolation_level, check_same_thread, None,
                   cached_statements, uri, autocommit=autocommit)
