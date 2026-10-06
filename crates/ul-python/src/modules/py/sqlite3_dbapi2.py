"""sqlite3.dbapi2: o mesmo conteúdo do pacote `sqlite3`."""

from sqlite3 import *
from sqlite3 import (Binary, Date, DateFromTicks, Time, TimeFromTicks, Timestamp, TimestampFromTicks,
                     adapt, adapters, converters, register_adapter, register_converter, sqlite_version,
                     sqlite_version_info, version, version_info, paramstyle, threadsafety, apilevel,
                     complete_statement, connect, enable_callback_tracebacks, PARSE_COLNAMES,
                     PARSE_DECLTYPES, Connection, Cursor, Row)
