# O módulo C `_sqlite3` do CPython: a interface DB-API 2.0 sobre o motor do SQLite do sandbox
# (`_sqlite_engine`). O pacote `sqlite3` é o do Debian (`__init__.py` e `dbapi2.py` do disco) e traz daqui
# tudo com `from _sqlite3 import *`; por isso só os nomes públicos do módulo C ficam sem sublinhado, e as
# importações e auxiliares daqui têm o sublinhado que os esconde do `import *` e do `dir()`.

import collections.abc as _cabc
import os as _os

import _sqlite_engine as _engine

_deprecated_version = '2.6.0'

threadsafety = 3
sqlite_version = _engine.sqlite_version

PARSE_DECLTYPES = 1
PARSE_COLNAMES = 2

LEGACY_TRANSACTION_CONTROL = -1

# Códigos de resultado primários do SQLite.
SQLITE_OK = 0
SQLITE_ERROR = 1
SQLITE_INTERNAL = 2
SQLITE_PERM = 3
SQLITE_ABORT = 4
SQLITE_BUSY = 5
SQLITE_LOCKED = 6
SQLITE_NOMEM = 7
SQLITE_READONLY = 8
SQLITE_INTERRUPT = 9
SQLITE_IOERR = 10
SQLITE_CORRUPT = 11
SQLITE_NOTFOUND = 12
SQLITE_FULL = 13
SQLITE_CANTOPEN = 14
SQLITE_PROTOCOL = 15
SQLITE_EMPTY = 16
SQLITE_SCHEMA = 17
SQLITE_TOOBIG = 18
SQLITE_CONSTRAINT = 19
SQLITE_MISMATCH = 20
SQLITE_MISUSE = 21
SQLITE_NOLFS = 22
SQLITE_AUTH = 23
SQLITE_FORMAT = 24
SQLITE_RANGE = 25
SQLITE_NOTADB = 26
SQLITE_NOTICE = 27
SQLITE_WARNING = 28
SQLITE_ROW = 100
SQLITE_DONE = 101

# Códigos estendidos: o primário mais o sufixo deslocado oito bits.
SQLITE_OK_LOAD_PERMANENTLY = 0 | (1 << 8)
SQLITE_OK_SYMLINK = 0 | (2 << 8)
SQLITE_ERROR_MISSING_COLLSEQ = 1 | (1 << 8)
SQLITE_ERROR_RETRY = 1 | (2 << 8)
SQLITE_ERROR_SNAPSHOT = 1 | (3 << 8)
SQLITE_ABORT_ROLLBACK = 4 | (2 << 8)
SQLITE_BUSY_RECOVERY = 5 | (1 << 8)
SQLITE_BUSY_SNAPSHOT = 5 | (2 << 8)
SQLITE_BUSY_TIMEOUT = 5 | (3 << 8)
SQLITE_LOCKED_SHAREDCACHE = 6 | (1 << 8)
SQLITE_LOCKED_VTAB = 6 | (2 << 8)
SQLITE_READONLY_RECOVERY = 8 | (1 << 8)
SQLITE_READONLY_CANTLOCK = 8 | (2 << 8)
SQLITE_READONLY_ROLLBACK = 8 | (3 << 8)
SQLITE_READONLY_DBMOVED = 8 | (4 << 8)
SQLITE_READONLY_CANTINIT = 8 | (5 << 8)
SQLITE_READONLY_DIRECTORY = 8 | (6 << 8)
SQLITE_IOERR_READ = 10 | (1 << 8)
SQLITE_IOERR_SHORT_READ = 10 | (2 << 8)
SQLITE_IOERR_WRITE = 10 | (3 << 8)
SQLITE_IOERR_FSYNC = 10 | (4 << 8)
SQLITE_IOERR_DIR_FSYNC = 10 | (5 << 8)
SQLITE_IOERR_TRUNCATE = 10 | (6 << 8)
SQLITE_IOERR_FSTAT = 10 | (7 << 8)
SQLITE_IOERR_UNLOCK = 10 | (8 << 8)
SQLITE_IOERR_RDLOCK = 10 | (9 << 8)
SQLITE_IOERR_DELETE = 10 | (10 << 8)
SQLITE_IOERR_BLOCKED = 10 | (11 << 8)
SQLITE_IOERR_NOMEM = 10 | (12 << 8)
SQLITE_IOERR_ACCESS = 10 | (13 << 8)
SQLITE_IOERR_CHECKRESERVEDLOCK = 10 | (14 << 8)
SQLITE_IOERR_LOCK = 10 | (15 << 8)
SQLITE_IOERR_CLOSE = 10 | (16 << 8)
SQLITE_IOERR_DIR_CLOSE = 10 | (17 << 8)
SQLITE_IOERR_SHMOPEN = 10 | (18 << 8)
SQLITE_IOERR_SHMSIZE = 10 | (19 << 8)
SQLITE_IOERR_SHMLOCK = 10 | (20 << 8)
SQLITE_IOERR_SHMMAP = 10 | (21 << 8)
SQLITE_IOERR_SEEK = 10 | (22 << 8)
SQLITE_IOERR_DELETE_NOENT = 10 | (23 << 8)
SQLITE_IOERR_MMAP = 10 | (24 << 8)
SQLITE_IOERR_GETTEMPPATH = 10 | (25 << 8)
SQLITE_IOERR_CONVPATH = 10 | (26 << 8)
SQLITE_IOERR_VNODE = 10 | (27 << 8)
SQLITE_IOERR_AUTH = 10 | (28 << 8)
SQLITE_IOERR_BEGIN_ATOMIC = 10 | (29 << 8)
SQLITE_IOERR_COMMIT_ATOMIC = 10 | (30 << 8)
SQLITE_IOERR_ROLLBACK_ATOMIC = 10 | (31 << 8)
SQLITE_IOERR_DATA = 10 | (32 << 8)
SQLITE_IOERR_CORRUPTFS = 10 | (33 << 8)
SQLITE_CORRUPT_VTAB = 11 | (1 << 8)
SQLITE_CORRUPT_SEQUENCE = 11 | (2 << 8)
SQLITE_CORRUPT_INDEX = 11 | (3 << 8)
SQLITE_CANTOPEN_NOTEMPDIR = 14 | (1 << 8)
SQLITE_CANTOPEN_ISDIR = 14 | (2 << 8)
SQLITE_CANTOPEN_FULLPATH = 14 | (3 << 8)
SQLITE_CANTOPEN_CONVPATH = 14 | (4 << 8)
SQLITE_CANTOPEN_DIRTYWAL = 14 | (5 << 8)
SQLITE_CANTOPEN_SYMLINK = 14 | (6 << 8)
SQLITE_CONSTRAINT_CHECK = 19 | (1 << 8)
SQLITE_CONSTRAINT_COMMITHOOK = 19 | (2 << 8)
SQLITE_CONSTRAINT_FOREIGNKEY = 19 | (3 << 8)
SQLITE_CONSTRAINT_FUNCTION = 19 | (4 << 8)
SQLITE_CONSTRAINT_NOTNULL = 19 | (5 << 8)
SQLITE_CONSTRAINT_PRIMARYKEY = 19 | (6 << 8)
SQLITE_CONSTRAINT_TRIGGER = 19 | (7 << 8)
SQLITE_CONSTRAINT_UNIQUE = 19 | (8 << 8)
SQLITE_CONSTRAINT_VTAB = 19 | (9 << 8)
SQLITE_CONSTRAINT_ROWID = 19 | (10 << 8)
SQLITE_CONSTRAINT_PINNED = 19 | (11 << 8)
SQLITE_AUTH_USER = 23 | (1 << 8)
SQLITE_NOTICE_RECOVER_WAL = 27 | (1 << 8)
SQLITE_NOTICE_RECOVER_ROLLBACK = 27 | (2 << 8)
SQLITE_WARNING_AUTOINDEX = 28 | (1 << 8)

# Retornos do autorizador e códigos de ação.
SQLITE_DENY = 1
SQLITE_IGNORE = 2
SQLITE_CREATE_INDEX = 1
SQLITE_CREATE_TABLE = 2
SQLITE_CREATE_TEMP_INDEX = 3
SQLITE_CREATE_TEMP_TABLE = 4
SQLITE_CREATE_TEMP_TRIGGER = 5
SQLITE_CREATE_TEMP_VIEW = 6
SQLITE_CREATE_TRIGGER = 7
SQLITE_CREATE_VIEW = 8
SQLITE_DELETE = 9
SQLITE_DROP_INDEX = 10
SQLITE_DROP_TABLE = 11
SQLITE_DROP_TEMP_INDEX = 12
SQLITE_DROP_TEMP_TABLE = 13
SQLITE_DROP_TEMP_TRIGGER = 14
SQLITE_DROP_TEMP_VIEW = 15
SQLITE_DROP_TRIGGER = 16
SQLITE_DROP_VIEW = 17
SQLITE_INSERT = 18
SQLITE_PRAGMA = 19
SQLITE_READ = 20
SQLITE_SELECT = 21
SQLITE_TRANSACTION = 22
SQLITE_UPDATE = 23
SQLITE_ATTACH = 24
SQLITE_DETACH = 25
SQLITE_ALTER_TABLE = 26
SQLITE_REINDEX = 27
SQLITE_ANALYZE = 28
SQLITE_CREATE_VTABLE = 29
SQLITE_DROP_VTABLE = 30
SQLITE_FUNCTION = 31
SQLITE_SAVEPOINT = 32
SQLITE_RECURSIVE = 33

# Categorias de limite de `Connection.getlimit`/`setlimit`.
SQLITE_LIMIT_LENGTH = 0
SQLITE_LIMIT_SQL_LENGTH = 1
SQLITE_LIMIT_COLUMN = 2
SQLITE_LIMIT_EXPR_DEPTH = 3
SQLITE_LIMIT_COMPOUND_SELECT = 4
SQLITE_LIMIT_VDBE_OP = 5
SQLITE_LIMIT_FUNCTION_ARG = 6
SQLITE_LIMIT_ATTACHED = 7
SQLITE_LIMIT_LIKE_PATTERN_LENGTH = 8
SQLITE_LIMIT_VARIABLE_NUMBER = 9
SQLITE_LIMIT_TRIGGER_DEPTH = 10
SQLITE_LIMIT_WORKER_THREADS = 11

# Opções de `Connection.getconfig`/`setconfig`.
SQLITE_DBCONFIG_ENABLE_FKEY = 1002
SQLITE_DBCONFIG_ENABLE_TRIGGER = 1003
SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER = 1004
SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION = 1005
SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE = 1006
SQLITE_DBCONFIG_ENABLE_QPSG = 1007
SQLITE_DBCONFIG_TRIGGER_EQP = 1008
SQLITE_DBCONFIG_RESET_DATABASE = 1009
SQLITE_DBCONFIG_DEFENSIVE = 1010
SQLITE_DBCONFIG_WRITABLE_SCHEMA = 1011
SQLITE_DBCONFIG_LEGACY_ALTER_TABLE = 1012
SQLITE_DBCONFIG_DQS_DML = 1013
SQLITE_DBCONFIG_DQS_DDL = 1014
SQLITE_DBCONFIG_ENABLE_VIEW = 1015
SQLITE_DBCONFIG_LEGACY_FILE_FORMAT = 1016
SQLITE_DBCONFIG_TRUSTED_SCHEMA = 1017


class Warning(Exception):
    __module__ = 'sqlite3'


class Error(Exception):
    __module__ = 'sqlite3'


class InterfaceError(Error):
    __module__ = 'sqlite3'


class DatabaseError(Error):
    __module__ = 'sqlite3'


class DataError(DatabaseError):
    __module__ = 'sqlite3'


class OperationalError(DatabaseError):
    __module__ = 'sqlite3'


class IntegrityError(DatabaseError):
    __module__ = 'sqlite3'


class InternalError(DatabaseError):
    __module__ = 'sqlite3'


class ProgrammingError(DatabaseError):
    __module__ = 'sqlite3'


class NotSupportedError(DatabaseError):
    __module__ = 'sqlite3'


_ERRORS = {
    'Warning': Warning, 'Error': Error, 'InterfaceError': InterfaceError, 'DatabaseError': DatabaseError,
    'DataError': DataError, 'OperationalError': OperationalError, 'IntegrityError': IntegrityError,
    'InternalError': InternalError, 'ProgrammingError': ProgrammingError,
    'NotSupportedError': NotSupportedError, 'MemoryError': MemoryError, 'OverflowError': OverflowError,
}

adapters = {}
converters = {}

_UNSET = object()


class PrepareProtocol:
    __module__ = 'sqlite3'


def register_adapter(type, adapter, /):
    adapters[(type, PrepareProtocol)] = adapter


def register_converter(typename, converter, /):
    converters[typename.upper()] = converter


def adapt(obj, proto=PrepareProtocol, alt=_UNSET, /):
    fn = adapters.get((type(obj), proto))
    if fn is not None:
        return fn(obj)
    conform = getattr(obj, '__conform__', None)
    if conform is not None:
        out = conform(proto)
        if out is not None:
            return out
    if alt is not _UNSET:
        return alt
    raise ProgrammingError('can not adapt type %r' % type(obj).__name__)


def enable_callback_tracebacks(enable, /):
    pass


def _raise(res):
    """Levanta a exceção que o `_sqlite_engine` descreveu como `(tipo, mensagem)`."""
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
    __module__ = 'sqlite3'

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
    __module__ = 'sqlite3'

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
        if isinstance(parameters, _cabc.Mapping):
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
        if first in _DML and conn._isolation_level is not None and not conn.in_transaction \
                and conn.autocommit == LEGACY_TRANSACTION_CONTROL:
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
        if first in ('INSERT', 'REPLACE') and not accumulate:
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


def _quote_name(name):
    return '"' + name.replace('"', '""') + '"'


class Blob:
    """Acesso a um BLOB pelo SQL: lê e grava por `substr` sobre a linha, sem a API incremental do SQLite."""
    __module__ = 'sqlite3'

    def __new__(cls, *args, **kwargs):
        raise TypeError("cannot create 'sqlite3.Blob' instances")

    @staticmethod
    def _open(conn, table, column, row, readonly, name):
        blob = object.__new__(Blob)
        blob._conn = conn
        blob._where = '%s.%s' % (_quote_name(name), _quote_name(table))
        blob._column = _quote_name(column)
        blob._row = row
        blob._readonly = readonly
        blob._offset = 0
        blob._open_flag = True
        blob._length = blob._query('SELECT length(CAST(%s AS BLOB)) FROM %s WHERE rowid = ?' % (blob._column, blob._where),
                                   (row,))
        if blob._length is None:
            raise OperationalError('no such rowid: %d' % row)
        return blob

    def _query(self, sql, params):
        res = self._conn._h.run(sql, params)
        _raise(res)
        rows = res[3]
        return rows[0][0] if rows else None

    def _check(self):
        if not self._open_flag or self._conn._closed:
            raise ProgrammingError('Cannot operate on a closed blob.')

    def close(self):
        self._open_flag = False

    def read(self, length=-1, /):
        self._check()
        if length < 0 or length > self._length - self._offset:
            length = self._length - self._offset
        if length == 0:
            return b''
        data = self._query('SELECT substr(CAST(%s AS BLOB), ?, ?) FROM %s WHERE rowid = ?' % (self._column, self._where),
                           (self._offset + 1, length, self._row))
        self._offset += length
        return data

    def _write_at(self, offset, data):
        if self._readonly:
            raise OperationalError('attempt to write a readonly database')
        if offset + len(data) > self._length:
            raise ValueError('data longer than blob length')
        sql = ('UPDATE %s SET %s = substr(CAST(%s AS BLOB), 1, ?) || ? || substr(CAST(%s AS BLOB), ?) WHERE rowid = ?'
               % (self._where, self._column, self._column, self._column))
        _raise(self._conn._h.run(sql, (offset, bytes(data), offset + len(data) + 1, self._row)))

    def write(self, data, /):
        self._check()
        data = bytes(memoryview(data))
        self._write_at(self._offset, data)
        self._offset += len(data)

    def seek(self, offset, origin=0, /):
        self._check()
        if origin == 0:
            pos = offset
        elif origin == 1:
            pos = self._offset + offset
        elif origin == 2:
            pos = self._length + offset
        else:
            raise ValueError('origin should be os.SEEK_SET, os.SEEK_CUR, or os.SEEK_END')
        if pos < 0 or pos > self._length:
            raise ValueError('offset out of blob range')
        self._offset = pos

    def tell(self):
        self._check()
        return self._offset

    def __len__(self):
        self._check()
        return self._length

    def _whole(self):
        if self._length == 0:
            return b''
        return self._query('SELECT CAST(%s AS BLOB) FROM %s WHERE rowid = ?' % (self._column, self._where), (self._row,))

    def __getitem__(self, key):
        self._check()
        if isinstance(key, slice):
            return self._whole()[key]
        index = key.__index__()
        if index < 0:
            index += self._length
        if not 0 <= index < self._length:
            raise IndexError('Blob index out of range')
        return self._whole()[index]

    def __setitem__(self, key, value):
        self._check()
        if isinstance(key, slice):
            start, stop, step = key.indices(self._length)
            data = bytes(memoryview(value))
            if step == 1:
                if len(data) != max(stop - start, 0):
                    raise IndexError('Blob slice assignment must be the same length as the slice')
                self._write_at(start, data)
                return
            positions = range(start, stop, step)
            if len(data) != len(positions):
                raise IndexError('Blob slice assignment must be the same length as the slice')
            for pos, byte in zip(positions, data):
                self._write_at(pos, bytes([byte]))
            return
        index = key.__index__()
        if index < 0:
            index += self._length
        if not 0 <= index < self._length:
            raise IndexError('Blob index out of range')
        if not isinstance(value, int):
            raise TypeError("'%s' object cannot be interpreted as an integer" % type(value).__name__)
        if not 0 <= value < 256:
            raise ValueError('byte must be in range(0, 256)')
        self._write_at(index, bytes([value]))

    def __enter__(self):
        self._check()
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        self.close()
        return False


class Connection:
    __module__ = 'sqlite3'

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
        path = _os.fspath(database)
        if isinstance(path, bytes):
            path = path.decode('utf-8')
        h = _engine.connect(path, bool(uri))
        _raise(h)
        self._h = h
        self._closed = False
        self._detect_types = detect_types
        self._isolation_level = None
        self.isolation_level = isolation_level
        self.row_factory = None
        self.text_factory = str
        self.autocommit = autocommit
        if autocommit is False:
            _raise(self._h.run('BEGIN', ()))

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
            if self.autocommit is False:
                _raise(self._h.run('BEGIN', ()))

    def rollback(self):
        self._check()
        if not self._h.is_autocommit():
            _raise(self._h.run('ROLLBACK', ()))
            if self.autocommit is False:
                _raise(self._h.run('BEGIN', ()))

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

    def blobopen(self, table, column, row, /, *, readonly=False, name='main'):
        self._check()
        return Blob._open(self, table, column, row, readonly, name)

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
