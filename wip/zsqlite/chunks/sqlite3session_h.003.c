** detected, SQLITE_CORRUPT is returned. Or, if an out-of-memory condition
** occurs during processing, this function returns SQLITE_NOMEM.
**
** In all cases, if an error occurs the state of the final contents of the
** changegroup is undefined. If no error occurs, SQLITE_OK is returned.
*/
SQLITE_API int sqlite3changegroup_add(sqlite3_changegroup*, int nData, void *pData);

/*
** CAPI3REF: Add A Single Change To A Changegroup
** METHOD: sqlite3_changegroup
**
** This function adds the single change currently indicated by the iterator
** passed as the second argument to the changegroup object. The rules for
** adding the change are just as described for [sqlite3changegroup_add()].
**
** If the change is successfully added to the changegroup, SQLITE_OK is
** returned. Otherwise, an SQLite error code is returned.
**
** The iterator must point to a valid entry when this function is called.
** If it does not, SQLITE_ERROR is returned and no change is added to the
** changegroup. Additionally, the iterator must not have been opened with
** the SQLITE_CHANGESETAPPLY_INVERT flag. In this case SQLITE_ERROR is also
** returned.
*/
SQLITE_API int sqlite3changegroup_add_change(
  sqlite3_changegroup*,
  sqlite3_changeset_iter*
);



/*
** CAPI3REF: Obtain A Composite Changeset From A Changegroup
** METHOD: sqlite3_changegroup
**
** Obtain a buffer containing a changeset (or patchset) representing the
** current contents of the changegroup. If the inputs to the changegroup
** were themselves changesets, the output is a changeset. Or, if the
** inputs were patchsets, the output is also a patchset.
**
** As with the output of the sqlite3session_changeset() and
** sqlite3session_patchset() functions, all changes related to a single
** table are grouped together in the output of this function. Tables appear
** in the same order as for the very first changeset added to the changegroup.
** If the second or subsequent changesets added to the changegroup contain
** changes for tables that do not appear in the first changeset, they are
** appended onto the end of the output changeset, again in the order in
** which they are first encountered.
**
** If an error occurs, an SQLite error code is returned and the output
** variables (*pnData) and (*ppData) are set to 0. Otherwise, SQLITE_OK
** is returned and the output variables are set to the size of and a
** pointer to the output buffer, respectively. In this case it is the
** responsibility of the caller to eventually free the buffer using a
** call to sqlite3_free().
*/
SQLITE_API int sqlite3changegroup_output(
  sqlite3_changegroup*,
  int *pnData,                    /* OUT: Size of output buffer in bytes */
  void **ppData                   /* OUT: Pointer to output buffer */
);

/*
** CAPI3REF: Delete A Changegroup Object
** DESTRUCTOR: sqlite3_changegroup
*/
SQLITE_API void sqlite3changegroup_delete(sqlite3_changegroup*);

/*
** CAPI3REF: Apply A Changeset To A Database
**
** Apply a changeset or patchset to a database. These functions attempt to
** update the "main" database attached to handle db with the changes found in
** the changeset passed via the second and third arguments.
**
** The fourth argument (xFilter) passed to these functions is the "filter
** callback". If it is not NULL, then for each table affected by at least one
** change in the changeset, the filter callback is invoked with
** the table name as the second argument, and a copy of the context pointer
** passed as the sixth argument as the first. If the "filter callback"
** returns zero, then no attempt is made to apply any changes to the table.
** Otherwise, if the return value is non-zero or the xFilter argument to
** is NULL, all changes related to the table are attempted.
**
** For each table that is not excluded by the filter callback, this function
** tests that the target database contains a compatible table. A table is
** considered compatible if all of the following are true:
**
** <ul>
**   <li> The table has the same name as the name recorded in the
**        changeset, and
**   <li> The table has at least as many columns as recorded in the
**        changeset, and
**   <li> The table has primary key columns in the same position as
**        recorded in the changeset.
** </ul>
**
** If there is no compatible table, it is not an error, but none of the
** changes associated with the table are applied. A warning message is issued
** via the sqlite3_log() mechanism with the error code SQLITE_SCHEMA. At most
** one such warning is issued for each table in the changeset.
**
** For each change for which there is a compatible table, an attempt is made
** to modify the table contents according to the UPDATE, INSERT or DELETE
** change. If a change cannot be applied cleanly, the conflict handler
** function passed as the fifth argument to sqlite3changeset_apply() may be
** invoked. A description of exactly when the conflict handler is invoked for
** each type of change is below.
**
** Unlike the xFilter argument, xConflict may not be passed NULL. The results
** of passing anything other than a valid function pointer as the xConflict
** argument are undefined.
**
** Each time the conflict handler function is invoked, it must return one
** of [SQLITE_CHANGESET_OMIT], [SQLITE_CHANGESET_ABORT] or
** [SQLITE_CHANGESET_REPLACE]. SQLITE_CHANGESET_REPLACE may only be returned
** if the second argument passed to the conflict handler is either
** SQLITE_CHANGESET_DATA or SQLITE_CHANGESET_CONFLICT. If the conflict-handler
** returns an illegal value, any changes already made are rolled back and
** the call to sqlite3changeset_apply() returns SQLITE_MISUSE. Different
** actions are taken by sqlite3changeset_apply() depending on the value
** returned by each invocation of the conflict-handler function. Refer to
** the documentation for the three
** [SQLITE_CHANGESET_OMIT|available return values] for details.
**
** <dl>
** <dt>DELETE Changes<dd>
**   For each DELETE change, the function checks if the target database
**   contains a row with the same primary key value (or values) as the
**   original row values stored in the changeset. If it does, and the values
**   stored in all non-primary key columns also match the values stored in
**   the changeset the row is deleted from the target database.
**
**   If a row with matching primary key values is found, but one or more of
**   the non-primary key fields contains a value different from the original
**   row value stored in the changeset, the conflict-handler function is
**   invoked with [SQLITE_CHANGESET_DATA] as the second argument. If the
**   database table has more columns than are recorded in the changeset,
**   only the values of those non-primary key fields are compared against
**   the current database contents - any trailing database table columns
**   are ignored.
**
**   If no row with matching primary key values is found in the database,
**   the conflict-handler function is invoked with [SQLITE_CHANGESET_NOTFOUND]
**   passed as the second argument.
**
**   If the DELETE operation is attempted, but SQLite returns SQLITE_CONSTRAINT
**   (which can only happen if a foreign key constraint is violated), the
**   conflict-handler function is invoked with [SQLITE_CHANGESET_CONSTRAINT]
**   passed as the second argument. This includes the case where the DELETE
**   operation is attempted because an earlier call to the conflict handler
**   function returned [SQLITE_CHANGESET_REPLACE].
**
** <dt>INSERT Changes<dd>
**   For each INSERT change, an attempt is made to insert the new row into
**   the database. If the changeset row contains fewer fields than the
**   database table, the trailing fields are populated with their default
**   values.
**
**   If the attempt to insert the row fails because the database already
**   contains a row with the same primary key values, the conflict handler
**   function is invoked with the second argument set to
**   [SQLITE_CHANGESET_CONFLICT].
**
**   If the attempt to insert the row fails because of some other constraint
**   violation (e.g. NOT NULL or UNIQUE), the conflict handler function is
**   invoked with the second argument set to [SQLITE_CHANGESET_CONSTRAINT].
**   This includes the case where the INSERT operation is re-attempted because
**   an earlier call to the conflict handler function returned
**   [SQLITE_CHANGESET_REPLACE].
**
** <dt>UPDATE Changes<dd>
**   For each UPDATE change, the function checks if the target database
**   contains a row with the same primary key value (or values) as the
**   original row values stored in the changeset. If it does, and the values
**   stored in all modified non-primary key columns also match the values
**   stored in the changeset the row is updated within the target database.
**
**   If a row with matching primary key values is found, but one or more of
**   the modified non-primary key fields contains a value different from an
**   original row value stored in the changeset, the conflict-handler function
**   is invoked with [SQLITE_CHANGESET_DATA] as the second argument. Since
**   UPDATE changes only contain values for non-primary key fields that are
**   to be modified, only those fields need to match the original values to
**   avoid the SQLITE_CHANGESET_DATA conflict-handler callback.
**
**   If no row with matching primary key values is found in the database,
**   the conflict-handler function is invoked with [SQLITE_CHANGESET_NOTFOUND]
**   passed as the second argument.
**
**   If the UPDATE operation is attempted, but SQLite returns
**   SQLITE_CONSTRAINT, the conflict-handler function is invoked with
**   [SQLITE_CHANGESET_CONSTRAINT] passed as the second argument.
**   This includes the case where the UPDATE operation is attempted after
**   an earlier call to the conflict handler function returned
**   [SQLITE_CHANGESET_REPLACE].
** </dl>
**
** It is safe to execute SQL statements, including those that write to the
** table that the callback related to, from within the xConflict callback.
** This can be used to further customize the application's conflict
** resolution strategy.
**
** All changes made by these functions are enclosed in a savepoint transaction.
** If any other error (aside from a constraint failure when attempting to
** write to the target database) occurs, then the savepoint transaction is
** rolled back, restoring the target database to its original state, and an
** SQLite error code returned.
**
** If the output parameters (ppRebase) and (pnRebase) are non-NULL and
** the input is a changeset (not a patchset), then sqlite3changeset_apply_v2()
** may set (*ppRebase) to point to a "rebase" that may be used with the
** sqlite3_rebaser APIs buffer before returning. In this case (*pnRebase)
** is set to the size of the buffer in bytes. It is the responsibility of the
** caller to eventually free any such buffer using sqlite3_free(). The buffer
** is only allocated and populated if one or more conflicts were encountered
** while applying the patchset. See comments surrounding the sqlite3_rebaser
** APIs for further details.
**
** The behavior of sqlite3changeset_apply_v2() and its streaming equivalent
** may be modified by passing a combination of
** [SQLITE_CHANGESETAPPLY_NOSAVEPOINT | supported flags] as the 9th parameter.
**
** Note that the sqlite3changeset_apply_v2() API is still <b>experimental</b>
** and therefore subject to change.
*/
SQLITE_API int sqlite3changeset_apply(
  sqlite3 *db,                    /* Apply change to "main" db of this handle */
  int nChangeset,                 /* Size of changeset in bytes */
  void *pChangeset,               /* Changeset blob */
  int(*xFilter)(
    void *pCtx,                   /* Copy of sixth arg to _apply() */
    const char *zTab              /* Table name */
  ),
  int(*xConflict)(
    void *pCtx,                   /* Copy of sixth arg to _apply() */
    int eConflict,                /* DATA, MISSING, CONFLICT, CONSTRAINT */
    sqlite3_changeset_iter *p     /* Handle describing change and conflict */
  ),
  void *pCtx                      /* First argument passed to xConflict */
);
SQLITE_API int sqlite3changeset_apply_v2(
  sqlite3 *db,                    /* Apply change to "main" db of this handle */
  int nChangeset,                 /* Size of changeset in bytes */
  void *pChangeset,               /* Changeset blob */
  int(*xFilter)(
    void *pCtx,                   /* Copy of sixth arg to _apply() */
    const char *zTab              /* Table name */
  ),
  int(*xConflict)(
    void *pCtx,                   /* Copy of sixth arg to _apply() */
    int eConflict,                /* DATA, MISSING, CONFLICT, CONSTRAINT */
    sqlite3_changeset_iter *p     /* Handle describing change and conflict */
  ),
  void *pCtx,                     /* First argument passed to xConflict */
  void **ppRebase, int *pnRebase, /* OUT: Rebase data */
  int flags                       /* SESSION_CHANGESETAPPLY_* flags */
);

/*
** CAPI3REF: Flags for sqlite3changeset_apply_v2
**
** The following flags may passed via the 9th parameter to
** [sqlite3changeset_apply_v2] and [sqlite3changeset_apply_v2_strm]:
**
** <dl>
** <dt>SQLITE_CHANGESETAPPLY_NOSAVEPOINT <dd>
**   Usually, the sessions module encloses all operations performed by
**   a single call to apply_v2() or apply_v2_strm() in a [SAVEPOINT]. The
**   SAVEPOINT is committed if the changeset or patchset is successfully
**   applied, or rolled back if an error occurs. Specifying this flag
**   causes the sessions module to omit this savepoint. In this case, if the
**   caller has an open transaction or savepoint when apply_v2() is called,
**   it may revert the partially applied changeset by rolling it back.
**
** <dt>SQLITE_CHANGESETAPPLY_INVERT <dd>
**   Invert the changeset before applying it. This is equivalent to inverting
**   a changeset using sqlite3changeset_invert() before applying it. It is
**   an error to specify this flag with a patchset.
**
** <dt>SQLITE_CHANGESETAPPLY_IGNORENOOP <dd>
**   Do not invoke the conflict handler callback for any changes that
**   would not actually modify the database even if they were applied.
**   Specifically, this means that the conflict handler is not invoked
**   for:
**    <ul>
**    <li>a delete change if the row being deleted cannot be found,
**    <li>an update change if the modified fields are already set to
**        their new values in the conflicting row, or
**    <li>an insert change if all fields of the conflicting row match
**        the row being inserted.
**    </ul>
**
** <dt>SQLITE_CHANGESETAPPLY_FKNOACTION <dd>
**   If this flag it set, then all foreign key constraints in the target
**   database behave as if they were declared with "ON UPDATE NO ACTION ON
**   DELETE NO ACTION", even if they are actually CASCADE, RESTRICT, SET NULL
**   or SET DEFAULT.
*/
#define SQLITE_CHANGESETAPPLY_NOSAVEPOINT   0x0001
#define SQLITE_CHANGESETAPPLY_INVERT        0x0002
#define SQLITE_CHANGESETAPPLY_IGNORENOOP    0x0004
#define SQLITE_CHANGESETAPPLY_FKNOACTION    0x0008

/*
** CAPI3REF: Constants Passed To The Conflict Handler
**
** Values that may be passed as the second argument to a conflict-handler.
**
** <dl>
** <dt>SQLITE_CHANGESET_DATA<dd>
**   The conflict handler is invoked with CHANGESET_DATA as the second argument
**   when processing a DELETE or UPDATE change if a row with the required
**   PRIMARY KEY fields is present in the database, but one or more other
**   (non primary-key) fields modified by the update do not contain the
**   expected "before" values.
**
**   The conflicting row, in this case, is the database row with the matching
**   primary key.
**
** <dt>SQLITE_CHANGESET_NOTFOUND<dd>
**   The conflict handler is invoked with CHANGESET_NOTFOUND as the second
**   argument when processing a DELETE or UPDATE change if a row with the
**   required PRIMARY KEY fields is not present in the database.
**
**   There is no conflicting row in this case. The results of invoking the
**   sqlite3changeset_conflict() API are undefined.
**
** <dt>SQLITE_CHANGESET_CONFLICT<dd>
**   CHANGESET_CONFLICT is passed as the second argument to the conflict
**   handler while processing an INSERT change if the operation would result
**   in duplicate primary key values.
**
**   The conflicting row in this case is the database row with the matching
**   primary key.
**
** <dt>SQLITE_CHANGESET_FOREIGN_KEY<dd>
**   If foreign key handling is enabled, and applying a changeset leaves the
**   database in a state containing foreign key violations, the conflict
**   handler is invoked with CHANGESET_FOREIGN_KEY as the second argument
**   exactly once before the changeset is committed. If the conflict handler
**   returns CHANGESET_OMIT, the changes, including those that caused the
**   foreign key constraint violation, are committed. Or, if it returns
**   CHANGESET_ABORT, the changeset is rolled back.
**
**   No current or conflicting row information is provided. The only function
**   it is possible to call on the supplied sqlite3_changeset_iter handle
**   is sqlite3changeset_fk_conflicts().
**
