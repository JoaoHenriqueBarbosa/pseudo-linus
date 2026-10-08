
#ifdef SQLITE_ENABLE_CEROD
/*
** Specify the activation key for a CEROD database.  Unless
** activated, none of the CEROD routines will work.
*/
SQLITE_API void sqlite3_activate_cerod(
  const char *zPassPhrase        /* Activation phrase */
);
#endif

/*
** CAPI3REF: Suspend Execution For A Short Time
**
** The sqlite3_sleep() function causes the current thread to suspend execution
** for at least a number of milliseconds specified in its parameter.
**
** If the operating system does not support sleep requests with
** millisecond time resolution, then the time will be rounded up to
** the nearest second. The number of milliseconds of sleep actually
** requested from the operating system is returned.
**
** ^SQLite implements this interface by calling the xSleep()
** method of the default [sqlite3_vfs] object.  If the xSleep() method
** of the default VFS is not implemented correctly, or not implemented at
** all, then the behavior of sqlite3_sleep() may deviate from the description
** in the previous paragraphs.
**
** If a negative argument is passed to sqlite3_sleep() the results vary by
** VFS and operating system.  Some system treat a negative argument as an
** instruction to sleep forever.  Others understand it to mean do not sleep
** at all. ^In SQLite version 3.42.0 and later, a negative
** argument passed into sqlite3_sleep() is changed to zero before it is relayed
** down into the xSleep method of the VFS.
*/
SQLITE_API int sqlite3_sleep(int);

/*
** CAPI3REF: Name Of The Folder Holding Temporary Files
**
** ^(If this global variable is made to point to a string which is
** the name of a folder (a.k.a. directory), then all temporary files
** created by SQLite when using a built-in [sqlite3_vfs | VFS]
** will be placed in that directory.)^  ^If this variable
** is a NULL pointer, then SQLite performs a search for an appropriate
** temporary file directory.
**
** Applications are strongly discouraged from using this global variable.
** It is required to set a temporary folder on Windows Runtime (WinRT).
** But for all other platforms, it is highly recommended that applications
** neither read nor write this variable.  This global variable is a relic
** that exists for backwards compatibility of legacy applications and should
** be avoided in new projects.
**
** It is not safe to read or modify this variable in more than one
** thread at a time.  It is not safe to read or modify this variable
** if a [database connection] is being used at the same time in a separate
** thread.
** It is intended that this variable be set once
** as part of process initialization and before any SQLite interface
** routines have been called and that this variable remain unchanged
** thereafter.
**
** ^The [temp_store_directory pragma] may modify this variable and cause
** it to point to memory obtained from [sqlite3_malloc].  ^Furthermore,
** the [temp_store_directory pragma] always assumes that any string
** that this variable points to is held in memory obtained from
** [sqlite3_malloc] and the pragma may attempt to free that memory
** using [sqlite3_free].
** Hence, if this variable is modified directly, either it should be
** made NULL or made to point to memory obtained from [sqlite3_malloc]
** or else the use of the [temp_store_directory pragma] should be avoided.
** Except when requested by the [temp_store_directory pragma], SQLite
** does not free the memory that sqlite3_temp_directory points to.  If
** the application wants that memory to be freed, it must do
** so itself, taking care to only do so after all [database connection]
** objects have been destroyed.
**
** <b>Note to Windows Runtime users:</b>  The temporary directory must be set
** prior to calling [sqlite3_open] or [sqlite3_open_v2].  Otherwise, various
** features that require the use of temporary files may fail.  Here is an
** example of how to do this using C++ with the Windows Runtime:
**
** <blockquote><pre>
** LPCWSTR zPath = Windows::Storage::ApplicationData::Current->
** &nbsp;     TemporaryFolder->Path->Data();
** char zPathBuf&#91;MAX_PATH + 1&#93;;
** memset(zPathBuf, 0, sizeof(zPathBuf));
** WideCharToMultiByte(CP_UTF8, 0, zPath, -1, zPathBuf, sizeof(zPathBuf),
** &nbsp;     NULL, NULL);
** sqlite3_temp_directory = sqlite3_mprintf("%s", zPathBuf);
** </pre></blockquote>
*/
SQLITE_API char *sqlite3_temp_directory;

/*
** CAPI3REF: Name Of The Folder Holding Database Files
**
** ^(If this global variable is made to point to a string which is
** the name of a folder (a.k.a. directory), then all database files
** specified with a relative pathname and created or accessed by
** SQLite when using a built-in windows [sqlite3_vfs | VFS] will be assumed
** to be relative to that directory.)^ ^If this variable is a NULL
** pointer, then SQLite assumes that all database files specified
** with a relative pathname are relative to the current directory
** for the process.  Only the windows VFS makes use of this global
** variable; it is ignored by the unix VFS.
**
** Changing the value of this variable while a database connection is
** open can result in a corrupt database.
**
** It is not safe to read or modify this variable in more than one
** thread at a time.  It is not safe to read or modify this variable
** if a [database connection] is being used at the same time in a separate
** thread.
** It is intended that this variable be set once
** as part of process initialization and before any SQLite interface
** routines have been called and that this variable remain unchanged
** thereafter.
**
** ^The [data_store_directory pragma] may modify this variable and cause
** it to point to memory obtained from [sqlite3_malloc].  ^Furthermore,
** the [data_store_directory pragma] always assumes that any string
** that this variable points to is held in memory obtained from
** [sqlite3_malloc] and the pragma may attempt to free that memory
** using [sqlite3_free].
** Hence, if this variable is modified directly, either it should be
** made NULL or made to point to memory obtained from [sqlite3_malloc]
** or else the use of the [data_store_directory pragma] should be avoided.
*/
SQLITE_API char *sqlite3_data_directory;

/*
** CAPI3REF: Win32 Specific Interface
**
** These interfaces are available only on Windows.  The
** [sqlite3_win32_set_directory] interface is used to set the value associated
** with the [sqlite3_temp_directory] or [sqlite3_data_directory] variable, to
** zValue, depending on the value of the type parameter.  The zValue parameter
** should be NULL to cause the previous value to be freed via [sqlite3_free];
** a non-NULL value will be copied into memory obtained from [sqlite3_malloc]
** prior to being used.  The [sqlite3_win32_set_directory] interface returns
** [SQLITE_OK] to indicate success, [SQLITE_ERROR] if the type is unsupported,
** or [SQLITE_NOMEM] if memory could not be allocated.  The value of the
** [sqlite3_data_directory] variable is intended to act as a replacement for
** the current directory on the sub-platforms of Win32 where that concept is
** not present, e.g. WinRT and UWP.  The [sqlite3_win32_set_directory8] and
** [sqlite3_win32_set_directory16] interfaces behave exactly the same as the
** sqlite3_win32_set_directory interface except the string parameter must be
** UTF-8 or UTF-16, respectively.
*/
SQLITE_API int sqlite3_win32_set_directory(
  unsigned long type, /* Identifier for directory being set or reset */
  void *zValue        /* New value for directory being set or reset */
);
SQLITE_API int sqlite3_win32_set_directory8(unsigned long type, const char *zValue);
SQLITE_API int sqlite3_win32_set_directory16(unsigned long type, const void *zValue);

/*
** CAPI3REF: Win32 Directory Types
**
** These macros are only available on Windows.  They define the allowed values
** for the type argument to the [sqlite3_win32_set_directory] interface.
*/
#define SQLITE_WIN32_DATA_DIRECTORY_TYPE  1
#define SQLITE_WIN32_TEMP_DIRECTORY_TYPE  2

/*
** CAPI3REF: Test For Auto-Commit Mode
** KEYWORDS: {autocommit mode}
** METHOD: sqlite3
**
** ^The sqlite3_get_autocommit() interface returns non-zero or
** zero if the given database connection is or is not in autocommit mode,
** respectively.  ^Autocommit mode is on by default.
** ^Autocommit mode is disabled by a [BEGIN] statement.
** ^Autocommit mode is re-enabled by a [COMMIT] or [ROLLBACK].
**
** If certain kinds of errors occur on a statement within a multi-statement
** transaction (errors including [SQLITE_FULL], [SQLITE_IOERR],
** [SQLITE_NOMEM], [SQLITE_BUSY], and [SQLITE_INTERRUPT]) then the
** transaction might be rolled back automatically.  The only way to
** find out whether SQLite automatically rolled back the transaction after
** an error is to use this function.
**
** If another thread changes the autocommit status of the database
** connection while this routine is running, then the return value
** is undefined.
*/
SQLITE_API int sqlite3_get_autocommit(sqlite3*);

/*
** CAPI3REF: Find The Database Handle Of A Prepared Statement
** METHOD: sqlite3_stmt
**
** ^The sqlite3_db_handle interface returns the [database connection] handle
** to which a [prepared statement] belongs.  ^The [database connection]
** returned by sqlite3_db_handle is the same [database connection]
** that was the first argument
** to the [sqlite3_prepare_v2()] call (or its variants) that was used to
** create the statement in the first place.
*/
SQLITE_API sqlite3 *sqlite3_db_handle(sqlite3_stmt*);

/*
** CAPI3REF: Return The Schema Name For A Database Connection
** METHOD: sqlite3
**
** ^The sqlite3_db_name(D,N) interface returns a pointer to the schema name
** for the N-th database on database connection D, or a NULL pointer of N is
** out of range.  An N value of 0 means the main database file.  An N of 1 is
** the "temp" schema.  Larger values of N correspond to various ATTACH-ed
** databases.
**
** Space to hold the string that is returned by sqlite3_db_name() is managed
** by SQLite itself.  The string might be deallocated by any operation that
** changes the schema, including [ATTACH] or [DETACH] or calls to
** [sqlite3_serialize()] or [sqlite3_deserialize()], even operations that
** occur on a different thread.  Applications that need to
** remember the string long-term should make their own copy.  Applications that
** are accessing the same database connection simultaneously on multiple
** threads should mutex-protect calls to this API and should make their own
** private copy of the result prior to releasing the mutex.
*/
SQLITE_API const char *sqlite3_db_name(sqlite3 *db, int N);

/*
** CAPI3REF: Return The Filename For A Database Connection
** METHOD: sqlite3
**
** ^The sqlite3_db_filename(D,N) interface returns a pointer to the filename
** associated with database N of connection D.
** ^If there is no attached database N on the database
** connection D, or if database N is a temporary or in-memory database, then
** this function will return either a NULL pointer or an empty string.
**
** ^The string value returned by this routine is owned and managed by
** the database connection.  ^The value will be valid until the database N
** is [DETACH]-ed or until the database connection closes.
**
** ^The filename returned by this function is the output of the
** xFullPathname method of the [VFS].  ^In other words, the filename
** will be an absolute pathname, even if the filename used
** to open the database originally was a URI or relative pathname.
**
** If the filename pointer returned by this routine is not NULL, then it
** can be used as the filename input parameter to these routines:
** <ul>
** <li> [sqlite3_uri_parameter()]
** <li> [sqlite3_uri_boolean()]
** <li> [sqlite3_uri_int64()]
** <li> [sqlite3_filename_database()]
** <li> [sqlite3_filename_journal()]
** <li> [sqlite3_filename_wal()]
** </ul>
*/
SQLITE_API sqlite3_filename sqlite3_db_filename(sqlite3 *db, const char *zDbName);

/*
** CAPI3REF: Determine if a database is read-only
** METHOD: sqlite3
**
** ^The sqlite3_db_readonly(D,N) interface returns 1 if the database N
** of connection D is read-only, 0 if it is read/write, or -1 if N is not
** the name of a database on connection D.
*/
SQLITE_API int sqlite3_db_readonly(sqlite3 *db, const char *zDbName);

/*
** CAPI3REF: Determine the transaction state of a database
** METHOD: sqlite3
**
** ^The sqlite3_txn_state(D,S) interface returns the current
** [transaction state] of schema S in database connection D.  ^If S is NULL,
** then the highest transaction state of any schema on database connection D
** is returned.  Transaction states are (in order of lowest to highest):
** <ol>
** <li value="0"> SQLITE_TXN_NONE
** <li value="1"> SQLITE_TXN_READ
** <li value="2"> SQLITE_TXN_WRITE
** </ol>
** ^If the S argument to sqlite3_txn_state(D,S) is not the name of
** a valid schema, then -1 is returned.
*/
SQLITE_API int sqlite3_txn_state(sqlite3*,const char *zSchema);

/*
** CAPI3REF: Allowed return values from sqlite3_txn_state()
** KEYWORDS: {transaction state}
**
** These constants define the current transaction state of a database file.
** ^The [sqlite3_txn_state(D,S)] interface returns one of these
** constants in order to describe the transaction state of schema S
** in [database connection] D.
**
** <dl>
** [[SQLITE_TXN_NONE]] <dt>SQLITE_TXN_NONE</dt>
** <dd>The SQLITE_TXN_NONE state means that no transaction is currently
** pending.</dd>
**
** [[SQLITE_TXN_READ]] <dt>SQLITE_TXN_READ</dt>
** <dd>The SQLITE_TXN_READ state means that the database is currently
** in a read transaction.  Content has been read from the database file
** but nothing in the database file has changed.  The transaction state
** will advanced to SQLITE_TXN_WRITE if any changes occur and there are
** no other conflicting concurrent write transactions.  The transaction
** state will revert to SQLITE_TXN_NONE following a [ROLLBACK] or
** [COMMIT].</dd>
**
** [[SQLITE_TXN_WRITE]] <dt>SQLITE_TXN_WRITE</dt>
** <dd>The SQLITE_TXN_WRITE state means that the database is currently
** in a write transaction.  Content has been written to the database file
** but has not yet committed.  The transaction state will change to
** to SQLITE_TXN_NONE at the next [ROLLBACK] or [COMMIT].</dd>
*/
#define SQLITE_TXN_NONE  0
#define SQLITE_TXN_READ  1
#define SQLITE_TXN_WRITE 2

/*
** CAPI3REF: Find the next prepared statement
** METHOD: sqlite3
**
** ^This interface returns a pointer to the next [prepared statement] after
** pStmt associated with the [database connection] pDb.  ^If pStmt is NULL
** then this interface returns a pointer to the first prepared statement
** associated with the database connection pDb.  ^If no prepared statement
** satisfies the conditions of this routine, it returns NULL.
**
** The [database connection] pointer D in a call to
** [sqlite3_next_stmt(D,S)] must refer to an open database
** connection and in particular must not be a NULL pointer.
*/
SQLITE_API sqlite3_stmt *sqlite3_next_stmt(sqlite3 *pDb, sqlite3_stmt *pStmt);

/*
** CAPI3REF: Commit And Rollback Notification Callbacks
** METHOD: sqlite3
**
** ^The sqlite3_commit_hook() interface registers a callback
** function to be invoked whenever a transaction is [COMMIT | committed].
** ^Any callback set by a previous call to sqlite3_commit_hook()
** for the same database connection is overridden.
** ^The sqlite3_rollback_hook() interface registers a callback
** function to be invoked whenever a transaction is [ROLLBACK | rolled back].
** ^Any callback set by a previous call to sqlite3_rollback_hook()
** for the same database connection is overridden.
** ^The pArg argument is passed through to the callback.
** ^If the callback on a commit hook function returns non-zero,
** then the commit is converted into a rollback.
