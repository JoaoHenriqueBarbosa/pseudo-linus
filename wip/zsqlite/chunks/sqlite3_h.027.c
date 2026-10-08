** </table>
**
** ^For the purposes of comparing virtual table output values to see if the
** values are same value for sorting purposes, two NULL values are considered
** to be the same.  In other words, the comparison operator is "IS"
** (or "IS NOT DISTINCT FROM") and not "==".
**
** If a virtual table implementation is unable to meet the requirements
** specified above, then it must not set the "orderByConsumed" flag in the
** [sqlite3_index_info] object or an incorrect answer may result.
**
** ^A virtual table implementation is always free to return rows in any order
** it wants, as long as the "orderByConsumed" flag is not set.  ^When the
** the "orderByConsumed" flag is unset, the query planner will add extra
** [bytecode] to ensure that the final results returned by the SQL query are
** ordered correctly.  The use of the "orderByConsumed" flag and the
** sqlite3_vtab_distinct() interface is merely an optimization.  ^Careful
** use of the sqlite3_vtab_distinct() interface and the "orderByConsumed"
** flag might help queries against a virtual table to run faster.  Being
** overly aggressive and setting the "orderByConsumed" flag when it is not
** valid to do so, on the other hand, might cause SQLite to return incorrect
** results.
*/
SQLITE_API int sqlite3_vtab_distinct(sqlite3_index_info*);

/*
** CAPI3REF: Identify and handle IN constraints in xBestIndex
**
** This interface may only be used from within an
** [xBestIndex|xBestIndex() method] of a [virtual table] implementation.
** The result of invoking this interface from any other context is
** undefined and probably harmful.
**
** ^(A constraint on a virtual table of the form
** "[IN operator|column IN (...)]" is
** communicated to the xBestIndex method as a
** [SQLITE_INDEX_CONSTRAINT_EQ] constraint.)^  If xBestIndex wants to use
** this constraint, it must set the corresponding
** aConstraintUsage[].argvIndex to a positive integer.  ^(Then, under
** the usual mode of handling IN operators, SQLite generates [bytecode]
** that invokes the [xFilter|xFilter() method] once for each value
** on the right-hand side of the IN operator.)^  Thus the virtual table
** only sees a single value from the right-hand side of the IN operator
** at a time.
**
** In some cases, however, it would be advantageous for the virtual
** table to see all values on the right-hand of the IN operator all at
** once.  The sqlite3_vtab_in() interfaces facilitates this in two ways:
**
** <ol>
** <li><p>
**   ^A call to sqlite3_vtab_in(P,N,-1) will return true (non-zero)
**   if and only if the [sqlite3_index_info|P->aConstraint][N] constraint
**   is an [IN operator] that can be processed all at once.  ^In other words,
**   sqlite3_vtab_in() with -1 in the third argument is a mechanism
**   by which the virtual table can ask SQLite if all-at-once processing
**   of the IN operator is even possible.
**
** <li><p>
**   ^A call to sqlite3_vtab_in(P,N,F) with F==1 or F==0 indicates
**   to SQLite that the virtual table does or does not want to process
**   the IN operator all-at-once, respectively.  ^Thus when the third
**   parameter (F) is non-negative, this interface is the mechanism by
**   which the virtual table tells SQLite how it wants to process the
**   IN operator.
** </ol>
**
** ^The sqlite3_vtab_in(P,N,F) interface can be invoked multiple times
** within the same xBestIndex method call.  ^For any given P,N pair,
** the return value from sqlite3_vtab_in(P,N,F) will always be the same
** within the same xBestIndex call.  ^If the interface returns true
** (non-zero), that means that the constraint is an IN operator
** that can be processed all-at-once.  ^If the constraint is not an IN
** operator or cannot be processed all-at-once, then the interface returns
** false.
**
** ^(All-at-once processing of the IN operator is selected if both of the
** following conditions are met:
**
** <ol>
** <li><p> The P->aConstraintUsage[N].argvIndex value is set to a positive
** integer.  This is how the virtual table tells SQLite that it wants to
** use the N-th constraint.
**
** <li><p> The last call to sqlite3_vtab_in(P,N,F) for which F was
** non-negative had F>=1.
** </ol>)^
**
** ^If either or both of the conditions above are false, then SQLite uses
** the traditional one-at-a-time processing strategy for the IN constraint.
** ^If both conditions are true, then the argvIndex-th parameter to the
** xFilter method will be an [sqlite3_value] that appears to be NULL,
** but which can be passed to [sqlite3_vtab_in_first()] and
** [sqlite3_vtab_in_next()] to find all values on the right-hand side
** of the IN constraint.
*/
SQLITE_API int sqlite3_vtab_in(sqlite3_index_info*, int iCons, int bHandle);

/*
** CAPI3REF: Find all elements on the right-hand side of an IN constraint.
**
** These interfaces are only useful from within the
** [xFilter|xFilter() method] of a [virtual table] implementation.
** The result of invoking these interfaces from any other context
** is undefined and probably harmful.
**
** The X parameter in a call to sqlite3_vtab_in_first(X,P) or
** sqlite3_vtab_in_next(X,P) should be one of the parameters to the
** xFilter method which invokes these routines, and specifically
** a parameter that was previously selected for all-at-once IN constraint
** processing use the [sqlite3_vtab_in()] interface in the
** [xBestIndex|xBestIndex method].  ^(If the X parameter is not
** an xFilter argument that was selected for all-at-once IN constraint
** processing, then these routines return [SQLITE_ERROR].)^
**
** ^(Use these routines to access all values on the right-hand side
** of the IN constraint using code like the following:
**
** <blockquote><pre>
** &nbsp;  for(rc=sqlite3_vtab_in_first(pList, &pVal);
** &nbsp;      rc==SQLITE_OK && pVal;
** &nbsp;      rc=sqlite3_vtab_in_next(pList, &pVal)
** &nbsp;  ){
** &nbsp;    // do something with pVal
** &nbsp;  }
** &nbsp;  if( rc!=SQLITE_OK ){
** &nbsp;    // an error has occurred
** &nbsp;  }
** </pre></blockquote>)^
**
** ^On success, the sqlite3_vtab_in_first(X,P) and sqlite3_vtab_in_next(X,P)
** routines return SQLITE_OK and set *P to point to the first or next value
** on the RHS of the IN constraint.  ^If there are no more values on the
** right hand side of the IN constraint, then *P is set to NULL and these
** routines return [SQLITE_DONE].  ^The return value might be
** some other value, such as SQLITE_NOMEM, in the event of a malfunction.
**
** The *ppOut values returned by these routines are only valid until the
** next call to either of these routines or until the end of the xFilter
** method from which these routines were called.  If the virtual table
** implementation needs to retain the *ppOut values for longer, it must make
** copies.  The *ppOut values are [protected sqlite3_value|protected].
*/
SQLITE_API int sqlite3_vtab_in_first(sqlite3_value *pVal, sqlite3_value **ppOut);
SQLITE_API int sqlite3_vtab_in_next(sqlite3_value *pVal, sqlite3_value **ppOut);

/*
** CAPI3REF: Constraint values in xBestIndex()
** METHOD: sqlite3_index_info
**
** This API may only be used from within the [xBestIndex|xBestIndex method]
** of a [virtual table] implementation. The result of calling this interface
** from outside of an xBestIndex method are undefined and probably harmful.
**
** ^When the sqlite3_vtab_rhs_value(P,J,V) interface is invoked from within
** the [xBestIndex] method of a [virtual table] implementation, with P being
** a copy of the [sqlite3_index_info] object pointer passed into xBestIndex and
** J being a 0-based index into P->aConstraint[], then this routine
** attempts to set *V to the value of the right-hand operand of
** that constraint if the right-hand operand is known.  ^If the
** right-hand operand is not known, then *V is set to a NULL pointer.
** ^The sqlite3_vtab_rhs_value(P,J,V) interface returns SQLITE_OK if
** and only if *V is set to a value.  ^The sqlite3_vtab_rhs_value(P,J,V)
** inteface returns SQLITE_NOTFOUND if the right-hand side of the J-th
** constraint is not available.  ^The sqlite3_vtab_rhs_value() interface
** can return an result code other than SQLITE_OK or SQLITE_NOTFOUND if
** something goes wrong.
**
** The sqlite3_vtab_rhs_value() interface is usually only successful if
** the right-hand operand of a constraint is a literal value in the original
** SQL statement.  If the right-hand operand is an expression or a reference
** to some other column or a [host parameter], then sqlite3_vtab_rhs_value()
** will probably return [SQLITE_NOTFOUND].
**
** ^(Some constraints, such as [SQLITE_INDEX_CONSTRAINT_ISNULL] and
** [SQLITE_INDEX_CONSTRAINT_ISNOTNULL], have no right-hand operand.  For such
** constraints, sqlite3_vtab_rhs_value() always returns SQLITE_NOTFOUND.)^
**
** ^The [sqlite3_value] object returned in *V is a protected sqlite3_value
** and remains valid for the duration of the xBestIndex method call.
** ^When xBestIndex returns, the sqlite3_value object returned by
** sqlite3_vtab_rhs_value() is automatically deallocated.
**
** The "_rhs_" in the name of this routine is an abbreviation for
** "Right-Hand Side".
*/
SQLITE_API int sqlite3_vtab_rhs_value(sqlite3_index_info*, int, sqlite3_value **ppVal);

/*
** CAPI3REF: Conflict resolution modes
** KEYWORDS: {conflict resolution mode}
**
** These constants are returned by [sqlite3_vtab_on_conflict()] to
** inform a [virtual table] implementation what the [ON CONFLICT] mode
** is for the SQL statement being evaluated.
**
** Note that the [SQLITE_IGNORE] constant is also used as a potential
** return value from the [sqlite3_set_authorizer()] callback and that
** [SQLITE_ABORT] is also a [result code].
*/
#define SQLITE_ROLLBACK 1
/* #define SQLITE_IGNORE 2 // Also used by sqlite3_authorizer() callback */
#define SQLITE_FAIL     3
/* #define SQLITE_ABORT 4  // Also an error code */
#define SQLITE_REPLACE  5

/*
** CAPI3REF: Prepared Statement Scan Status Opcodes
** KEYWORDS: {scanstatus options}
**
** The following constants can be used for the T parameter to the
** [sqlite3_stmt_scanstatus(S,X,T,V)] interface.  Each constant designates a
** different metric for sqlite3_stmt_scanstatus() to return.
**
** When the value returned to V is a string, space to hold that string is
** managed by the prepared statement S and will be automatically freed when
** S is finalized.
**
** Not all values are available for all query elements. When a value is
** not available, the output variable is set to -1 if the value is numeric,
** or to NULL if it is a string (SQLITE_SCANSTAT_NAME).
**
** <dl>
** [[SQLITE_SCANSTAT_NLOOP]] <dt>SQLITE_SCANSTAT_NLOOP</dt>
** <dd>^The [sqlite3_int64] variable pointed to by the V parameter will be
** set to the total number of times that the X-th loop has run.</dd>
**
** [[SQLITE_SCANSTAT_NVISIT]] <dt>SQLITE_SCANSTAT_NVISIT</dt>
** <dd>^The [sqlite3_int64] variable pointed to by the V parameter will be set
** to the total number of rows examined by all iterations of the X-th loop.</dd>
**
** [[SQLITE_SCANSTAT_EST]] <dt>SQLITE_SCANSTAT_EST</dt>
** <dd>^The "double" variable pointed to by the V parameter will be set to the
** query planner's estimate for the average number of rows output from each
** iteration of the X-th loop.  If the query planner's estimates was accurate,
** then this value will approximate the quotient NVISIT/NLOOP and the
** product of this value for all prior loops with the same SELECTID will
** be the NLOOP value for the current loop.
**
** [[SQLITE_SCANSTAT_NAME]] <dt>SQLITE_SCANSTAT_NAME</dt>
** <dd>^The "const char *" variable pointed to by the V parameter will be set
** to a zero-terminated UTF-8 string containing the name of the index or table
** used for the X-th loop.
**
** [[SQLITE_SCANSTAT_EXPLAIN]] <dt>SQLITE_SCANSTAT_EXPLAIN</dt>
** <dd>^The "const char *" variable pointed to by the V parameter will be set
** to a zero-terminated UTF-8 string containing the [EXPLAIN QUERY PLAN]
** description for the X-th loop.
**
** [[SQLITE_SCANSTAT_SELECTID]] <dt>SQLITE_SCANSTAT_SELECTID</dt>
** <dd>^The "int" variable pointed to by the V parameter will be set to the
** id for the X-th query plan element. The id value is unique within the
** statement. The select-id is the same value as is output in the first
** column of an [EXPLAIN QUERY PLAN] query.
**
** [[SQLITE_SCANSTAT_PARENTID]] <dt>SQLITE_SCANSTAT_PARENTID</dt>
** <dd>The "int" variable pointed to by the V parameter will be set to the
** the id of the parent of the current query element, if applicable, or
** to zero if the query element has no parent. This is the same value as
** returned in the second column of an [EXPLAIN QUERY PLAN] query.
**
** [[SQLITE_SCANSTAT_NCYCLE]] <dt>SQLITE_SCANSTAT_NCYCLE</dt>
** <dd>The sqlite3_int64 output value is set to the number of cycles,
** according to the processor time-stamp counter, that elapsed while the
** query element was being processed. This value is not available for
** all query elements - if it is unavailable the output variable is
** set to -1.
** </dl>
*/
#define SQLITE_SCANSTAT_NLOOP    0
#define SQLITE_SCANSTAT_NVISIT   1
#define SQLITE_SCANSTAT_EST      2
#define SQLITE_SCANSTAT_NAME     3
#define SQLITE_SCANSTAT_EXPLAIN  4
#define SQLITE_SCANSTAT_SELECTID 5
#define SQLITE_SCANSTAT_PARENTID 6
#define SQLITE_SCANSTAT_NCYCLE   7

/*
** CAPI3REF: Prepared Statement Scan Status
** METHOD: sqlite3_stmt
**
** These interfaces return information about the predicted and measured
** performance for pStmt.  Advanced applications can use this
** interface to compare the predicted and the measured performance and
** issue warnings and/or rerun [ANALYZE] if discrepancies are found.
**
** Since this interface is expected to be rarely used, it is only
** available if SQLite is compiled using the [SQLITE_ENABLE_STMT_SCANSTATUS]
** compile-time option.
**
** The "iScanStatusOp" parameter determines which status information to return.
** The "iScanStatusOp" must be one of the [scanstatus options] or the behavior
** of this interface is undefined. ^The requested measurement is written into
** a variable pointed to by the "pOut" parameter.
**
** The "flags" parameter must be passed a mask of flags. At present only
** one flag is defined - SQLITE_SCANSTAT_COMPLEX. If SQLITE_SCANSTAT_COMPLEX
** is specified, then status information is available for all elements
** of a query plan that are reported by "EXPLAIN QUERY PLAN" output. If
** SQLITE_SCANSTAT_COMPLEX is not specified, then only query plan elements
** that correspond to query loops (the "SCAN..." and "SEARCH..." elements of
** the EXPLAIN QUERY PLAN output) are available. Invoking API
** sqlite3_stmt_scanstatus() is equivalent to calling
** sqlite3_stmt_scanstatus_v2() with a zeroed flags parameter.
**
** Parameter "idx" identifies the specific query element to retrieve statistics
** for. Query elements are numbered starting from zero. A value of -1 may be
** to query for statistics regarding the entire query. ^If idx is out of range
** - less than -1 or greater than or equal to the total number of query
** elements used to implement the statement - a non-zero value is returned and
** the variable that pOut points to is unchanged.
**
** See also: [sqlite3_stmt_scanstatus_reset()]
*/
SQLITE_API int sqlite3_stmt_scanstatus(
  sqlite3_stmt *pStmt,      /* Prepared statement for which info desired */
  int idx,                  /* Index of loop to report on */
  int iScanStatusOp,        /* Information desired.  SQLITE_SCANSTAT_* */
  void *pOut                /* Result written here */
);
SQLITE_API int sqlite3_stmt_scanstatus_v2(
  sqlite3_stmt *pStmt,      /* Prepared statement for which info desired */
  int idx,                  /* Index of loop to report on */
  int iScanStatusOp,        /* Information desired.  SQLITE_SCANSTAT_* */
  int flags,                /* Mask of flags defined below */
  void *pOut                /* Result written here */
);

/*
** CAPI3REF: Prepared Statement Scan Status
** KEYWORDS: {scan status flags}
*/
#define SQLITE_SCANSTAT_COMPLEX 0x0001

/*
** CAPI3REF: Zero Scan-Status Counters
** METHOD: sqlite3_stmt
**
** ^Zero all [sqlite3_stmt_scanstatus()] related event counters.
**
** This API is only available if the library is built with pre-processor
** symbol [SQLITE_ENABLE_STMT_SCANSTATUS] defined.
*/
SQLITE_API void sqlite3_stmt_scanstatus_reset(sqlite3_stmt*);

/*
** CAPI3REF: Flush caches to disk mid-transaction
** METHOD: sqlite3
**
** ^If a write-transaction is open on [database connection] D when the
** [sqlite3_db_cacheflush(D)] interface invoked, any dirty
** pages in the pager-cache that are not currently in use are written out
** to disk. A dirty page may be in use if a database cursor created by an
** active SQL statement is reading from it, or if it is page 1 of a database
** file (page 1 is always "in use").  ^The [sqlite3_db_cacheflush(D)]
** interface flushes caches for all schemas - "main", "temp", and
** any [attached] databases.
**
** ^If this function needs to obtain extra database locks before dirty pages
** can be flushed to disk, it does so. ^If those locks cannot be obtained
** immediately and there is a busy-handler callback configured, it is invoked
** in the usual manner. ^If the required lock still cannot be obtained, then
** the database is skipped and an attempt made to flush any dirty pages
** belonging to the next (if any) database. ^If any databases are skipped
** because locks cannot be obtained, but no other error occurs, this
** function returns SQLITE_BUSY.
**
** ^If any other error occurs while flushing dirty pages to disk (for
** example an IO error or out-of-memory condition), then processing is
** abandoned and an SQLite [error code] is returned to the caller immediately.
**
** ^Otherwise, if no error occurs, [sqlite3_db_cacheflush()] returns SQLITE_OK.
**
** ^This function does not set the database handle error code or message
** returned by the [sqlite3_errcode()] and [sqlite3_errmsg()] functions.
*/
SQLITE_API int sqlite3_db_cacheflush(sqlite3*);

/*
** CAPI3REF: The pre-update hook.
** METHOD: sqlite3
**
** ^These interfaces are only available if SQLite is compiled using the
** [SQLITE_ENABLE_PREUPDATE_HOOK] compile-time option.
**
** ^The [sqlite3_preupdate_hook()] interface registers a callback function
** that is invoked prior to each [INSERT], [UPDATE], and [DELETE] operation
** on a database table.
** ^At most one preupdate hook may be registered at a time on a single
** [database connection]; each call to [sqlite3_preupdate_hook()] overrides
** the previous setting.
** ^The preupdate hook is disabled by invoking [sqlite3_preupdate_hook()]
** with a NULL pointer as the second parameter.
** ^The third parameter to [sqlite3_preupdate_hook()] is passed through as
** the first parameter to callbacks.
**
** ^The preupdate hook only fires for changes to real database tables; the
** preupdate hook is not invoked for changes to [virtual tables] or to
** system tables like sqlite_sequence or sqlite_stat1.
**
** ^The second parameter to the preupdate callback is a pointer to
** the [database connection] that registered the preupdate hook.
** ^The third parameter to the preupdate callback is one of the constants
** [SQLITE_INSERT], [SQLITE_DELETE], or [SQLITE_UPDATE] to identify the
** kind of update operation that is about to occur.
** ^(The fourth parameter to the preupdate callback is the name of the
** database within the database connection that is being modified.  This
** will be "main" for the main database or "temp" for TEMP tables or
** the name given after the AS keyword in the [ATTACH] statement for attached
** databases.)^
** ^The fifth parameter to the preupdate callback is the name of the
** table that is being modified.
**
** For an UPDATE or DELETE operation on a [rowid table], the sixth
** parameter passed to the preupdate callback is the initial [rowid] of the
** row being modified or deleted. For an INSERT operation on a rowid table,
** or any operation on a WITHOUT ROWID table, the value of the sixth
** parameter is undefined. For an INSERT or UPDATE on a rowid table the
** seventh parameter is the final rowid value of the row being inserted
** or updated. The value of the seventh parameter passed to the callback
** function is not defined for operations on WITHOUT ROWID tables, or for
** DELETE operations on rowid tables.
**
** ^The sqlite3_preupdate_hook(D,C,P) function returns the P argument from
** the previous call on the same [database connection] D, or NULL for
** the first call on D.
**
** The [sqlite3_preupdate_old()], [sqlite3_preupdate_new()],
** [sqlite3_preupdate_count()], and [sqlite3_preupdate_depth()] interfaces
** provide additional information about a preupdate event. These routines
** may only be called from within a preupdate callback.  Invoking any of
** these routines from outside of a preupdate callback or with a
** [database connection] pointer that is different from the one supplied
** to the preupdate callback results in undefined and probably undesirable
** behavior.
**
** ^The [sqlite3_preupdate_count(D)] interface returns the number of columns
** in the row that is being inserted, updated, or deleted.
**
** ^The [sqlite3_preupdate_old(D,N,P)] interface writes into P a pointer to
** a [protected sqlite3_value] that contains the value of the Nth column of
** the table row before it is updated.  The N parameter must be between 0
** and one less than the number of columns or the behavior will be
** undefined. This must only be used within SQLITE_UPDATE and SQLITE_DELETE
** preupdate callbacks; if it is used by an SQLITE_INSERT callback then the
** behavior is undefined.  The [sqlite3_value] that P points to
** will be destroyed when the preupdate callback returns.
**
** ^The [sqlite3_preupdate_new(D,N,P)] interface writes into P a pointer to
** a [protected sqlite3_value] that contains the value of the Nth column of
** the table row after it is updated.  The N parameter must be between 0
** and one less than the number of columns or the behavior will be
** undefined. This must only be used within SQLITE_INSERT and SQLITE_UPDATE
** preupdate callbacks; if it is used by an SQLITE_DELETE callback then the
** behavior is undefined.  The [sqlite3_value] that P points to
** will be destroyed when the preupdate callback returns.
**
** ^The [sqlite3_preupdate_depth(D)] interface returns 0 if the preupdate
** callback was invoked as a result of a direct insert, update, or delete
** operation; or 1 for inserts, updates, or deletes invoked by top-level
** triggers; or 2 for changes resulting from triggers called by top-level
** triggers; and so forth.
**
** When the [sqlite3_blob_write()] API is used to update a blob column,
** the pre-update hook is invoked with SQLITE_DELETE. This is because the
** in this case the new values are not available. In this case, when a
** callback made with op==SQLITE_DELETE is actually a write using the
** sqlite3_blob_write() API, the [sqlite3_preupdate_blobwrite()] returns
** the index of the column being written. In other cases, where the
** pre-update hook is being invoked for some other reason, including a
** regular DELETE, sqlite3_preupdate_blobwrite() returns -1.
**
** See also:  [sqlite3_update_hook()]
*/
#if defined(SQLITE_ENABLE_PREUPDATE_HOOK)
SQLITE_API void *sqlite3_preupdate_hook(
  sqlite3 *db,
  void(*xPreUpdate)(
    void *pCtx,                   /* Copy of third arg to preupdate_hook() */
    sqlite3 *db,                  /* Database handle */
    int op,                       /* SQLITE_UPDATE, DELETE or INSERT */
    char const *zDb,              /* Database name */
    char const *zName,            /* Table name */
    sqlite3_int64 iKey1,          /* Rowid of row about to be deleted/updated */
    sqlite3_int64 iKey2           /* New rowid value (for a rowid UPDATE) */
  ),
  void*
);
SQLITE_API int sqlite3_preupdate_old(sqlite3 *, int, sqlite3_value **);
SQLITE_API int sqlite3_preupdate_count(sqlite3 *);
SQLITE_API int sqlite3_preupdate_depth(sqlite3 *);
SQLITE_API int sqlite3_preupdate_new(sqlite3 *, int, sqlite3_value **);
SQLITE_API int sqlite3_preupdate_blobwrite(sqlite3 *);
#endif

/*
** CAPI3REF: Low-level system error code
** METHOD: sqlite3
**
** ^Attempt to return the underlying operating system error code or error
** number that caused the most recent I/O error or failure to open a file.
** The return value is OS-dependent.  For example, on unix systems, after
** [sqlite3_open_v2()] returns [SQLITE_CANTOPEN], this interface could be
** called to get back the underlying "errno" that caused the problem, such
** as ENOSPC, EAUTH, EISDIR, and so forth.
*/
SQLITE_API int sqlite3_system_errno(sqlite3*);

/*
** CAPI3REF: Database Snapshot
** KEYWORDS: {snapshot} {sqlite3_snapshot}
**
** An instance of the snapshot object records the state of a [WAL mode]
** database for some specific point in history.
**
** In [WAL mode], multiple [database connections] that are open on the
** same database file can each be reading a different historical version
** of the database file.  When a [database connection] begins a read
** transaction, that connection sees an unchanging copy of the database
** as it existed for the point in time when the transaction first started.
** Subsequent changes to the database from other connections are not seen
** by the reader until a new read transaction is started.
**
** The sqlite3_snapshot object records state information about an historical
** version of the database file so that it is possible to later open a new read
** transaction that sees that historical version of the database rather than
** the most recent version.
*/
typedef struct sqlite3_snapshot {
  unsigned char hidden[48];
} sqlite3_snapshot;
