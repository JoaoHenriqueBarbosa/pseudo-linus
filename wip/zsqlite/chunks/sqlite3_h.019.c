**
** ^The sqlite3_commit_hook(D,C,P) and sqlite3_rollback_hook(D,C,P) functions
** return the P argument from the previous call of the same function
** on the same [database connection] D, or NULL for
** the first call for each function on D.
**
** The commit and rollback hook callbacks are not reentrant.
** The callback implementation must not do anything that will modify
** the database connection that invoked the callback.  Any actions
** to modify the database connection must be deferred until after the
** completion of the [sqlite3_step()] call that triggered the commit
** or rollback hook in the first place.
** Note that running any other SQL statements, including SELECT statements,
** or merely calling [sqlite3_prepare_v2()] and [sqlite3_step()] will modify
** the database connections for the meaning of "modify" in this paragraph.
**
** ^Registering a NULL function disables the callback.
**
** ^When the commit hook callback routine returns zero, the [COMMIT]
** operation is allowed to continue normally.  ^If the commit hook
** returns non-zero, then the [COMMIT] is converted into a [ROLLBACK].
** ^The rollback hook is invoked on a rollback that results from a commit
** hook returning non-zero, just as it would be with any other rollback.
**
** ^For the purposes of this API, a transaction is said to have been
** rolled back if an explicit "ROLLBACK" statement is executed, or
** an error or constraint causes an implicit rollback to occur.
** ^The rollback callback is not invoked if a transaction is
** automatically rolled back because the database connection is closed.
**
** See also the [sqlite3_update_hook()] interface.
*/
SQLITE_API void *sqlite3_commit_hook(sqlite3*, int(*)(void*), void*);
SQLITE_API void *sqlite3_rollback_hook(sqlite3*, void(*)(void *), void*);

/*
** CAPI3REF: Autovacuum Compaction Amount Callback
** METHOD: sqlite3
**
** ^The sqlite3_autovacuum_pages(D,C,P,X) interface registers a callback
** function C that is invoked prior to each autovacuum of the database
** file.  ^The callback is passed a copy of the generic data pointer (P),
** the schema-name of the attached database that is being autovacuumed,
** the size of the database file in pages, the number of free pages,
** and the number of bytes per page, respectively.  The callback should
** return the number of free pages that should be removed by the
** autovacuum.  ^If the callback returns zero, then no autovacuum happens.
** ^If the value returned is greater than or equal to the number of
** free pages, then a complete autovacuum happens.
**
** <p>^If there are multiple ATTACH-ed database files that are being
** modified as part of a transaction commit, then the autovacuum pages
** callback is invoked separately for each file.
**
** <p><b>The callback is not reentrant.</b> The callback function should
** not attempt to invoke any other SQLite interface.  If it does, bad
** things may happen, including segmentation faults and corrupt database
** files.  The callback function should be a simple function that
** does some arithmetic on its input parameters and returns a result.
**
** ^The X parameter to sqlite3_autovacuum_pages(D,C,P,X) is an optional
** destructor for the P parameter.  ^If X is not NULL, then X(P) is
** invoked whenever the database connection closes or when the callback
** is overwritten by another invocation of sqlite3_autovacuum_pages().
**
** <p>^There is only one autovacuum pages callback per database connection.
** ^Each call to the sqlite3_autovacuum_pages() interface overrides all
** previous invocations for that database connection.  ^If the callback
** argument (C) to sqlite3_autovacuum_pages(D,C,P,X) is a NULL pointer,
** then the autovacuum steps callback is canceled.  The return value
** from sqlite3_autovacuum_pages() is normally SQLITE_OK, but might
** be some other error code if something goes wrong.  The current
** implementation will only return SQLITE_OK or SQLITE_MISUSE, but other
** return codes might be added in future releases.
**
** <p>If no autovacuum pages callback is specified (the usual case) or
** a NULL pointer is provided for the callback,
** then the default behavior is to vacuum all free pages.  So, in other
** words, the default behavior is the same as if the callback function
** were something like this:
**
** <blockquote><pre>
** &nbsp;   unsigned int demonstration_autovac_pages_callback(
** &nbsp;     void *pClientData,
** &nbsp;     const char *zSchema,
** &nbsp;     unsigned int nDbPage,
** &nbsp;     unsigned int nFreePage,
** &nbsp;     unsigned int nBytePerPage
** &nbsp;   ){
** &nbsp;     return nFreePage;
** &nbsp;   }
** </pre></blockquote>
*/
SQLITE_API int sqlite3_autovacuum_pages(
  sqlite3 *db,
  unsigned int(*)(void*,const char*,unsigned int,unsigned int,unsigned int),
  void*,
  void(*)(void*)
);


/*
** CAPI3REF: Data Change Notification Callbacks
** METHOD: sqlite3
**
** ^The sqlite3_update_hook() interface registers a callback function
** with the [database connection] identified by the first argument
** to be invoked whenever a row is updated, inserted or deleted in
** a [rowid table].
** ^Any callback set by a previous call to this function
** for the same database connection is overridden.
**
** ^The second argument is a pointer to the function to invoke when a
** row is updated, inserted or deleted in a rowid table.
** ^The first argument to the callback is a copy of the third argument
** to sqlite3_update_hook().
** ^The second callback argument is one of [SQLITE_INSERT], [SQLITE_DELETE],
** or [SQLITE_UPDATE], depending on the operation that caused the callback
** to be invoked.
** ^The third and fourth arguments to the callback contain pointers to the
** database and table name containing the affected row.
** ^The final callback parameter is the [rowid] of the row.
** ^In the case of an update, this is the [rowid] after the update takes place.
**
** ^(The update hook is not invoked when internal system tables are
** modified (i.e. sqlite_sequence).)^
** ^The update hook is not invoked when [WITHOUT ROWID] tables are modified.
**
** ^In the current implementation, the update hook
** is not invoked when conflicting rows are deleted because of an
** [ON CONFLICT | ON CONFLICT REPLACE] clause.  ^Nor is the update hook
** invoked when rows are deleted using the [truncate optimization].
** The exceptions defined in this paragraph might change in a future
** release of SQLite.
**
** Whether the update hook is invoked before or after the
** corresponding change is currently unspecified and may differ
** depending on the type of change. Do not rely on the order of the
** hook call with regards to the final result of the operation which
** triggers the hook.
**
** The update hook implementation must not do anything that will modify
** the database connection that invoked the update hook.  Any actions
** to modify the database connection must be deferred until after the
** completion of the [sqlite3_step()] call that triggered the update hook.
** Note that [sqlite3_prepare_v2()] and [sqlite3_step()] both modify their
** database connections for the meaning of "modify" in this paragraph.
**
** ^The sqlite3_update_hook(D,C,P) function
** returns the P argument from the previous call
** on the same [database connection] D, or NULL for
** the first call on D.
**
** See also the [sqlite3_commit_hook()], [sqlite3_rollback_hook()],
** and [sqlite3_preupdate_hook()] interfaces.
*/
SQLITE_API void *sqlite3_update_hook(
  sqlite3*,
  void(*)(void *,int ,char const *,char const *,sqlite3_int64),
  void*
);

/*
** CAPI3REF: Enable Or Disable Shared Pager Cache
**
** ^(This routine enables or disables the sharing of the database cache
** and schema data structures between [database connection | connections]
** to the same database. Sharing is enabled if the argument is true
** and disabled if the argument is false.)^
**
** This interface is omitted if SQLite is compiled with
** [-DSQLITE_OMIT_SHARED_CACHE].  The [-DSQLITE_OMIT_SHARED_CACHE]
** compile-time option is recommended because the
** [use of shared cache mode is discouraged].
**
** ^Cache sharing is enabled and disabled for an entire process.
** This is a change as of SQLite [version 3.5.0] ([dateof:3.5.0]).
** In prior versions of SQLite,
** sharing was enabled or disabled for each thread separately.
**
** ^(The cache sharing mode set by this interface effects all subsequent
** calls to [sqlite3_open()], [sqlite3_open_v2()], and [sqlite3_open16()].
** Existing database connections continue to use the sharing mode
** that was in effect at the time they were opened.)^
**
** ^(This routine returns [SQLITE_OK] if shared cache was enabled or disabled
** successfully.  An [error code] is returned otherwise.)^
**
** ^Shared cache is disabled by default. It is recommended that it stay
** that way.  In other words, do not use this routine.  This interface
** continues to be provided for historical compatibility, but its use is
** discouraged.  Any use of shared cache is discouraged.  If shared cache
** must be used, it is recommended that shared cache only be enabled for
** individual database connections using the [sqlite3_open_v2()] interface
** with the [SQLITE_OPEN_SHAREDCACHE] flag.
**
** Note: This method is disabled on MacOS X 10.7 and iOS version 5.0
** and will always return SQLITE_MISUSE. On those systems,
** shared cache mode should be enabled per-database connection via
** [sqlite3_open_v2()] with [SQLITE_OPEN_SHAREDCACHE].
**
** This interface is threadsafe on processors where writing a
** 32-bit integer is atomic.
**
** See Also:  [SQLite Shared-Cache Mode]
*/
SQLITE_API int sqlite3_enable_shared_cache(int);

/*
** CAPI3REF: Attempt To Free Heap Memory
**
** ^The sqlite3_release_memory() interface attempts to free N bytes
** of heap memory by deallocating non-essential memory allocations
** held by the database library.   Memory used to cache database
** pages to improve performance is an example of non-essential memory.
** ^sqlite3_release_memory() returns the number of bytes actually freed,
** which might be more or less than the amount requested.
** ^The sqlite3_release_memory() routine is a no-op returning zero
** if SQLite is not compiled with [SQLITE_ENABLE_MEMORY_MANAGEMENT].
**
** See also: [sqlite3_db_release_memory()]
*/
SQLITE_API int sqlite3_release_memory(int);

/*
** CAPI3REF: Free Memory Used By A Database Connection
** METHOD: sqlite3
**
** ^The sqlite3_db_release_memory(D) interface attempts to free as much heap
** memory as possible from database connection D. Unlike the
** [sqlite3_release_memory()] interface, this interface is in effect even
** when the [SQLITE_ENABLE_MEMORY_MANAGEMENT] compile-time option is
** omitted.
**
** See also: [sqlite3_release_memory()]
*/
SQLITE_API int sqlite3_db_release_memory(sqlite3*);

/*
** CAPI3REF: Impose A Limit On Heap Size
**
** These interfaces impose limits on the amount of heap memory that will be
** by all database connections within a single process.
**
** ^The sqlite3_soft_heap_limit64() interface sets and/or queries the
** soft limit on the amount of heap memory that may be allocated by SQLite.
** ^SQLite strives to keep heap memory utilization below the soft heap
** limit by reducing the number of pages held in the page cache
** as heap memory usages approaches the limit.
** ^The soft heap limit is "soft" because even though SQLite strives to stay
** below the limit, it will exceed the limit rather than generate
** an [SQLITE_NOMEM] error.  In other words, the soft heap limit
** is advisory only.
**
** ^The sqlite3_hard_heap_limit64(N) interface sets a hard upper bound of
** N bytes on the amount of memory that will be allocated.  ^The
** sqlite3_hard_heap_limit64(N) interface is similar to
** sqlite3_soft_heap_limit64(N) except that memory allocations will fail
** when the hard heap limit is reached.
**
** ^The return value from both sqlite3_soft_heap_limit64() and
** sqlite3_hard_heap_limit64() is the size of
** the heap limit prior to the call, or negative in the case of an
** error.  ^If the argument N is negative
** then no change is made to the heap limit.  Hence, the current
** size of heap limits can be determined by invoking
** sqlite3_soft_heap_limit64(-1) or sqlite3_hard_heap_limit(-1).
**
** ^Setting the heap limits to zero disables the heap limiter mechanism.
**
** ^The soft heap limit may not be greater than the hard heap limit.
** ^If the hard heap limit is enabled and if sqlite3_soft_heap_limit(N)
** is invoked with a value of N that is greater than the hard heap limit,
** the soft heap limit is set to the value of the hard heap limit.
** ^The soft heap limit is automatically enabled whenever the hard heap
** limit is enabled. ^When sqlite3_hard_heap_limit64(N) is invoked and
** the soft heap limit is outside the range of 1..N, then the soft heap
** limit is set to N.  ^Invoking sqlite3_soft_heap_limit64(0) when the
** hard heap limit is enabled makes the soft heap limit equal to the
** hard heap limit.
**
** The memory allocation limits can also be adjusted using
** [PRAGMA soft_heap_limit] and [PRAGMA hard_heap_limit].
**
** ^(The heap limits are not enforced in the current implementation
** if one or more of following conditions are true:
**
** <ul>
** <li> The limit value is set to zero.
** <li> Memory accounting is disabled using a combination of the
**      [sqlite3_config]([SQLITE_CONFIG_MEMSTATUS],...) start-time option and
**      the [SQLITE_DEFAULT_MEMSTATUS] compile-time option.
** <li> An alternative page cache implementation is specified using
**      [sqlite3_config]([SQLITE_CONFIG_PCACHE2],...).
** <li> The page cache allocates from its own memory pool supplied
**      by [sqlite3_config]([SQLITE_CONFIG_PAGECACHE],...) rather than
**      from the heap.
** </ul>)^
**
** The circumstances under which SQLite will enforce the heap limits may
** changes in future releases of SQLite.
*/
SQLITE_API sqlite3_int64 sqlite3_soft_heap_limit64(sqlite3_int64 N);
SQLITE_API sqlite3_int64 sqlite3_hard_heap_limit64(sqlite3_int64 N);

/*
** CAPI3REF: Deprecated Soft Heap Limit Interface
** DEPRECATED
**
** This is a deprecated version of the [sqlite3_soft_heap_limit64()]
** interface.  This routine is provided for historical compatibility
** only.  All new applications should use the
** [sqlite3_soft_heap_limit64()] interface rather than this one.
*/
SQLITE_API SQLITE_DEPRECATED void sqlite3_soft_heap_limit(int N);


/*
** CAPI3REF: Extract Metadata About A Column Of A Table
** METHOD: sqlite3
**
** ^(The sqlite3_table_column_metadata(X,D,T,C,....) routine returns
** information about column C of table T in database D
** on [database connection] X.)^  ^The sqlite3_table_column_metadata()
** interface returns SQLITE_OK and fills in the non-NULL pointers in
** the final five arguments with appropriate values if the specified
** column exists.  ^The sqlite3_table_column_metadata() interface returns
** SQLITE_ERROR if the specified column does not exist.
** ^If the column-name parameter to sqlite3_table_column_metadata() is a
** NULL pointer, then this routine simply checks for the existence of the
** table and returns SQLITE_OK if the table exists and SQLITE_ERROR if it
** does not.  If the table name parameter T in a call to
** sqlite3_table_column_metadata(X,D,T,C,...) is NULL then the result is
** undefined behavior.
**
** ^The column is identified by the second, third and fourth parameters to
** this function. ^(The second parameter is either the name of the database
** (i.e. "main", "temp", or an attached database) containing the specified
** table or NULL.)^ ^If it is NULL, then all attached databases are searched
** for the table using the same algorithm used by the database engine to
** resolve unqualified table references.
**
** ^The third and fourth parameters to this function are the table and column
** name of the desired column, respectively.
**
** ^Metadata is returned by writing to the memory locations passed as the 5th
** and subsequent parameters to this function. ^Any of these arguments may be
** NULL, in which case the corresponding element of metadata is omitted.
**
** ^(<blockquote>
** <table border="1">
** <tr><th> Parameter <th> Output<br>Type <th>  Description
**
** <tr><td> 5th <td> const char* <td> Data type
** <tr><td> 6th <td> const char* <td> Name of default collation sequence
** <tr><td> 7th <td> int         <td> True if column has a NOT NULL constraint
** <tr><td> 8th <td> int         <td> True if column is part of the PRIMARY KEY
** <tr><td> 9th <td> int         <td> True if column is [AUTOINCREMENT]
** </table>
** </blockquote>)^
**
** ^The memory pointed to by the character pointers returned for the
** declaration type and collation sequence is valid until the next
** call to any SQLite API function.
**
** ^If the specified table is actually a view, an [error code] is returned.
**
** ^If the specified column is "rowid", "oid" or "_rowid_" and the table
** is not a [WITHOUT ROWID] table and an
** [INTEGER PRIMARY KEY] column has been explicitly declared, then the output
** parameters are set for the explicitly declared column. ^(If there is no
** [INTEGER PRIMARY KEY] column, then the outputs
** for the [rowid] are set as follows:
**
** <pre>
**     data type: "INTEGER"
**     collation sequence: "BINARY"
**     not null: 0
**     primary key: 1
**     auto increment: 0
** </pre>)^
**
** ^This function causes all database schemas to be read from disk and
** parsed, if that has not already been done, and returns an error if
** any errors are encountered while loading the schema.
*/
SQLITE_API int sqlite3_table_column_metadata(
  sqlite3 *db,                /* Connection handle */
  const char *zDbName,        /* Database name or NULL */
  const char *zTableName,     /* Table name */
  const char *zColumnName,    /* Column name */
  char const **pzDataType,    /* OUTPUT: Declared data type */
  char const **pzCollSeq,     /* OUTPUT: Collation sequence name */
  int *pNotNull,              /* OUTPUT: True if NOT NULL constraint exists */
  int *pPrimaryKey,           /* OUTPUT: True if column part of PK */
  int *pAutoinc               /* OUTPUT: True if column is auto-increment */
);

/*
** CAPI3REF: Load An Extension
** METHOD: sqlite3
**
** ^This interface loads an SQLite extension library from the named file.
**
** ^The sqlite3_load_extension() interface attempts to load an
** [SQLite extension] library contained in the file zFile.  If
** the file cannot be loaded directly, attempts are made to load
** with various operating-system specific extensions added.
** So for example, if "samplelib" cannot be loaded, then names like
** "samplelib.so" or "samplelib.dylib" or "samplelib.dll" might
** be tried also.
**
** ^The entry point is zProc.
** ^(zProc may be 0, in which case SQLite will try to come up with an
** entry point name on its own.  It first tries "sqlite3_extension_init".
** If that does not work, it constructs a name "sqlite3_X_init" where the
** X is consists of the lower-case equivalent of all ASCII alphabetic
** characters in the filename from the last "/" to the first following
** "." and omitting any initial "lib".)^
** ^The sqlite3_load_extension() interface returns
** [SQLITE_OK] on success and [SQLITE_ERROR] if something goes wrong.
** ^If an error occurs and pzErrMsg is not 0, then the
** [sqlite3_load_extension()] interface shall attempt to
** fill *pzErrMsg with error message text stored in memory
** obtained from [sqlite3_malloc()]. The calling function
** should free this memory by calling [sqlite3_free()].
**
** ^Extension loading must be enabled using
** [sqlite3_enable_load_extension()] or
** [sqlite3_db_config](db,[SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION],1,NULL)
** prior to calling this API,
** otherwise an error will be returned.
**
** <b>Security warning:</b> It is recommended that the
** [SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION] method be used to enable only this
** interface.  The use of the [sqlite3_enable_load_extension()] interface
** should be avoided.  This will keep the SQL function [load_extension()]
** disabled and prevent SQL injections from giving attackers
** access to extension loading capabilities.
**
** See also the [load_extension() SQL function].
*/
SQLITE_API int sqlite3_load_extension(
  sqlite3 *db,          /* Load the extension into this database connection */
  const char *zFile,    /* Name of the shared library containing extension */
  const char *zProc,    /* Entry point.  Derived from zFile if 0 */
  char **pzErrMsg       /* Put error message here if not 0 */
);

/*
** CAPI3REF: Enable Or Disable Extension Loading
** METHOD: sqlite3
**
** ^So as not to open security holes in older applications that are
** unprepared to deal with [extension loading], and as a means of disabling
** [extension loading] while evaluating user-entered SQL, the following API
** is provided to turn the [sqlite3_load_extension()] mechanism on and off.
**
** ^Extension loading is off by default.
** ^Call the sqlite3_enable_load_extension() routine with onoff==1
** to turn extension loading on and call it with onoff==0 to turn
** it back off again.
**
** ^This interface enables or disables both the C-API
** [sqlite3_load_extension()] and the SQL function [load_extension()].
** ^(Use [sqlite3_db_config](db,[SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION],..)
** to enable or disable only the C-API.)^
**
** <b>Security warning:</b> It is recommended that extension loading
** be enabled using the [SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION] method
** rather than this interface, so the [load_extension()] SQL function
** remains disabled. This will prevent SQL injections from giving attackers
** access to extension loading capabilities.
*/
SQLITE_API int sqlite3_enable_load_extension(sqlite3 *db, int onoff);

/*
** CAPI3REF: Automatically Load Statically Linked Extensions
**
** ^This interface causes the xEntryPoint() function to be invoked for
** each new [database connection] that is created.  The idea here is that
** xEntryPoint() is the entry point for a statically linked [SQLite extension]
** that is to be automatically loaded into all new database connections.
**
** ^(Even though the function prototype shows that xEntryPoint() takes
** no arguments and returns void, SQLite invokes xEntryPoint() with three
** arguments and expects an integer result as if the signature of the
** entry point where as follows:
**
** <blockquote><pre>
** &nbsp;  int xEntryPoint(
** &nbsp;    sqlite3 *db,
** &nbsp;    const char **pzErrMsg,
** &nbsp;    const struct sqlite3_api_routines *pThunk
** &nbsp;  );
** </pre></blockquote>)^
**
** If the xEntryPoint routine encounters an error, it should make *pzErrMsg
** point to an appropriate error message (obtained from [sqlite3_mprintf()])
** and return an appropriate [error code].  ^SQLite ensures that *pzErrMsg
** is NULL before calling the xEntryPoint().  ^SQLite will invoke
** [sqlite3_free()] on *pzErrMsg after xEntryPoint() returns.  ^If any
** xEntryPoint() returns an error, the [sqlite3_open()], [sqlite3_open16()],
** or [sqlite3_open_v2()] call that provoked the xEntryPoint() will fail.
**
** ^Calling sqlite3_auto_extension(X) with an entry point X that is already
** on the list of automatic extensions is a harmless no-op. ^No entry point
** will be called more than once for each database connection that is opened.
**
** See also: [sqlite3_reset_auto_extension()]
** and [sqlite3_cancel_auto_extension()]
*/
SQLITE_API int sqlite3_auto_extension(void(*xEntryPoint)(void));

/*
** CAPI3REF: Cancel Automatic Extension Loading
**
** ^The [sqlite3_cancel_auto_extension(X)] interface unregisters the
** initialization routine X that was registered using a prior call to
** [sqlite3_auto_extension(X)].  ^The [sqlite3_cancel_auto_extension(X)]
** routine returns 1 if initialization routine X was successfully
** unregistered and it returns 0 if X was not on the list of initialization
** routines.
*/
SQLITE_API int sqlite3_cancel_auto_extension(void(*xEntryPoint)(void));

/*
** CAPI3REF: Reset Automatic Extension Loading
**
** ^This interface disables all automatic extensions previously
** registered using [sqlite3_auto_extension()].
*/
SQLITE_API void sqlite3_reset_auto_extension(void);

/*
** Structures used by the virtual table interface
*/
typedef struct sqlite3_vtab sqlite3_vtab;
typedef struct sqlite3_index_info sqlite3_index_info;
typedef struct sqlite3_vtab_cursor sqlite3_vtab_cursor;
typedef struct sqlite3_module sqlite3_module;

/*
** CAPI3REF: Virtual Table Object
** KEYWORDS: sqlite3_module {virtual table module}
**
** This structure, sometimes called a "virtual table module",
** defines the implementation of a [virtual table].
** This structure consists mostly of methods for the module.
**
** ^A virtual table module is created by filling in a persistent
** instance of this structure and passing a pointer to that instance
** to [sqlite3_create_module()] or [sqlite3_create_module_v2()].
** ^The registration remains valid until it is replaced by a different
** module or until the [database connection] closes.  The content
** of this structure must not change while it is registered with
** any database connection.
*/
struct sqlite3_module {
  int iVersion;
  int (*xCreate)(sqlite3*, void *pAux,
               int argc, const char *const*argv,
               sqlite3_vtab **ppVTab, char**);
  int (*xConnect)(sqlite3*, void *pAux,
               int argc, const char *const*argv,
               sqlite3_vtab **ppVTab, char**);
  int (*xBestIndex)(sqlite3_vtab *pVTab, sqlite3_index_info*);
  int (*xDisconnect)(sqlite3_vtab *pVTab);
  int (*xDestroy)(sqlite3_vtab *pVTab);
  int (*xOpen)(sqlite3_vtab *pVTab, sqlite3_vtab_cursor **ppCursor);
  int (*xClose)(sqlite3_vtab_cursor*);
  int (*xFilter)(sqlite3_vtab_cursor*, int idxNum, const char *idxStr,
                int argc, sqlite3_value **argv);
  int (*xNext)(sqlite3_vtab_cursor*);
  int (*xEof)(sqlite3_vtab_cursor*);
  int (*xColumn)(sqlite3_vtab_cursor*, sqlite3_context*, int);
  int (*xRowid)(sqlite3_vtab_cursor*, sqlite3_int64 *pRowid);
  int (*xUpdate)(sqlite3_vtab *, int, sqlite3_value **, sqlite3_int64 *);
  int (*xBegin)(sqlite3_vtab *pVTab);
  int (*xSync)(sqlite3_vtab *pVTab);
  int (*xCommit)(sqlite3_vtab *pVTab);
  int (*xRollback)(sqlite3_vtab *pVTab);
  int (*xFindFunction)(sqlite3_vtab *pVtab, int nArg, const char *zName,
                       void (**pxFunc)(sqlite3_context*,int,sqlite3_value**),
                       void **ppArg);
  int (*xRename)(sqlite3_vtab *pVtab, const char *zNew);
  /* The methods above are in version 1 of the sqlite_module object. Those
  ** below are for version 2 and greater. */
  int (*xSavepoint)(sqlite3_vtab *pVTab, int);
  int (*xRelease)(sqlite3_vtab *pVTab, int);
  int (*xRollbackTo)(sqlite3_vtab *pVTab, int);
  /* The methods above are in versions 1 and 2 of the sqlite_module object.
  ** Those below are for version 3 and greater. */
  int (*xShadowName)(const char*);
  /* The methods above are in versions 1 through 3 of the sqlite_module object.
  ** Those below are for version 4 and greater. */
  int (*xIntegrity)(sqlite3_vtab *pVTab, const char *zSchema,
                    const char *zTabName, int mFlags, char **pzErr);
};
