** METHOD: sqlite3
**
** ^(The sqlite3_wal_checkpoint_v2(D,X,M,L,C) interface runs a checkpoint
** operation on database X of [database connection] D in mode M.  Status
** information is written back into integers pointed to by L and C.)^
** ^(The M parameter must be a valid [checkpoint mode]:)^
**
** <dl>
** <dt>SQLITE_CHECKPOINT_PASSIVE<dd>
**   ^Checkpoint as many frames as possible without waiting for any database
**   readers or writers to finish, then sync the database file if all frames
**   in the log were checkpointed. ^The [busy-handler callback]
**   is never invoked in the SQLITE_CHECKPOINT_PASSIVE mode.
**   ^On the other hand, passive mode might leave the checkpoint unfinished
**   if there are concurrent readers or writers.
**
** <dt>SQLITE_CHECKPOINT_FULL<dd>
**   ^This mode blocks (it invokes the
**   [sqlite3_busy_handler|busy-handler callback]) until there is no
**   database writer and all readers are reading from the most recent database
**   snapshot. ^It then checkpoints all frames in the log file and syncs the
**   database file. ^This mode blocks new database writers while it is pending,
**   but new database readers are allowed to continue unimpeded.
**
** <dt>SQLITE_CHECKPOINT_RESTART<dd>
**   ^This mode works the same way as SQLITE_CHECKPOINT_FULL with the addition
**   that after checkpointing the log file it blocks (calls the
**   [busy-handler callback])
**   until all readers are reading from the database file only. ^This ensures
**   that the next writer will restart the log file from the beginning.
**   ^Like SQLITE_CHECKPOINT_FULL, this mode blocks new
**   database writer attempts while it is pending, but does not impede readers.
**
** <dt>SQLITE_CHECKPOINT_TRUNCATE<dd>
**   ^This mode works the same way as SQLITE_CHECKPOINT_RESTART with the
**   addition that it also truncates the log file to zero bytes just prior
**   to a successful return.
** </dl>
**
** ^If pnLog is not NULL, then *pnLog is set to the total number of frames in
** the log file or to -1 if the checkpoint could not run because
** of an error or because the database is not in [WAL mode]. ^If pnCkpt is not
** NULL,then *pnCkpt is set to the total number of checkpointed frames in the
** log file (including any that were already checkpointed before the function
** was called) or to -1 if the checkpoint could not run due to an error or
** because the database is not in WAL mode. ^Note that upon successful
** completion of an SQLITE_CHECKPOINT_TRUNCATE, the log file will have been
** truncated to zero bytes and so both *pnLog and *pnCkpt will be set to zero.
**
** ^All calls obtain an exclusive "checkpoint" lock on the database file. ^If
** any other process is running a checkpoint operation at the same time, the
** lock cannot be obtained and SQLITE_BUSY is returned. ^Even if there is a
** busy-handler configured, it will not be invoked in this case.
**
** ^The SQLITE_CHECKPOINT_FULL, RESTART and TRUNCATE modes also obtain the
** exclusive "writer" lock on the database file. ^If the writer lock cannot be
** obtained immediately, and a busy-handler is configured, it is invoked and
** the writer lock retried until either the busy-handler returns 0 or the lock
** is successfully obtained. ^The busy-handler is also invoked while waiting for
** database readers as described above. ^If the busy-handler returns 0 before
** the writer lock is obtained or while waiting for database readers, the
** checkpoint operation proceeds from that point in the same way as
** SQLITE_CHECKPOINT_PASSIVE - checkpointing as many frames as possible
** without blocking any further. ^SQLITE_BUSY is returned in this case.
**
** ^If parameter zDb is NULL or points to a zero length string, then the
** specified operation is attempted on all WAL databases [attached] to
** [database connection] db.  In this case the
** values written to output parameters *pnLog and *pnCkpt are undefined. ^If
** an SQLITE_BUSY error is encountered when processing one or more of the
** attached WAL databases, the operation is still attempted on any remaining
** attached databases and SQLITE_BUSY is returned at the end. ^If any other
** error occurs while processing an attached database, processing is abandoned
** and the error code is returned to the caller immediately. ^If no error
** (SQLITE_BUSY or otherwise) is encountered while processing the attached
** databases, SQLITE_OK is returned.
**
** ^If database zDb is the name of an attached database that is not in WAL
** mode, SQLITE_OK is returned and both *pnLog and *pnCkpt set to -1. ^If
** zDb is not NULL (or a zero length string) and is not the name of any
** attached database, SQLITE_ERROR is returned to the caller.
**
** ^Unless it returns SQLITE_MISUSE,
** the sqlite3_wal_checkpoint_v2() interface
** sets the error information that is queried by
** [sqlite3_errcode()] and [sqlite3_errmsg()].
**
** ^The [PRAGMA wal_checkpoint] command can be used to invoke this interface
** from SQL.
*/
SQLITE_API int sqlite3_wal_checkpoint_v2(
  sqlite3 *db,                    /* Database handle */
  const char *zDb,                /* Name of attached database (or NULL) */
  int eMode,                      /* SQLITE_CHECKPOINT_* value */
  int *pnLog,                     /* OUT: Size of WAL log in frames */
  int *pnCkpt                     /* OUT: Total number of frames checkpointed */
);

/*
** CAPI3REF: Checkpoint Mode Values
** KEYWORDS: {checkpoint mode}
**
** These constants define all valid values for the "checkpoint mode" passed
** as the third parameter to the [sqlite3_wal_checkpoint_v2()] interface.
** See the [sqlite3_wal_checkpoint_v2()] documentation for details on the
** meaning of each of these checkpoint modes.
*/
#define SQLITE_CHECKPOINT_PASSIVE  0  /* Do as much as possible w/o blocking */
#define SQLITE_CHECKPOINT_FULL     1  /* Wait for writers, then checkpoint */
#define SQLITE_CHECKPOINT_RESTART  2  /* Like FULL but wait for readers */
#define SQLITE_CHECKPOINT_TRUNCATE 3  /* Like RESTART but also truncate WAL */

/*
** CAPI3REF: Virtual Table Interface Configuration
**
** This function may be called by either the [xConnect] or [xCreate] method
** of a [virtual table] implementation to configure
** various facets of the virtual table interface.
**
** If this interface is invoked outside the context of an xConnect or
** xCreate virtual table method then the behavior is undefined.
**
** In the call sqlite3_vtab_config(D,C,...) the D parameter is the
** [database connection] in which the virtual table is being created and
** which is passed in as the first argument to the [xConnect] or [xCreate]
** method that is invoking sqlite3_vtab_config().  The C parameter is one
** of the [virtual table configuration options].  The presence and meaning
** of parameters after C depend on which [virtual table configuration option]
** is used.
*/
SQLITE_API int sqlite3_vtab_config(sqlite3*, int op, ...);

/*
** CAPI3REF: Virtual Table Configuration Options
** KEYWORDS: {virtual table configuration options}
** KEYWORDS: {virtual table configuration option}
**
** These macros define the various options to the
** [sqlite3_vtab_config()] interface that [virtual table] implementations
** can use to customize and optimize their behavior.
**
** <dl>
** [[SQLITE_VTAB_CONSTRAINT_SUPPORT]]
** <dt>SQLITE_VTAB_CONSTRAINT_SUPPORT</dt>
** <dd>Calls of the form
** [sqlite3_vtab_config](db,SQLITE_VTAB_CONSTRAINT_SUPPORT,X) are supported,
** where X is an integer.  If X is zero, then the [virtual table] whose
** [xCreate] or [xConnect] method invoked [sqlite3_vtab_config()] does not
** support constraints.  In this configuration (which is the default) if
** a call to the [xUpdate] method returns [SQLITE_CONSTRAINT], then the entire
** statement is rolled back as if [ON CONFLICT | OR ABORT] had been
** specified as part of the users SQL statement, regardless of the actual
** ON CONFLICT mode specified.
**
** If X is non-zero, then the virtual table implementation guarantees
** that if [xUpdate] returns [SQLITE_CONSTRAINT], it will do so before
** any modifications to internal or persistent data structures have been made.
** If the [ON CONFLICT] mode is ABORT, FAIL, IGNORE or ROLLBACK, SQLite
** is able to roll back a statement or database transaction, and abandon
** or continue processing the current SQL statement as appropriate.
** If the ON CONFLICT mode is REPLACE and the [xUpdate] method returns
** [SQLITE_CONSTRAINT], SQLite handles this as if the ON CONFLICT mode
** had been ABORT.
**
** Virtual table implementations that are required to handle OR REPLACE
** must do so within the [xUpdate] method. If a call to the
** [sqlite3_vtab_on_conflict()] function indicates that the current ON
** CONFLICT policy is REPLACE, the virtual table implementation should
** silently replace the appropriate rows within the xUpdate callback and
** return SQLITE_OK. Or, if this is not possible, it may return
** SQLITE_CONSTRAINT, in which case SQLite falls back to OR ABORT
** constraint handling.
** </dd>
**
** [[SQLITE_VTAB_DIRECTONLY]]<dt>SQLITE_VTAB_DIRECTONLY</dt>
** <dd>Calls of the form
** [sqlite3_vtab_config](db,SQLITE_VTAB_DIRECTONLY) from within the
** the [xConnect] or [xCreate] methods of a [virtual table] implementation
** prohibits that virtual table from being used from within triggers and
** views.
** </dd>
**
** [[SQLITE_VTAB_INNOCUOUS]]<dt>SQLITE_VTAB_INNOCUOUS</dt>
** <dd>Calls of the form
** [sqlite3_vtab_config](db,SQLITE_VTAB_INNOCUOUS) from within the
** the [xConnect] or [xCreate] methods of a [virtual table] implementation
** identify that virtual table as being safe to use from within triggers
** and views.  Conceptually, the SQLITE_VTAB_INNOCUOUS tag means that the
** virtual table can do no serious harm even if it is controlled by a
** malicious hacker.  Developers should avoid setting the SQLITE_VTAB_INNOCUOUS
** flag unless absolutely necessary.
** </dd>
**
** [[SQLITE_VTAB_USES_ALL_SCHEMAS]]<dt>SQLITE_VTAB_USES_ALL_SCHEMAS</dt>
** <dd>Calls of the form
** [sqlite3_vtab_config](db,SQLITE_VTAB_USES_ALL_SCHEMA) from within the
** the [xConnect] or [xCreate] methods of a [virtual table] implementation
** instruct the query planner to begin at least a read transaction on
** all schemas ("main", "temp", and any ATTACH-ed databases) whenever the
** virtual table is used.
** </dd>
** </dl>
*/
#define SQLITE_VTAB_CONSTRAINT_SUPPORT 1
#define SQLITE_VTAB_INNOCUOUS          2
#define SQLITE_VTAB_DIRECTONLY         3
#define SQLITE_VTAB_USES_ALL_SCHEMAS   4

/*
** CAPI3REF: Determine The Virtual Table Conflict Policy
**
** This function may only be called from within a call to the [xUpdate] method
** of a [virtual table] implementation for an INSERT or UPDATE operation. ^The
** value returned is one of [SQLITE_ROLLBACK], [SQLITE_IGNORE], [SQLITE_FAIL],
** [SQLITE_ABORT], or [SQLITE_REPLACE], according to the [ON CONFLICT] mode
** of the SQL statement that triggered the call to the [xUpdate] method of the
** [virtual table].
*/
SQLITE_API int sqlite3_vtab_on_conflict(sqlite3 *);

/*
** CAPI3REF: Determine If Virtual Table Column Access Is For UPDATE
**
** If the sqlite3_vtab_nochange(X) routine is called within the [xColumn]
** method of a [virtual table], then it might return true if the
** column is being fetched as part of an UPDATE operation during which the
** column value will not change.  The virtual table implementation can use
** this hint as permission to substitute a return value that is less
** expensive to compute and that the corresponding
** [xUpdate] method understands as a "no-change" value.
**
** If the [xColumn] method calls sqlite3_vtab_nochange() and finds that
** the column is not changed by the UPDATE statement, then the xColumn
** method can optionally return without setting a result, without calling
** any of the [sqlite3_result_int|sqlite3_result_xxxxx() interfaces].
** In that case, [sqlite3_value_nochange(X)] will return true for the
** same column in the [xUpdate] method.
**
** The sqlite3_vtab_nochange() routine is an optimization.  Virtual table
** implementations should continue to give a correct answer even if the
** sqlite3_vtab_nochange() interface were to always return false.  In the
** current implementation, the sqlite3_vtab_nochange() interface does always
** returns false for the enhanced [UPDATE FROM] statement.
*/
SQLITE_API int sqlite3_vtab_nochange(sqlite3_context*);

/*
** CAPI3REF: Determine The Collation For a Virtual Table Constraint
** METHOD: sqlite3_index_info
**
** This function may only be called from within a call to the [xBestIndex]
** method of a [virtual table].  This function returns a pointer to a string
** that is the name of the appropriate collation sequence to use for text
** comparisons on the constraint identified by its arguments.
**
** The first argument must be the pointer to the [sqlite3_index_info] object
** that is the first parameter to the xBestIndex() method. The second argument
** must be an index into the aConstraint[] array belonging to the
** sqlite3_index_info structure passed to xBestIndex.
**
** Important:
** The first parameter must be the same pointer that is passed into the
** xBestMethod() method.  The first parameter may not be a pointer to a
** different [sqlite3_index_info] object, even an exact copy.
**
** The return value is computed as follows:
**
** <ol>
** <li><p> If the constraint comes from a WHERE clause expression that contains
**         a [COLLATE operator], then the name of the collation specified by
**         that COLLATE operator is returned.
** <li><p> If there is no COLLATE operator, but the column that is the subject
**         of the constraint specifies an alternative collating sequence via
**         a [COLLATE clause] on the column definition within the CREATE TABLE
**         statement that was passed into [sqlite3_declare_vtab()], then the
**         name of that alternative collating sequence is returned.
** <li><p> Otherwise, "BINARY" is returned.
** </ol>
*/
SQLITE_API const char *sqlite3_vtab_collation(sqlite3_index_info*,int);

/*
** CAPI3REF: Determine if a virtual table query is DISTINCT
** METHOD: sqlite3_index_info
**
** This API may only be used from within an [xBestIndex|xBestIndex method]
** of a [virtual table] implementation. The result of calling this
** interface from outside of xBestIndex() is undefined and probably harmful.
**
** ^The sqlite3_vtab_distinct() interface returns an integer between 0 and
** 3.  The integer returned by sqlite3_vtab_distinct()
** gives the virtual table additional information about how the query
** planner wants the output to be ordered. As long as the virtual table
** can meet the ordering requirements of the query planner, it may set
** the "orderByConsumed" flag.
**
** <ol><li value="0"><p>
** ^If the sqlite3_vtab_distinct() interface returns 0, that means
** that the query planner needs the virtual table to return all rows in the
** sort order defined by the "nOrderBy" and "aOrderBy" fields of the
** [sqlite3_index_info] object.  This is the default expectation.  If the
** virtual table outputs all rows in sorted order, then it is always safe for
** the xBestIndex method to set the "orderByConsumed" flag, regardless of
** the return value from sqlite3_vtab_distinct().
** <li value="1"><p>
** ^(If the sqlite3_vtab_distinct() interface returns 1, that means
** that the query planner does not need the rows to be returned in sorted order
** as long as all rows with the same values in all columns identified by the
** "aOrderBy" field are adjacent.)^  This mode is used when the query planner
** is doing a GROUP BY.
** <li value="2"><p>
** ^(If the sqlite3_vtab_distinct() interface returns 2, that means
** that the query planner does not need the rows returned in any particular
** order, as long as rows with the same values in all columns identified
** by "aOrderBy" are adjacent.)^  ^(Furthermore, when two or more rows
** contain the same values for all columns identified by "colUsed", all but
** one such row may optionally be omitted from the result.)^
** The virtual table is not required to omit rows that are duplicates
** over the "colUsed" columns, but if the virtual table can do that without
** too much extra effort, it could potentially help the query to run faster.
** This mode is used for a DISTINCT query.
** <li value="3"><p>
** ^(If the sqlite3_vtab_distinct() interface returns 3, that means the
** virtual table must return rows in the order defined by "aOrderBy" as
** if the sqlite3_vtab_distinct() interface had returned 0.  However if
** two or more rows in the result have the same values for all columns
** identified by "colUsed", then all but one such row may optionally be
** omitted.)^  Like when the return value is 2, the virtual table
** is not required to omit rows that are duplicates over the "colUsed"
** columns, but if the virtual table can do that without
** too much extra effort, it could potentially help the query to run faster.
** This mode is used for queries
** that have both DISTINCT and ORDER BY clauses.
** </ol>
**
** <p>The following table summarizes the conditions under which the
** virtual table is allowed to set the "orderByConsumed" flag based on
** the value returned by sqlite3_vtab_distinct().  This table is a
** restatement of the previous four paragraphs:
**
** <table border=1 cellspacing=0 cellpadding=10 width="90%">
** <tr>
** <td valign="top">sqlite3_vtab_distinct() return value
** <td valign="top">Rows are returned in aOrderBy order
** <td valign="top">Rows with the same value in all aOrderBy columns are adjacent
** <td valign="top">Duplicates over all colUsed columns may be omitted
** <tr><td>0<td>yes<td>yes<td>no
** <tr><td>1<td>no<td>yes<td>no
** <tr><td>2<td>no<td>yes<td>yes
** <tr><td>3<td>yes<td>yes<td>yes
