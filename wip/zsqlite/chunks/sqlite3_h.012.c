  sqlite3_stmt **ppStmt,  /* OUT: Statement handle */
  const char **pzTail     /* OUT: Pointer to unused portion of zSql */
);
SQLITE_API int sqlite3_prepare_v3(
  sqlite3 *db,            /* Database handle */
  const char *zSql,       /* SQL statement, UTF-8 encoded */
  int nByte,              /* Maximum length of zSql in bytes. */
  unsigned int prepFlags, /* Zero or more SQLITE_PREPARE_ flags */
  sqlite3_stmt **ppStmt,  /* OUT: Statement handle */
  const char **pzTail     /* OUT: Pointer to unused portion of zSql */
);
SQLITE_API int sqlite3_prepare16(
  sqlite3 *db,            /* Database handle */
  const void *zSql,       /* SQL statement, UTF-16 encoded */
  int nByte,              /* Maximum length of zSql in bytes. */
  sqlite3_stmt **ppStmt,  /* OUT: Statement handle */
  const void **pzTail     /* OUT: Pointer to unused portion of zSql */
);
SQLITE_API int sqlite3_prepare16_v2(
  sqlite3 *db,            /* Database handle */
  const void *zSql,       /* SQL statement, UTF-16 encoded */
  int nByte,              /* Maximum length of zSql in bytes. */
  sqlite3_stmt **ppStmt,  /* OUT: Statement handle */
  const void **pzTail     /* OUT: Pointer to unused portion of zSql */
);
SQLITE_API int sqlite3_prepare16_v3(
  sqlite3 *db,            /* Database handle */
  const void *zSql,       /* SQL statement, UTF-16 encoded */
  int nByte,              /* Maximum length of zSql in bytes. */
  unsigned int prepFlags, /* Zero or more SQLITE_PREPARE_ flags */
  sqlite3_stmt **ppStmt,  /* OUT: Statement handle */
  const void **pzTail     /* OUT: Pointer to unused portion of zSql */
);

/*
** CAPI3REF: Retrieving Statement SQL
** METHOD: sqlite3_stmt
**
** ^The sqlite3_sql(P) interface returns a pointer to a copy of the UTF-8
** SQL text used to create [prepared statement] P if P was
** created by [sqlite3_prepare_v2()], [sqlite3_prepare_v3()],
** [sqlite3_prepare16_v2()], or [sqlite3_prepare16_v3()].
** ^The sqlite3_expanded_sql(P) interface returns a pointer to a UTF-8
** string containing the SQL text of prepared statement P with
** [bound parameters] expanded.
** ^The sqlite3_normalized_sql(P) interface returns a pointer to a UTF-8
** string containing the normalized SQL text of prepared statement P.  The
** semantics used to normalize a SQL statement are unspecified and subject
** to change.  At a minimum, literal values will be replaced with suitable
** placeholders.
**
** ^(For example, if a prepared statement is created using the SQL
** text "SELECT $abc,:xyz" and if parameter $abc is bound to integer 2345
** and parameter :xyz is unbound, then sqlite3_sql() will return
** the original string, "SELECT $abc,:xyz" but sqlite3_expanded_sql()
** will return "SELECT 2345,NULL".)^
**
** ^The sqlite3_expanded_sql() interface returns NULL if insufficient memory
** is available to hold the result, or if the result would exceed the
** the maximum string length determined by the [SQLITE_LIMIT_LENGTH].
**
** ^The [SQLITE_TRACE_SIZE_LIMIT] compile-time option limits the size of
** bound parameter expansions.  ^The [SQLITE_OMIT_TRACE] compile-time
** option causes sqlite3_expanded_sql() to always return NULL.
**
** ^The strings returned by sqlite3_sql(P) and sqlite3_normalized_sql(P)
** are managed by SQLite and are automatically freed when the prepared
** statement is finalized.
** ^The string returned by sqlite3_expanded_sql(P), on the other hand,
** is obtained from [sqlite3_malloc()] and must be freed by the application
** by passing it to [sqlite3_free()].
**
** ^The sqlite3_normalized_sql() interface is only available if
** the [SQLITE_ENABLE_NORMALIZE] compile-time option is defined.
*/
SQLITE_API const char *sqlite3_sql(sqlite3_stmt *pStmt);
SQLITE_API char *sqlite3_expanded_sql(sqlite3_stmt *pStmt);
#ifdef SQLITE_ENABLE_NORMALIZE
SQLITE_API const char *sqlite3_normalized_sql(sqlite3_stmt *pStmt);
#endif

/*
** CAPI3REF: Determine If An SQL Statement Writes The Database
** METHOD: sqlite3_stmt
**
** ^The sqlite3_stmt_readonly(X) interface returns true (non-zero) if
** and only if the [prepared statement] X makes no direct changes to
** the content of the database file.
**
** Note that [application-defined SQL functions] or
** [virtual tables] might change the database indirectly as a side effect.
** ^(For example, if an application defines a function "eval()" that
** calls [sqlite3_exec()], then the following SQL statement would
** change the database file through side-effects:
**
** <blockquote><pre>
**    SELECT eval('DELETE FROM t1') FROM t2;
** </pre></blockquote>
**
** But because the [SELECT] statement does not change the database file
** directly, sqlite3_stmt_readonly() would still return true.)^
**
** ^Transaction control statements such as [BEGIN], [COMMIT], [ROLLBACK],
** [SAVEPOINT], and [RELEASE] cause sqlite3_stmt_readonly() to return true,
** since the statements themselves do not actually modify the database but
** rather they control the timing of when other statements modify the
** database.  ^The [ATTACH] and [DETACH] statements also cause
** sqlite3_stmt_readonly() to return true since, while those statements
** change the configuration of a database connection, they do not make
** changes to the content of the database files on disk.
** ^The sqlite3_stmt_readonly() interface returns true for [BEGIN] since
** [BEGIN] merely sets internal flags, but the [BEGIN|BEGIN IMMEDIATE] and
** [BEGIN|BEGIN EXCLUSIVE] commands do touch the database and so
** sqlite3_stmt_readonly() returns false for those commands.
**
** ^This routine returns false if there is any possibility that the
** statement might change the database file.  ^A false return does
** not guarantee that the statement will change the database file.
** ^For example, an UPDATE statement might have a WHERE clause that
** makes it a no-op, but the sqlite3_stmt_readonly() result would still
** be false.  ^Similarly, a CREATE TABLE IF NOT EXISTS statement is a
** read-only no-op if the table already exists, but
** sqlite3_stmt_readonly() still returns false for such a statement.
**
** ^If prepared statement X is an [EXPLAIN] or [EXPLAIN QUERY PLAN]
** statement, then sqlite3_stmt_readonly(X) returns the same value as
** if the EXPLAIN or EXPLAIN QUERY PLAN prefix were omitted.
*/
SQLITE_API int sqlite3_stmt_readonly(sqlite3_stmt *pStmt);

/*
** CAPI3REF: Query The EXPLAIN Setting For A Prepared Statement
** METHOD: sqlite3_stmt
**
** ^The sqlite3_stmt_isexplain(S) interface returns 1 if the
** prepared statement S is an EXPLAIN statement, or 2 if the
** statement S is an EXPLAIN QUERY PLAN.
** ^The sqlite3_stmt_isexplain(S) interface returns 0 if S is
** an ordinary statement or a NULL pointer.
*/
SQLITE_API int sqlite3_stmt_isexplain(sqlite3_stmt *pStmt);

/*
** CAPI3REF: Change The EXPLAIN Setting For A Prepared Statement
** METHOD: sqlite3_stmt
**
** The sqlite3_stmt_explain(S,E) interface changes the EXPLAIN
** setting for [prepared statement] S.  If E is zero, then S becomes
** a normal prepared statement.  If E is 1, then S behaves as if
** its SQL text began with "[EXPLAIN]".  If E is 2, then S behaves as if
** its SQL text began with "[EXPLAIN QUERY PLAN]".
**
** Calling sqlite3_stmt_explain(S,E) might cause S to be reprepared.
** SQLite tries to avoid a reprepare, but a reprepare might be necessary
** on the first transition into EXPLAIN or EXPLAIN QUERY PLAN mode.
**
** Because of the potential need to reprepare, a call to
** sqlite3_stmt_explain(S,E) will fail with SQLITE_ERROR if S cannot be
** reprepared because it was created using [sqlite3_prepare()] instead of
** the newer [sqlite3_prepare_v2()] or [sqlite3_prepare_v3()] interfaces and
** hence has no saved SQL text with which to reprepare.
**
** Changing the explain setting for a prepared statement does not change
** the original SQL text for the statement.  Hence, if the SQL text originally
** began with EXPLAIN or EXPLAIN QUERY PLAN, but sqlite3_stmt_explain(S,0)
** is called to convert the statement into an ordinary statement, the EXPLAIN
** or EXPLAIN QUERY PLAN keywords will still appear in the sqlite3_sql(S)
** output, even though the statement now acts like a normal SQL statement.
**
** This routine returns SQLITE_OK if the explain mode is successfully
** changed, or an error code if the explain mode could not be changed.
** The explain mode cannot be changed while a statement is active.
** Hence, it is good practice to call [sqlite3_reset(S)]
** immediately prior to calling sqlite3_stmt_explain(S,E).
*/
SQLITE_API int sqlite3_stmt_explain(sqlite3_stmt *pStmt, int eMode);

/*
** CAPI3REF: Determine If A Prepared Statement Has Been Reset
** METHOD: sqlite3_stmt
**
** ^The sqlite3_stmt_busy(S) interface returns true (non-zero) if the
** [prepared statement] S has been stepped at least once using
** [sqlite3_step(S)] but has neither run to completion (returned
** [SQLITE_DONE] from [sqlite3_step(S)]) nor
** been reset using [sqlite3_reset(S)].  ^The sqlite3_stmt_busy(S)
** interface returns false if S is a NULL pointer.  If S is not a
** NULL pointer and is not a pointer to a valid [prepared statement]
** object, then the behavior is undefined and probably undesirable.
**
** This interface can be used in combination [sqlite3_next_stmt()]
** to locate all prepared statements associated with a database
** connection that are in need of being reset.  This can be used,
** for example, in diagnostic routines to search for prepared
** statements that are holding a transaction open.
*/
SQLITE_API int sqlite3_stmt_busy(sqlite3_stmt*);

/*
** CAPI3REF: Dynamically Typed Value Object
** KEYWORDS: {protected sqlite3_value} {unprotected sqlite3_value}
**
** SQLite uses the sqlite3_value object to represent all values
** that can be stored in a database table. SQLite uses dynamic typing
** for the values it stores.  ^Values stored in sqlite3_value objects
** can be integers, floating point values, strings, BLOBs, or NULL.
**
** An sqlite3_value object may be either "protected" or "unprotected".
** Some interfaces require a protected sqlite3_value.  Other interfaces
** will accept either a protected or an unprotected sqlite3_value.
** Every interface that accepts sqlite3_value arguments specifies
** whether or not it requires a protected sqlite3_value.  The
** [sqlite3_value_dup()] interface can be used to construct a new
** protected sqlite3_value from an unprotected sqlite3_value.
**
** The terms "protected" and "unprotected" refer to whether or not
** a mutex is held.  An internal mutex is held for a protected
** sqlite3_value object but no mutex is held for an unprotected
** sqlite3_value object.  If SQLite is compiled to be single-threaded
** (with [SQLITE_THREADSAFE=0] and with [sqlite3_threadsafe()] returning 0)
** or if SQLite is run in one of reduced mutex modes
** [SQLITE_CONFIG_SINGLETHREAD] or [SQLITE_CONFIG_MULTITHREAD]
** then there is no distinction between protected and unprotected
** sqlite3_value objects and they can be used interchangeably.  However,
** for maximum code portability it is recommended that applications
** still make the distinction between protected and unprotected
** sqlite3_value objects even when not strictly required.
**
** ^The sqlite3_value objects that are passed as parameters into the
** implementation of [application-defined SQL functions] are protected.
** ^The sqlite3_value objects returned by [sqlite3_vtab_rhs_value()]
** are protected.
** ^The sqlite3_value object returned by
** [sqlite3_column_value()] is unprotected.
** Unprotected sqlite3_value objects may only be used as arguments
** to [sqlite3_result_value()], [sqlite3_bind_value()], and
** [sqlite3_value_dup()].
** The [sqlite3_value_blob | sqlite3_value_type()] family of
** interfaces require protected sqlite3_value objects.
*/
typedef struct sqlite3_value sqlite3_value;

/*
** CAPI3REF: SQL Function Context Object
**
** The context in which an SQL function executes is stored in an
** sqlite3_context object.  ^A pointer to an sqlite3_context object
** is always first parameter to [application-defined SQL functions].
** The application-defined SQL function implementation will pass this
** pointer through into calls to [sqlite3_result_int | sqlite3_result()],
** [sqlite3_aggregate_context()], [sqlite3_user_data()],
** [sqlite3_context_db_handle()], [sqlite3_get_auxdata()],
** and/or [sqlite3_set_auxdata()].
*/
typedef struct sqlite3_context sqlite3_context;

/*
** CAPI3REF: Binding Values To Prepared Statements
** KEYWORDS: {host parameter} {host parameters} {host parameter name}
** KEYWORDS: {SQL parameter} {SQL parameters} {parameter binding}
** METHOD: sqlite3_stmt
**
** ^(In the SQL statement text input to [sqlite3_prepare_v2()] and its variants,
** literals may be replaced by a [parameter] that matches one of following
** templates:
**
** <ul>
** <li>  ?
** <li>  ?NNN
** <li>  :VVV
** <li>  @VVV
** <li>  $VVV
** </ul>
**
** In the templates above, NNN represents an integer literal,
** and VVV represents an alphanumeric identifier.)^  ^The values of these
** parameters (also called "host parameter names" or "SQL parameters")
** can be set using the sqlite3_bind_*() routines defined here.
**
** ^The first argument to the sqlite3_bind_*() routines is always
** a pointer to the [sqlite3_stmt] object returned from
** [sqlite3_prepare_v2()] or its variants.
**
** ^The second argument is the index of the SQL parameter to be set.
** ^The leftmost SQL parameter has an index of 1.  ^When the same named
** SQL parameter is used more than once, second and subsequent
** occurrences have the same index as the first occurrence.
** ^The index for named parameters can be looked up using the
** [sqlite3_bind_parameter_index()] API if desired.  ^The index
** for "?NNN" parameters is the value of NNN.
** ^The NNN value must be between 1 and the [sqlite3_limit()]
** parameter [SQLITE_LIMIT_VARIABLE_NUMBER] (default value: 32766).
**
** ^The third argument is the value to bind to the parameter.
** ^If the third parameter to sqlite3_bind_text() or sqlite3_bind_text16()
** or sqlite3_bind_blob() is a NULL pointer then the fourth parameter
** is ignored and the end result is the same as sqlite3_bind_null().
** ^If the third parameter to sqlite3_bind_text() is not NULL, then
** it should be a pointer to well-formed UTF8 text.
** ^If the third parameter to sqlite3_bind_text16() is not NULL, then
** it should be a pointer to well-formed UTF16 text.
** ^If the third parameter to sqlite3_bind_text64() is not NULL, then
** it should be a pointer to a well-formed unicode string that is
** either UTF8 if the sixth parameter is SQLITE_UTF8, or UTF16
** otherwise.
**
** [[byte-order determination rules]] ^The byte-order of
** UTF16 input text is determined by the byte-order mark (BOM, U+FEFF)
** found in first character, which is removed, or in the absence of a BOM
** the byte order is the native byte order of the host
** machine for sqlite3_bind_text16() or the byte order specified in
** the 6th parameter for sqlite3_bind_text64().)^
** ^If UTF16 input text contains invalid unicode
** characters, then SQLite might change those invalid characters
** into the unicode replacement character: U+FFFD.
**
** ^(In those routines that have a fourth argument, its value is the
** number of bytes in the parameter.  To be clear: the value is the
** number of <u>bytes</u> in the value, not the number of characters.)^
** ^If the fourth parameter to sqlite3_bind_text() or sqlite3_bind_text16()
** is negative, then the length of the string is
** the number of bytes up to the first zero terminator.
** If the fourth parameter to sqlite3_bind_blob() is negative, then
** the behavior is undefined.
** If a non-negative fourth parameter is provided to sqlite3_bind_text()
** or sqlite3_bind_text16() or sqlite3_bind_text64() then
** that parameter must be the byte offset
** where the NUL terminator would occur assuming the string were NUL
** terminated.  If any NUL characters occurs at byte offsets less than
** the value of the fourth parameter then the resulting string value will
** contain embedded NULs.  The result of expressions involving strings
** with embedded NULs is undefined.
**
** ^The fifth argument to the BLOB and string binding interfaces controls
** or indicates the lifetime of the object referenced by the third parameter.
** These three options exist:
** ^ (1) A destructor to dispose of the BLOB or string after SQLite has finished
** with it may be passed. ^It is called to dispose of the BLOB or string even
** if the call to the bind API fails, except the destructor is not called if
** the third parameter is a NULL pointer or the fourth parameter is negative.
** ^ (2) The special constant, [SQLITE_STATIC], may be passed to indicate that
** the application remains responsible for disposing of the object. ^In this
** case, the object and the provided pointer to it must remain valid until
** either the prepared statement is finalized or the same SQL parameter is
** bound to something else, whichever occurs sooner.
** ^ (3) The constant, [SQLITE_TRANSIENT], may be passed to indicate that the
** object is to be copied prior to the return from sqlite3_bind_*(). ^The
** object and pointer to it must remain valid until then. ^SQLite will then
** manage the lifetime of its private copy.
**
